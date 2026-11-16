//! The Attack command and the Attack and Kill combat actions.
//!
//! The damage arithmetic is shared with the menu's preview (combat.rs). What this adds is what the
//! game writes while it fights: default relation records from its relation lookups, the defender's
//! unlocked-tech cache refresh in the defence bonus, and the advance path search.

use crate::action::{Action, Combat, MoveEffect, Payload};
use crate::combat::{battle, data, has};
use crate::state::{Game, Unit, Xy};
use crate::state::{COOL_DOWN, PEACE};
use crate::step::{
    attack_diplomacy, facing_direction, player_index, refresh_tech_cache, relation_state,
};
use crate::tables::Tables;

const BOOSTED: i64 = 2;
const INVISIBLE: i64 = 3;
const SWIFT: i64 = 6;
const DOUBLE_READY: i64 = 7;
const CITY: i32 = 1;
const ICE: i32 = 6;
const PACIFIST_TASK: usize = 1;
const KILLER_TASK: usize = 5;

fn effects_mut(g: &mut Game, node: u32) -> Result<&mut crate::inline::Inline<i64, 9>, String> {
    let list = g.units[node as usize].effects as usize;
    g.effect_lists
        .get_mut(list)
        .ok_or_else(|| "effect list".into())
}

fn remove_effect(g: &mut Game, node: u32, effect: i64) -> Result<(), String> {
    let e = effects_mut(g, node)?;
    if let Some(i) = e.iter().position(|x| *x == effect) {
        e.remove(i);
    }
    Ok(())
}

/// Attack options at a position with their state writes (the relation lookup for
/// an invisible foreign unit, the peace checks, a capturer's city-owner checks): the targets around
/// `center` that `unit` (attacking for `viewer`) could attack
#[allow(clippy::too_many_arguments)] // the rule's inputs, passed as the step holds them
fn options_at(
    g: &mut Game,
    t: &Tables,
    viewer: i32,
    center: Xy,
    range: i32,
    include_hidden: bool,
    unit: &Unit,
    ignore: bool,
) -> Result<crate::combat::Targets, String> {
    let capturer = has(t, unit, "infiltrate")?;
    let drench = has(t, unit, "drench")?;
    let mut out = crate::combat::Targets::new();
    if range < 0 {
        return Ok(out);
    }
    let h = g.tiles.len() as i32 / g.width;
    for x in (center.x - range).max(0)..=(center.x + range).min(g.width - 1) {
        for y in (center.y - range).max(0)..=(center.y + range).min(h - 1) {
            let at = Xy { x, y };
            let i = g.index(at).unwrap();
            if at == center || !include_hidden && !g.tiles[i].explorers.contains(&viewer) {
                continue;
            }
            if capturer {
                let x = g.tiles[i];
                if x.improvement.as_ref().is_some_and(|m| m.kind == CITY)
                    && x.owner != 0
                    && x.owner != unit.owner
                {
                    let r = relation_state(g, viewer, x.owner)?;
                    let captured = x.unit.is_some_and(|n| g.units[n as usize].owner != x.owner);
                    let robbed = x.improvement.as_ref().unwrap().effects.contains(&1);
                    if r == PEACE || captured || robbed || !ignore && r == COOL_DOWN {
                        continue;
                    }
                    out.push(at);
                }
                continue;
            }
            if drench && crate::aquarion::floodable(&g.tiles[i]) {
                out.push(at);
                continue;
            }
            let Some(n) = g.tiles[i].unit else { continue };
            let owner = g.units[n as usize].owner;
            let invisible =
                crate::menu::unit_effect_list(g, g.units[n as usize].effects).contains(&INVISIBLE);
            if owner != viewer && invisible && relation_state(g, viewer, owner)? != PEACE {
                continue;
            }
            if owner != viewer
                && (ignore || {
                    let r = relation_state(g, viewer, owner)?;
                    r != PEACE && r != COOL_DOWN
                })
            {
                out.push(at);
            }
        }
    }
    Ok(out)
}

/// Attack options (the gate, then options at around the unit as its owner)
fn option_writes(
    g: &mut Game,
    t: &Tables,
    unit: &Unit,
    range: i32,
    ignore: bool,
) -> Result<(), String> {
    if unit.leader != 0
        || data(t, unit)?.attack.wrapping_mul(10) == 0
            && !has(t, unit, "convert")?
            && !has(t, unit, "infiltrate")?
    {
        return Ok(());
    }
    options_at(
        g,
        t,
        unit.owner,
        unit.coordinates,
        range,
        false,
        unit,
        ignore,
    )?;
    Ok(())
}

/// The game's writes for Attack admission (its attack options).
pub fn admit_writes(g: &mut Game, t: &Tables, unit: &Unit) -> Result<(), String> {
    option_writes(g, t, unit, data(t, unit)?.range, false)
}

/// Battle results (accounting for peace, full data) with their state writes:
/// (attack damage, retaliation or -1, should move to target, should reveal the defender)
pub(crate) fn battle_full(
    g: &mut Game,
    t: &Tables,
    a: &Unit,
    d: &Unit,
) -> Result<(i32, i32, bool, bool), String> {
    if d.owner == a.owner || {
        let r = relation_state(g, a.owner, d.owner)?;
        r == PEACE || r == COOL_DOWN
    } {
        return Ok((0, -1, false, false));
    }
    let (dealt, back, bonus_reached, retaliation_reached) = battle(g, t, a, d)?;
    if bonus_reached
        && g.tile(d.coordinates).is_some()
        && d.owner != 0
        && player_index(g, d.owner).is_ok()
    {
        refresh_tech_cache(g, t, d.owner)?;
    }
    if retaliation_reached
        && !(has(t, d, "stiff")?
            || has(t, a, "convert")?
            || has(t, a, "surprise")?
            || crate::menu::unit_effect_list(g, d.effects).contains(&0)
            || has(t, a, "freeze")?)
    {
        option_writes(g, t, d, data(t, d)?.range, true)?;
    }
    let mut advance = false;
    if dealt >= d.health && data(t, a)?.range == 1 {
        refresh_tech_cache(g, t, a.owner)?;
        advance = crate::movement::can_advance(g, t, a, d.coordinates)?;
    }
    let reveal = dealt > 0 && !crate::menu::unit_effect_list(g, d.effects).contains(&INVISIBLE);
    Ok((dealt, back, advance, reveal))
}

/// The Attack command's children
pub fn attack_command(
    g: &mut Game,
    t: &Tables,
    p: i32,
    unit: u32,
    origin: Xy,
    target: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let id = u64::from(unit);
    let node = g
        .tiles
        .iter()
        .filter_map(|x| x.unit)
        .find(|&n| u64::from(g.units[n as usize].id) == id)
        .ok_or("attack with a missing unit")?;
    let u = g.units[node as usize];
    let range = data(t, &u)?.range;
    crate::step::base(out, p, 2);
    // the unit or its passenger infiltrates; the Infiltrate action then reads the tile's unit
    let infiltrator = has(t, &u, "infiltrate")?
        || u.passenger
            .is_some_and(|n| has(t, &g.units[n as usize], "infiltrate").unwrap_or(false));
    if infiltrator && !has(t, &u, "consumed")? {
        // infiltration: the cloak goes into the city instead of fighting
        out.push(Action::Combat(Combat::Infiltrate {
            player: p,
            origin: u.coordinates,
            target,
        }));
        g.units[node as usize].moved = true;
        g.units[node as usize].attacked = true;
        let owner = g.tile(target).ok_or("infiltrate target")?.owner;
        attack_diplomacy(g, p, owner, out)?;
        return Ok(());
    }
    {
        let a = "consumed";
        if has(t, &u, a)?
            || u.passenger
                .is_some_and(|n| has(t, &g.units[n as usize], a).unwrap_or(false))
        {
            return Err(format!("attacking with {a} not implemented"));
        }
    }
    let defender = g
        .tile(target)
        .ok_or("attack target")?
        .unit
        .map(|n| g.units[n as usize]);
    let (mut dealt, mut back, mut advance, mut reveal) = (0, 0, false, false);
    let mut killed = true;
    if let Some(d) = &defender {
        let peace = d.owner == p || {
            let r = relation_state(g, p, d.owner)?;
            r == PEACE || r == COOL_DOWN
        };
        if !peace {
            player_index(g, d.owner)?;
            attack_diplomacy(g, p, d.owner, out)?;
            (dealt, back, advance, reveal) = battle_full(g, t, &u, d)?;
            killed = dealt >= d.health;
            if has(t, &u, "convert")? && !killed && d.owner != 255 {
                out.push(Action::Combat(Combat::Convert {
                    player: p,
                    origin,
                    target,
                }));
            }
            if killed && has(t, &u, "eat")? && d.owner != 255 {
                crate::linked::eat_after_kill(g, p, node, out)?;
            }
        }
    }
    if back < u.health {
        out.push(Action::MoveEffect(MoveEffect::Reselect {
            player: p,
            unit: id as u32,
        }));
    }
    // an Escape unit can still move after attacking; an Escape skater only while it stands on ice
    let escape = has(t, &u, "escape")?;
    let mut moved = !escape;
    if escape && has(t, &u, "skate")? {
        let at = if advance { target } else { u.coordinates };
        moved = g.tile(at).ok_or("skate attack tile")?.terrain != ICE;
    }
    g.units[node as usize].moved = moved;
    if crate::menu::unit_effect_list(g, u.effects).contains(&DOUBLE_READY) {
        remove_effect(g, node, DOUBLE_READY)?;
    } else {
        g.units[node as usize].attacked = true;
    }
    if killed && has(t, &u, "persist")? {
        // Persist: another target in range from where the unit ends up keeps it able to attack
        let position = if advance { target } else { origin };
        let sight = 1 + i32::from(has(t, &u, "scout")?);
        let targets = options_at(g, t, p, position, range, range <= sight, &u, false)?;
        if targets.iter().any(|&c| c != target) {
            g.units[node as usize].attacked = false;
        }
    }
    if has(t, &u, "freeze")? {
        // a freezing attack hits and freezes in one action, with no advance or retaliation
        out.push(Action::Combat(Combat::FreezeUnit {
            player: p,
            origin,
            target,
            damage: dealt,
        }));
        return Ok(());
    }
    let delay = if advance {
        out.push(Action::MoveEffect(MoveEffect::Move {
            player: p,
            unit: id as u32,
            path: crate::action::Path::from_iter([target, origin]),
            should_animate: false,
            reason: 1,
        }));
        100
    } else if back > 0 {
        out.push(Action::Combat(Combat::Attack {
            player: p,
            origin: target,
            target: origin,
            damage: back,
            animation: 1,
            should_move_to_target: false,
            delay: 100,
        }));
        250
    } else {
        100
    };
    if has(t, &u, "splash")? && range > 1 {
        // Splash: half damage on every unit around the target not at peace with the attacker
        for x in target.x - 1..=target.x + 1 {
            for y in target.y - 1..=target.y + 1 {
                let c = Xy { x, y };
                if c == target {
                    continue;
                }
                let Some(n) = g.tile(c).and_then(|x| x.unit) else {
                    continue;
                };
                let d = g.units[n as usize];
                let peace = d.owner == p || {
                    let r = relation_state(g, p, d.owner)?;
                    r == PEACE || r == COOL_DOWN
                };
                if peace {
                    continue;
                }
                if has(t, &u, "poison")? {
                    out.push(Action::Combat(Combat::Poison {
                        player: p,
                        origin,
                        target: c,
                    }));
                    continue;
                }
                let damage = half_damage(battle_full(g, t, &u, &d)?.0);
                out.push(Action::Combat(Combat::Attack {
                    player: p,
                    origin,
                    target: c,
                    damage,
                    animation: 2,
                    should_move_to_target: false,
                    delay: 20,
                }));
            }
        }
    }
    out.push(Action::Combat(Combat::Attack {
        player: p,
        origin,
        target,
        damage: dealt,
        animation: 1,
        should_move_to_target: advance,
        delay,
    }));
    if crate::menu::unit_effect_list(g, u.effects).contains(&INVISIBLE) {
        out.push(Action::MoveEffect(MoveEffect::Reveal {
            player: p,
            coordinates: u.coordinates,
            origin: Xy { x: -1, y: -1 },
            show_popup: false,
        }));
    }
    if reveal {
        if g.records_achievements() {
            return Err("achievement tracking not implemented".into());
        }
        let i = player_index(g, p)?;
        reset_peace(g, i);
    }
    Ok(())
}

/// Half the attack damage, rounded down to a multiple of 10
pub(crate) fn half_damage(damage: i32) -> i32 {
    (damage / 20).wrapping_mul(10)
}

/// an attack restarts the count of peaceful turns, unless the Pacifist task is complete
pub(crate) fn reset_peace(g: &mut Game, i: usize) {
    if let Some(task) = g.players[i].task_mut(PACIFIST_TASK) {
        if !task.flags.completed {
            task.peaceful_turns = Some(0);
        }
    }
}

fn set_direction(g: &mut Game, node: u32, from: Xy, to: Xy) {
    let d = facing_direction(from, to);
    g.units[node as usize].direction = d;
    g.units[node as usize].flipped = d < 4;
}

/// The Combat Attack action
pub fn perform_attack(
    g: &mut Game,
    t: &Tables,
    supplied: i32,
    origin: Xy,
    target: Xy,
    damage: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let source = g.tile(origin).and_then(|c| c.unit);
    let ti = g.index(target).ok_or("attack target tile")?;
    let defender = g.tiles[ti].unit;
    if let Some(s) = source {
        set_direction(g, s, origin, target);
        remove_effect(g, s, BOOSTED)?;
    }
    let actor = if origin != target {
        source.map_or(supplied, |s| g.units[s as usize].owner)
    } else {
        supplied
    };
    let actor_index = player_index(g, actor).ok();
    if let Some(d) = defender {
        set_direction(g, d, target, origin);
        remove_effect(g, d, BOOSTED)?;
        remove_effect(g, d, SWIFT)?;
        let snapshot = g.units[d as usize];
        out.extend(crate::aquarion::bubble_lost(g, &snapshot));
        let signed = snapshot.health as i16;
        let dealt = damage.min(i32::from(signed)) as i16;
        let health = signed.wrapping_sub(dealt) as u16;
        g.units[d as usize].health = i32::from(health);
        if health == 0 {
            if snapshot.owner != 255 {
                let ai = actor_index.ok_or("attack killer missing")?;
                g.players[ai].kills += 1;
                if origin != (Xy { x: -1, y: -1 }) {
                    if let Some(s) = source {
                        let su = g.units[s as usize];
                        if !has(t, &su, "carry")?
                            && !data(t, &su)?.hidden
                            && !has(t, &su, "static")?
                        {
                            g.units[s as usize].xp += 1;
                            if g.records_achievements() {
                                return Err("achievement tracking not implemented".into());
                            }
                        }
                    }
                }
                match g.players[ai].task_mut(KILLER_TASK) {
                    None => out.push(Action::EnableTask {
                        player: actor,
                        task: KILLER_TASK as u32,
                    }),
                    Some(task) => {
                        // a completed task stays complete; else count the kill
                        let bumped = if task.flags.completed {
                            true
                        } else {
                            let k = task.kills.unwrap_or(0) + 1;
                            task.kills = Some(k);
                            k >= 10
                        };
                        if bumped {
                            out.extend(crate::step::task_notification(g, t, ai, KILLER_TASK)?);
                        }
                    }
                }
            }
            let o = player_index(g, snapshot.owner)?;
            g.players[o].casualties += 1;
            out.push(Action::Combat(Combat::Kill {
                player: actor,
                coordinates: target,
            }));
        } else if let Some(s) = source {
            if has(t, &g.units[s as usize].clone(), "poison")? {
                out.push(Action::Combat(Combat::Poison {
                    player: actor,
                    origin,
                    target,
                }));
            }
        }
    }
    // Drench: the target tile floods after the hit
    if let Some(s) = source {
        if has(t, &g.units[s as usize].clone(), "drench")?
            && crate::aquarion::floodable(&g.tiles[ti])
        {
            out.push(Action::Combat(Combat::Flood {
                player: actor,
                coordinates: target,
            }));
        }
    }
    Ok(())
}

/// The Combat Kill action
pub fn kill(g: &mut Game, t: &Tables, c: Xy, out: &mut Vec<Action>) -> Result<(), String> {
    let i = g.index(c).ok_or("kill tile")?;
    let n = g.tiles[i].unit.ok_or("kill with no unit")?;
    g.units[n as usize].health = 0;
    let u = g.units[n as usize];
    // the follower of a dead head takes its flags and grows into a centipede; the leader lets go
    if u.follower != 0 {
        if let Some(f) = crate::linked::find(g, u.follower) {
            let fu = &mut g.units[f as usize];
            fu.moved = u.moved;
            fu.attacked = u.attacked;
            out.push(Action::Combat(Combat::Upgrade {
                player: u.owner,
                kind: t.units.id("centipede")? as i32,
                coordinates: fu.coordinates,
                cost: 0,
            }));
        }
    }
    if u.leader != 0 {
        if let Some(l) = crate::linked::find(g, u.leader) {
            g.units[l as usize].follower = 0;
        }
    }
    if crate::menu::unit_effect_list(g, u.effects).contains(&1) {
        crate::cymanti::spores(g, t, c, out)?;
    }
    let score = crate::units::unit_score(g, t, &u);
    g.tiles[i].unit = None;
    out.push(Action::Combat(Combat::DecreaseScore {
        player: u.owner,
        amount: score,
    }));
    if has(t, &u, "tentacles")? {
        crate::aquarion::update_tentacles(g, t)?;
    }
    Ok(())
}

/// The Combat Convert action
pub fn convert(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    origin: Xy,
    target: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let source = g.tile(origin).ok_or("convert origin")?.unit;
    let id = g
        .tile(target)
        .and_then(|c| c.unit)
        .ok_or("convert target unit")?;
    let original = g.units[id as usize];
    if original.owner == actor {
        return Ok(());
    }
    let source = source.ok_or("convert source unit")?;
    let converter = g.units[source as usize];
    let p = player_index(g, converter.owner)?;
    if original.owner != 255 {
        if let Some(task) = g.players[p].task_mut(9) {
            if task.flags.started {
                if !task.flags.completed {
                    task.conversions = Some(task.conversions.unwrap_or(0) + 1);
                }
                out.extend(crate::step::task_notification(g, t, p, 9)?);
            }
        }
    }
    // a converted chain member brings its neighbours in the chain along
    for linked in [original.follower, original.leader] {
        if linked != 0 {
            if let Some(n) = crate::linked::find(g, linked) {
                let l = &g.units[n as usize];
                if l.owner != actor {
                    out.push(Action::Combat(Combat::Convert {
                        player: actor,
                        origin: original.coordinates,
                        target: l.coordinates,
                    }));
                }
            }
        }
    }
    let escape = has(t, &converter, "escape")?;
    g.units[source as usize].moved = !escape;
    g.units[source as usize].attacked = true;
    let u = &mut g.units[id as usize];
    u.owner = converter.owner;
    u.moved = true;
    u.attacked = true;
    let passenger = u.passenger;
    if !has(t, &original, "independent")? {
        let capital = crate::step::current_capital(g, g.players[p].action.id)?;
        g.units[id as usize].home = capital;
        if let Some(n) = passenger {
            g.units[n as usize].owner = converter.owner;
            g.units[n as usize].home = capital;
        }
    } else if let Some(n) = passenger {
        g.units[n as usize].owner = converter.owner;
    }
    let score = crate::units::unit_score(g, t, &g.units[id as usize]);
    out.push(Action::Payload(Payload::IncreaseScore {
        player: actor,
        amount: score,
        source: target,
        delay: 0,
    }));
    out.push(Action::Combat(Combat::DecreaseScore {
        player: original.owner,
        amount: score,
    }));
    let kind = g.units[id as usize].kind;
    let data = t
        .units
        .get(kind as usize)
        .ok_or("converted unit of unknown kind")?;
    let sight = 1 + i32::from(data.unit_abilities.has("scout"));
    crate::explore::explore_from(g, converter.owner, target, sight, out)?;
    Ok(())
}

/// Area damage (Stomp): a half-damage attack on every non-peaceful neighbour
pub fn area_damage(
    g: &mut Game,
    t: &Tables,
    node: u32,
    center: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let attacker = g.units[node as usize];
    for x in center.x - 1..=center.x + 1 {
        for y in center.y - 1..=center.y + 1 {
            let target = Xy { x, y };
            if target == center {
                continue;
            }
            let Some(h) = g.tile(target).and_then(|c| c.unit) else {
                continue;
            };
            let d = g.units[h as usize];
            let peace = d.owner == attacker.owner || {
                let r = relation_state(g, attacker.owner, d.owner)?;
                r == PEACE || r == COOL_DOWN
            };
            if peace {
                continue;
            }
            let (dealt, _, _, reveal) = battle_full(g, t, &attacker, &d)?;
            if reveal {
                let i = player_index(g, attacker.owner)?;
                reset_peace(g, i);
                attack_diplomacy(g, attacker.owner, d.owner, out)?;
                if g.records_achievements() {
                    return Err("achievement tracking not implemented".into());
                }
            }
            let tentacles = has(t, &attacker, "tentacles")?;
            let damage = if tentacles { dealt } else { half_damage(dealt) };
            out.push(Action::Combat(Combat::Attack {
                player: attacker.owner,
                origin: center,
                target,
                damage,
                animation: if tentacles { 3 } else { 2 },
                should_move_to_target: false,
                delay: 20,
            }));
        }
    }
    Ok(())
}
