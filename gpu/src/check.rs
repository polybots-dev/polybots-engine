//! A recorded game through the kernel sources: at every command, the host build's view, legal
//! list and step against engine-fast's on the same state. engine-fast plays its own state
//! through the whole game (no reload at turn handoffs); whether engine-fast itself agrees with
//! the game is a question for recorded games, not for this check.
//!
//! What it counts: decisions whose list, view and step were compared equal; steps the port
//! does not take yet (not ported, by reason) and capacity stops, both counted and never taken
//! as agreement. The first difference ends the game as `Differs`; a stop of engine-fast's own
//! step ends it as `Refused` once the port has given the same reason.

use crate::state::{self as device};
use crate::tables::DeviceTables;
use crate::{menu as rows, step as kernel, view as views};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::{step, view};
use engine_fast_frame::Action;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// every command compared, or stopped where engine-fast stops with the port's reason
    Exact,
    /// engine-fast's step stops here (the port gave the same reason)
    Refused { at: usize, rule: String },
    /// the port differs from engine-fast at command `at`, in `what`
    Differs { at: usize, what: String },
    /// the game outgrew the device block at command `at`; nothing after it is compared
    Capacity { at: usize, reason: String },
}

#[derive(Default)]
pub struct Report {
    pub outcome: Option<Outcome>,
    /// commands whose view, list and step all compared equal
    pub compared: usize,
    /// commands the port's step does not take yet, by reason (not compared)
    pub not_ported: BTreeMap<String, u64>,
    /// commands stopped on a device capacity (not compared)
    pub capacity: BTreeMap<String, u64>,
    /// compared steps by command kind
    pub kinds: BTreeMap<i32, u64>,
    /// the longest legal list seen
    pub longest_list: usize,
    /// attack previews compared equal (rows that have one)
    pub previews: u64,
    /// blocks and moves kept for a device comparison (every `keep_every`th command)
    pub kept: Vec<(device::Block, Action)>,
    /// the smallest and largest value of each field the narrowing of the block
    /// sizes, over every state after a compared step
    pub ranges: BTreeMap<&'static str, (i64, i64)>,
}

/// Widen `ranges[name]` to hold `v`.
fn seen(ranges: &mut BTreeMap<&'static str, (i64, i64)>, name: &'static str, v: i64) {
    let r = ranges.entry(name).or_insert((v, v));
    r.0 = r.0.min(v);
    r.1 = r.1.max(v);
}

/// The ranges of the block's fields in one state: counts, and the values of the fields of
/// units in use, effect lists, tiles, improvements and players.
pub fn ranges_of(b: &device::Block, ranges: &mut BTreeMap<&'static str, (i64, i64)>) {
    let tiles = &b.tiles()[..b.tile_count as usize];
    let mut units = 0;
    for u in b.units().iter().filter(|u| u.used_n != 0) {
        units += 1;
        for (name, v) in [
            ("unit.id", i64::from(u.id)),
            ("unit.leader", i64::from(u.leader)),
            ("unit.follower", i64::from(u.follower)),
            ("unit.owner", i64::from(u.owner_n)),
            ("unit.kind", i64::from(u.kind_n)),
            ("unit.health", i64::from(u.health_n)),
            ("unit.promotion_level", i64::from(u.promotion_level_n)),
            ("unit.xp", i64::from(u.xp_n)),
            ("unit.direction", i64::from(u.direction_n)),
            ("unit.created_turn", i64::from(u.created_turn_n)),
            ("unit.effects", i64::from(u.effects_n)),
            ("unit.birth_climate", i64::from(u.birth_climate_n)),
            ("unit.birth_climate_skin", i64::from(u.birth_climate_skin_n)),
            ("unit.home.x", i64::from(u.home.x)),
            (
                "unit.previous.x",
                i64::from(u.previous_turn_end_coordinates.x),
            ),
        ] {
            seen(ranges, name, v);
        }
    }
    seen(ranges, "units in use", units);
    let mut lists = 0;
    for l in b.lists().iter().filter(|l| l.used_n != 0) {
        lists += 1;
        seen(ranges, "effect_list.len", i64::from(l.len_n));
        for &x in &l.items_n[..l.len_n.max(0) as usize] {
            seen(ranges, "effect_list.item", i64::from(x));
        }
    }
    seen(ranges, "effect lists in use", lists);
    for t in tiles {
        for (name, v) in [
            ("tile.terrain", i64::from(t.terrain_n)),
            ("tile.climate", i64::from(t.climate_n)),
            ("tile.altitude", i64::from(t.altitude_n)),
            ("tile.owner", i64::from(t.owner_n)),
            ("tile.capital_of", i64::from(t.capital_of_n)),
            ("tile.explorers_len", i64::from(t.explorers_len_n)),
            ("tile.resource", i64::from(t.resource_n)),
            ("tile.continent", i64::from(t.continent_n)),
            (
                "tile.last_population_check",
                i64::from(t.last_population_check_n),
            ),
            (
                "tile.available_population",
                i64::from(t.available_population_n),
            ),
            ("tile.skin", i64::from(t.skin_n)),
            ("tile.effects_len", i64::from(t.effects_len_n)),
        ] {
            seen(ranges, name, v);
        }
        for &e in &t.explorers_n[..t.explorers_len_n.max(0) as usize] {
            seen(ranges, "tile.explorer", i64::from(e));
        }
        for &e in &t.effects_n[..t.effects_len_n.max(0) as usize] {
            seen(ranges, "tile.effect", i64::from(e));
        }
        if t.has_improvement_n != 0 {
            let m = &t.improvement;
            for (name, v) in [
                ("improvement.kind", i64::from(m.kind_n)),
                ("improvement.owner", i64::from(m.owner_n)),
                ("improvement.founder", i64::from(m.founder_n)),
                ("improvement.level", i64::from(m.level_n)),
                ("improvement.founded", i64::from(m.founded_n)),
                ("improvement.xp", i64::from(m.xp_n)),
                ("improvement.population", i64::from(m.population_n)),
                ("improvement.production", i64::from(m.production_n)),
                ("improvement.base_score", i64::from(m.base_score_n)),
                ("improvement.border_size", i64::from(m.border_size_n)),
                ("improvement.upgrade", i64::from(m.upgrade_n)),
                ("improvement.effects_len", i64::from(m.effects_len_n)),
            ] {
                seen(ranges, name, v);
            }
            for &r in &m.rewards_n {
                seen(ranges, "improvement.reward", i64::from(r));
            }
        }
    }
    for p in &b.players[..b.player_count as usize] {
        for (name, v) in [
            ("player.currency", i64::from(p.currency)),
            ("player.cities", i64::from(p.cities)),
            ("player.kills", i64::from(p.kills)),
            ("player.casualties", i64::from(p.casualties)),
            ("player.score", p.score),
            ("player.end_score", p.end_score),
            ("player.available_tech_len", i64::from(p.available_tech_len)),
        ] {
            seen(ranges, name, v);
        }
    }
    for (name, v) in [
        ("game.current_turn", i64::from(b.current_turn)),
        ("game.current_unit_id", i64::from(b.current_unit_id)),
        ("game.actions_len", i64::from(b.actions_len)),
        ("game.triggers_len", i64::from(b.triggers_len)),
        ("game.naming_count", i64::from(b.naming_count)),
    ] {
        seen(ranges, name, v);
    }
}

impl Report {
    pub fn to_json(&self) -> Value {
        let (outcome, at, detail) = match self.outcome.as_ref().unwrap_or(&Outcome::Exact) {
            Outcome::Exact => ("exact", self.compared, String::new()),
            Outcome::Refused { at, rule } => ("refused", *at, rule.clone()),
            Outcome::Differs { at, what } => ("differs", *at, what.clone()),
            Outcome::Capacity { at, reason } => ("capacity", *at, reason.clone()),
        };
        let kinds: BTreeMap<String, u64> = self
            .kinds
            .iter()
            .map(|(k, n)| (k.to_string(), *n))
            .collect();
        json!({"outcome": outcome, "at": at, "detail": detail, "compared": self.compared,
            "not_ported": self.not_ported, "capacity": self.capacity, "kinds": kinds,
            "longest_list": self.longest_list, "previews_compared": self.previews, "ranges": self.ranges})
    }
}

/// engine-fast's legal list as the batch hands it out: rows sorted by the first five fields.
pub fn engine_rows(g: &Game, t: &Tables) -> Result<Vec<rows::Row>, String> {
    engine_list(g, t).map(|(rows, _)| rows)
}

/// The batch's legal rows and each row's attack preview, from engine-fast's own menu.
pub fn engine_list(g: &Game, t: &Tables) -> Result<(Vec<rows::Row>, Vec<[i16; 4]>), String> {
    engine_fast_batch::outputs::legal_previews(g, t)
}

fn is_capacity(stop: &kernel::Stop) -> bool {
    kernel::is_capacity(stop.code)
}

/// Play `commands` from `g` on engine-fast, comparing the port at each. `players` holds each
/// command's recorded player (empty: not checked); a command by a player other than the one
/// to move ends the game as refused.
pub fn check(
    mut g: Game,
    t: &Tables,
    flat: &DeviceTables,
    commands: &[Action],
    players: &[i32],
    keep_every: usize,
) -> Report {
    let mut report = Report::default();
    for (at, action) in commands.iter().enumerate() {
        let differs = |what: String| Outcome::Differs { at, what };
        let block = match device::to_device(&g) {
            Ok(b) => b,
            Err(reason) => {
                report.outcome = Some(Outcome::Capacity { at, reason });
                return report;
            }
        };
        // the view of the player to move
        let mover = g.players[g.current_player_index as usize].action.id;
        if players.get(at).is_some_and(|&p| p != mover) {
            report.outcome = Some(Outcome::Refused {
                at,
                rule: format!(
                    "{} by a player other than the one to move not implemented",
                    action.kind
                ),
            });
            return report;
        }
        let mut want_view = view::View::default();
        let want_view = view::write(&g, t, mover, &mut want_view).map(|_| want_view);
        let got_view = views::twin(&block, flat, mover);
        if got_view != want_view {
            report.outcome = Some(differs("view".into()));
            return report;
        }
        // the legal list
        // the legal list, and its rows' attack previews
        let want_rows = engine_list(&g, t);
        match (rows::twin_menu_previews(&block, flat), &want_rows) {
            (Ok(a), Ok(b)) if &a == b => {
                report.longest_list = report.longest_list.max(a.0.len());
                report.previews += a.1.iter().filter(|p| p[0] == 1).count() as u64;
            }
            (Err(s), _) if is_capacity(&s) => {
                *report.capacity.entry(s.text).or_default() += 1;
            }
            (Err(s), Err(e)) if &s.text == e => {}
            (Ok(a), Ok(b)) if a.0 == b.0 => {
                report.outcome = Some(differs("attack previews".into()));
                return report;
            }
            _ => {
                report.outcome = Some(differs("legal list".into()));
                return report;
            }
        }
        // the step
        let before = g.clone();
        let mut ours = device::to_device(&g).expect("the block was made above");
        let rust = step::command(&mut g, t, action, |_| {});
        let port = kernel::twin_command(&mut ours, flat, action);
        if keep_every > 0 && at % keep_every == 0 {
            report.kept.push((block, action.clone()));
        }
        match (&rust, port) {
            (_, Err(s)) if s.not_ported().is_some() => {
                *report.not_ported.entry(s.text).or_default() += 1;
                if let Err(e) = rust {
                    report.outcome = Some(Outcome::Refused { at, rule: e });
                    return report;
                }
                continue;
            }
            (_, Err(s)) if is_capacity(&s) => {
                *report.capacity.entry(s.text).or_default() += 1;
                if let Err(e) = rust {
                    report.outcome = Some(Outcome::Refused { at, rule: e });
                    return report;
                }
                continue;
            }
            (Err(e), Err(s)) if &s.text == e => {
                report.outcome = Some(Outcome::Refused {
                    at,
                    rule: e.clone(),
                });
                return report;
            }
            (Ok(()), Ok(())) => {
                let want = device::canonical(&g);
                let got = device::from_device(&ours, &before).and_then(|x| device::canonical(&x));
                match (want, got) {
                    (Ok(want), Ok(mut got)) => {
                        got.side = want.side.clone();
                        got.memory = want.memory.clone();
                        if got != want {
                            report.outcome = Some(differs("state after the step".into()));
                            return report;
                        }
                        ranges_of(&ours, &mut report.ranges);
                    }
                    _ => {
                        report.outcome = Some(differs("state after the step (unreadable)".into()));
                        return report;
                    }
                }
            }
            (rust, port) => {
                let port = port.map_err(|s| s.text);
                report.outcome = Some(differs(format!(
                    "step result (engine {rust:?}, port {port:?}, action {action:?})"
                )));
                return report;
            }
        }
        report.compared += 1;
        *report.kinds.entry(action.kind).or_default() += 1;
    }
    report.outcome = Some(Outcome::Exact);
    report
}
