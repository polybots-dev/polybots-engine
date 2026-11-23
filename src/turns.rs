//! Turn changes: the EndTurn command (unit and improvement upkeep), the EndTurn and StartTurn actions
//! (turn order, income, embassy income, peace-request messages, first contact), the winner check and
//! the EndMatch command.

use crate::action::{Action, City, Payload, TurnEffect};
use crate::inline::Seats;
use crate::state::{Game, Relation, Xy};
use crate::state::{COOL_DOWN, NEUTRAL, PEACE};
use crate::step::{
    player_index, refresh_tech_cache, relation_entry, relation_of, task_notification,
};
use crate::tables::Tables;

const CITY: i32 = 1;
const PORT: i32 = 7;
const FROZEN: i64 = 0;
const POISONED: i64 = 1;
const PETRIFIED: i64 = 5;
const DOUBLE_READY: i64 = 7;
const PASS_AND_PLAY: i64 = 2;
const FINAL_TURN: i64 = 3;
const ENDED: i64 = 4;
const PEACE_REQUEST_MESSAGE: i64 = 1;
const EMBASSY_MESSAGE: i64 = 5;
const PEACE_REQUEST_TRIGGER: i64 = 2;
const PACIFIST_TASK: usize = 1;
const EXPLORER_TASK: usize = 7;

fn alive(g: &Game, p: i32) -> Result<bool, String> {
    crate::menu::alive(g, p)
}

fn ability(t: &Tables, kind: i32, a: &str) -> bool {
    t.units
        .get(kind as usize)
        .is_some_and(|d| d.unit_abilities.has(a))
}

fn improvement_has(t: &Tables, kind: i32, a: &str) -> bool {
    t.improvements
        .get(kind as usize)
        .is_some_and(|d| d.improvement_abilities.has(a))
}

/// The EndTurn command's upkeep and children
pub fn end_command(g: &mut Game, t: &Tables, p: i32, out: &mut Vec<Action>) -> Result<(), String> {
    crate::step::base(out, p, 15);
    out.push(Action::TurnEffect(TurnEffect::EndTurn { player: p }));
    if g.records_achievements() {
        return Err("achievement tracking not implemented".into());
    }
    for i in 0..g.tiles.len() {
        let c = g.tiles[i].coordinates;
        if let Some(n) = g.tiles[i].unit {
            let u = g.units[n as usize];
            if u.owner == p {
                let max = crate::combat::max_health(g, t, &u)?;
                let fx = crate::menu::unit_effect_list(g, u.effects);
                let recover = u.leader == 0
                    && (u.health < max || fx.contains(&POISONED))
                    && !u.moved
                    && !fx.contains(&FROZEN)
                    && !u.attacked;
                if recover {
                    out.push(Action::TurnEffect(TurnEffect::Heal {
                        player: p,
                        coordinates: c,
                        fixed_heal: 0,
                    }));
                }
                if ability(t, u.kind, "grow") {
                    grow(g, t, p, c, &u, out)?;
                }
                let m = g.unit_mut(n as usize);
                m.moved = false;
                m.attacked = false;
                m.previous_turn_end_coordinates = m.coordinates;
                let list = m.effects as usize;
                // the list is written (and its holders stamped, a scan of the board) only when
                // it changes
                let double = ability(t, u.kind, "doubleattack");
                let now = g.effect_lists.get(list).ok_or("effect list")?;
                let changes = now.contains(&FROZEN)
                    || now.contains(&PETRIFIED)
                    || double && !now.contains(&DOUBLE_READY);
                if changes {
                    let e = g.effects_mut(list).ok_or("effect list")?;
                    for x in [FROZEN, PETRIFIED] {
                        if let Some(k) = e.iter().position(|v| *v == x) {
                            e.remove(k);
                        }
                    }
                    if double && !e.contains(&DOUBLE_READY) {
                        e.try_push(DOUBLE_READY)?;
                    }
                }
            }
        }
        if g.tiles[i].owner == p {
            crate::cymanti::improvement_upkeep(g, t, p, c, out)?;
        }
    }
    crate::aquarion::update_tentacles(g, t)?;
    Ok(())
}

/// A Grow unit's upkeep: once its age reaches its kind's
/// growthRate it upgrades, free, to the unit kind whose upgradesFrom names it.
fn grow(
    g: &Game,
    t: &Tables,
    p: i32,
    c: Xy,
    u: &crate::state::Unit,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let data = t.units.get(u.kind as usize).ok_or("unknown unit kind")?;
    let name = t.units.name(u.kind as usize)?;
    let mut next = t
        .units
        .by_id
        .iter()
        .enumerate()
        .filter_map(|(k, e)| e.as_ref().map(|(_, d)| (k, d)))
        .filter(|(_, d)| d.upgrades_from.as_deref() == Some(name));
    // the game takes the first in its catalog's serialized order; a Grow unit has one
    // successor, so two are refused rather than ordered
    let Some((kind, _)) = next.next() else {
        return Ok(());
    };
    if next.next().is_some() {
        return Err("a Grow unit with two successors".into());
    }
    // a missing growthRate is the catalog default, 0
    if data.growth_rate.unwrap_or(0) as i64
        <= g.current_turn.wrapping_sub(u.created_turn) as u32 as i64
    {
        out.push(Action::TurnEffect(TurnEffect::Upgrade {
            player: p,
            coordinates: c,
            unit: kind as i32,
            cost: 0,
        }));
    }
    Ok(())
}

/// The winner: (rank-first player, whether the game is complete); refreshes every
/// player's capital_count_cache
pub fn winner(g: &mut Game) -> Result<(i32, bool), String> {
    let rules = g.fixed.clone();
    let playable = g.players.iter().filter(|p| p.action.id != 255).count();
    if !(2..=4).contains(&playable) || g.players.len() > 5 || rules.rules_mode == 7 {
        return Err("winner check outside 2-4 players or in Tutorial".into());
    }
    let (mode, game_type) = (rules.rules_mode, rules.game_type);
    let starts: Seats<i32> = g
        .players
        .iter()
        // Nature has no capital and is never counted
        .map(|p| -> Result<i32, String> {
            if p.action.id == 255 {
                return Ok(0);
            }
            Ok(g.tile(p.start_tile).ok_or("capital off the board")?.owner)
        })
        .collect::<Result<_, _>>()?;
    let ids = g.player_ids();
    let capitals = |id: i32| {
        ids.iter()
            .zip(&starts)
            .filter(|&(&p, &owner)| p != 255 && owner == id)
            .count() as i64
    };
    for (k, &id) in ids.iter().enumerate() {
        g.players[k].capital_count_cache = capitals(id);
    }
    let key = |k: usize| -> (i64, i64) {
        let p = &g.players[k];
        let primary = match mode {
            2 => i64::from(p.cities),
            4 => p.capital_count_cache,
            _ => 0,
        };
        (primary, p.score)
    };
    let mut ranked: Seats<usize> = (0..g.players.len()).collect();
    // small sort: a swap network for 2 or 3 entries, insertion above; descending keys
    let greater = |a: usize, b: usize| key(a) < key(b);
    match ranked.len() {
        2 | 3 => {
            let pairs: &[(usize, usize)] = if ranked.len() == 2 {
                &[(0, 1)]
            } else {
                &[(0, 1), (0, 2), (1, 2)]
            };
            for &(i, j) in pairs {
                if greater(ranked[i], ranked[j]) {
                    ranked.swap(i, j);
                }
            }
        }
        _ => {
            for i in 1..ranked.len() {
                let mut j = i;
                while j > 0 && key(ranked[j]) > key(ranked[j - 1]) {
                    ranked.swap(j, j - 1);
                    j -= 1;
                }
            }
        }
    }
    let mut active: Seats<usize> = Seats::new();
    let mut eliminated: Seats<usize> = Seats::new();
    let mut nature = None;
    for &k in ranked.iter() {
        if ids[k] == 255 {
            nature = Some(k);
        } else if g.players[k].wiped_at_command != -1 {
            eliminated.push(k);
        } else {
            active.push(k);
        }
    }
    // eliminated players (membership: wiped), latest elimination first by
    // (the resignation index when there is one, else the wipe index); a swap network up to 3 entries
    let wiped = |k: usize| {
        let r = &g.players[k];
        if r.resigned_at_command >= 0 {
            r.resigned_at_command
        } else {
            r.wiped_at_command
        }
    };
    if eliminated.len() > 3 {
        return Err("ranking more than three eliminated players not implemented".into());
    }
    let pairs: &[(usize, usize)] = match eliminated.len() {
        2 => &[(0, 1)],
        3 => &[(0, 1), (0, 2), (1, 2)],
        _ => &[],
    };
    for &(a, b) in pairs {
        if wiped(eliminated[a]) < wiped(eliminated[b]) {
            eliminated.swap(a, b);
        }
    }
    for &k in eliminated.iter().chain(&nature) {
        active.push(k);
    }
    let selected = active[0];
    let mut real = 0;
    let mut alive_count = 0;
    for &p in ids.iter().filter(|&&p| p != 255) {
        let a = alive(g, p)?;
        alive_count += i32::from(a);
        real += i32::from(a && !g.player(p).unwrap().auto_play);
    }
    let (turn_limit, score_limit) = (rules.turn_limit, rules.score_limit);
    #[allow(clippy::if_same_then_else)] // one branch per way a game ends, in the rules' order
    let completed = if real < 2 && (game_type == 1 || game_type == 4) || real == 0 {
        true
    } else if rules.win_by_extermination && alive_count == 1 {
        true
    } else if turn_limit > 0 {
        i64::from(g.current_turn) >= turn_limit
    } else if rules.win_by_capital && capitals(ids[selected]) == g.players.len() as i64 - 1 {
        true
    } else {
        score_limit > 0 && g.players[selected].score >= score_limit
    };
    Ok((ids[selected], completed))
}

/// The index of the first player after `from` in turn order that `keep` takes (the search
/// wraps past the last seat, and then `from` itself comes last), and whether it wrapped; None when `keep` takes nobody.
fn next_index(
    g: &Game,
    from: usize,
    keep: impl Fn(i32) -> Result<bool, String>,
) -> Result<Option<(usize, bool)>, String> {
    let kept: Seats<bool> = g
        .players
        .iter()
        .map(|q| keep(q.action.id))
        .collect::<Result<_, _>>()?;
    if !kept.iter().any(|&k| k) {
        return Ok(None);
    }
    let mut index = from;
    let mut wrapped = false;
    loop {
        index += 1;
        if index >= kept.len() {
            index = 0;
            wrapped = true;
        }
        if kept[index] {
            return Ok(Some((index, wrapped)));
        }
    }
}

/// The seat of the batch's `other_view` output: the
/// next player after the one to move in turn order, as EndTurn passes the turn: alive and not
/// Nature. When every other player is dead (the game then ends at the mover's EndTurn) it is
/// the next of them all the same; None only in a game of one player.
pub fn other_seat(g: &Game) -> Result<Option<i32>, String> {
    let from = g.current_player_index as usize;
    let mover = g.players.get(from).ok_or("no player to move")?.action.id;
    let other = |q: i32| q != mover && q != 255;
    let next = match next_index(g, from, |q| Ok(other(q) && alive(g, q)?))? {
        Some(n) => Some(n),
        None => next_index(g, from, |q| Ok(other(q)))?,
    };
    Ok(next.map(|(i, _)| g.players[i].action.id))
}

/// The EndTurn turn effect
pub fn end_turn(g: &mut Game, p: i32, out: &mut Vec<Action>) -> Result<(), String> {
    let i = player_index(g, p)?;
    if let Some(task) = g.players[i].task_mut(PACIFIST_TASK) {
        if !task.flags.completed {
            task.peaceful_turns = Some(task.peaceful_turns.unwrap_or(0) + 1);
        }
    }
    let current = g.players[g.current_player_index as usize].action.id;
    if p == current {
        let (index, wrapped) = next_index(g, g.current_player_index as usize, |q| alive(g, q))?
            .ok_or("no player alive")?;
        g.current_player_index = index as i32;
        g.current_turn += i32::from(wrapped);
    }
    let (selected, completed) = winner(g)?;
    if completed {
        out.push(Action::GameOver {
            player: p,
            winner: selected,
        });
    }
    let next = g.players[g.current_player_index as usize].action.id;
    out.push(Action::TurnEffect(TurnEffect::StartTurn { player: next }));
    if g.fixed.game_type == PASS_AND_PLAY {
        out.push(Action::TurnEffect(TurnEffect::PassPlayer { player: next }));
    }
    // remove the robbed effect from the ending player's improvements
    for i in 0..g.tiles.len() {
        let x = &g.tiles[i];
        let robbed = x.owner == p
            && x.improvement
                .as_ref()
                .is_some_and(|m| m.effects.contains(&1));
        if robbed {
            let e = &mut g.tile_mut(i).improvement.as_mut().unwrap().effects;
            if let Some(k) = e.iter().position(|&v| v == 1) {
                e.remove(k);
            }
        }
    }
    // the ending player's cool-downs toward every player end
    for &other in g.player_ids().iter() {
        if other == 0 {
            continue;
        }
        let r = relation_entry(g, i, other)?;
        if r.state == COOL_DOWN {
            r.state = NEUTRAL;
        }
    }
    Ok(())
}

/// A relation as stored, or the default record where there is none. The game's lookup
/// would insert that default; a default record counts as none, so
/// income, which the rule queries also ask for, writes nothing.
fn relation(g: &Game, i: usize, other: i32) -> Relation {
    g.players[i]
        .relation(other)
        .copied()
        .unwrap_or(Relation::DEFAULT)
}

/// Whether the players are at war: an attack within the last turn either way, unless at peace
fn has_war(g: &Game, i: usize, o: usize) -> bool {
    let other = g.players[o].action.id;
    if other != 0 && relation(g, i, other).state == PEACE {
        return false;
    }
    let turn = i64::from(g.current_turn);
    let made = |k: usize| k < g.current_player_index as usize;
    let delta = turn - i64::from(relation(g, i, other).last_attack_turn);
    if delta < 1 || delta == 1 && !made(i) {
        return true;
    }
    let own = g.players[i].action.id;
    let delta = turn - i64::from(relation(g, o, own).last_attack_turn);
    delta < 1 || delta == 1 && !made(o)
}

/// Embassy income: the player's embassy in the opponent's capital
fn embassy_income(g: &Game, t: &Tables, i: usize, o: usize) -> Result<i32, String> {
    let r = relation(g, i, g.players[o].action.id);
    if r.embassy_level < 1 || has_war(g, o, i) {
        return Ok(0);
    }
    let base = t.embassy_income.ok_or("embassy income")?;
    let peace = r.state == PEACE;
    Ok((base * if peace { 2 } else { 1 } * i64::from(r.embassy_level)) as i32)
}

/// Work: an owned improvement's or city's income at `level`
pub(crate) fn work(g: &Game, t: &Tables, c: Xy, level: i32) -> Result<i32, String> {
    let i = g.index(c).ok_or("work tile")?;
    let tile = &g.tiles[i];
    let Some(m) = &tile.improvement else {
        return Ok(0);
    };
    let (owner, unit) = (tile.owner, tile.unit);
    let (kind, production, xp, robbed) = (m.kind, m.production, m.xp, m.effects.contains(&1));
    if kind == 0 {
        return Ok(0);
    }
    let Some(d) = t.improvements.get(kind as usize) else {
        return Ok(0);
    };
    if kind != CITY || owner == 0 {
        let w = d.work.unwrap_or(0); // the game catalog defaults a missing integer field to 0
        let w = if w > 0 { (w * level).max(0) } else { w };
        let mut doubled = false;
        if d.improvement_abilities.has("doubled") {
            'n: for x in c.x - 1..=c.x + 1 {
                for y in c.y - 1..=c.y + 1 {
                    let n = Xy { x, y };
                    if n == c {
                        continue;
                    }
                    if let Some(o) = g.tile(n) {
                        if o.improvement.as_ref().is_some_and(|m| m.kind == PORT) {
                            doubled = o.owner == owner;
                            break 'n;
                        }
                    }
                }
            }
        }
        return Ok(w << u32::from(doubled));
    }
    if unit.is_some_and(|n| g.units[n as usize].owner != owner) {
        return Ok(0);
    }
    if robbed {
        return Ok(0);
    }
    let Ok(pi) = player_index(g, owner) else {
        return Ok(0);
    };
    let mut w = level + production - 1;
    if g.players[pi].start_tile == c {
        w += g.players[pi].handicap;
        for o in 0..g.players.len() {
            w += embassy_income(g, t, pi, o)?;
        }
    }
    Ok((w + xp.min(0)).max(0))
}

/// Player work: income from a tile for `p`, own or an embassy's capital
pub(crate) fn player_work(g: &Game, t: &Tables, c: Xy, p: i32, level: i32) -> Result<i32, String> {
    let i = g.index(c).ok_or("player work tile")?;
    let pi = player_index(g, p)?;
    let tile = &g.tiles[i];
    let (owner, capital_of, unit) = (tile.owner, tile.capital_of, tile.unit);
    if owner == p {
        return work(g, t, c, level);
    }
    if owner != 0 && capital_of == owner {
        let opponent = unit.is_some_and(|n| {
            let o = g.units[n as usize].owner;
            o != 255 && o != owner
        });
        if !opponent {
            let oi = player_index(g, owner)?;
            return embassy_income(g, t, oi, pi);
        }
    }
    Ok(0)
}

/// The StartTurn turn effect
pub fn start_turn(g: &mut Game, t: &Tables, p: i32, out: &mut Vec<Action>) -> Result<(), String> {
    let index = player_index(g, p)?;
    for other in 0..g.players.len() {
        if relation_entry(g, other, p)?.embassy_level >= 1 && !has_war(g, index, other) {
            relation_entry(g, other, p)?.embassy_level = 1;
        }
    }
    let current = g.players[g.current_player_index as usize].action.id;
    for n in 0..g.tiles.len() {
        // read before the calls below, which write to the game
        let tile = &g.tiles[n];
        let (c, owner) = (tile.coordinates, tile.owner);
        let built = tile
            .improvement
            .as_ref()
            .map(|m| (m.kind, m.level, m.xp, m.founded));
        if owner == p {
            if let Some((kind, level, xp, founded)) = built {
                if g.current_turn != 0 && kind != 0 && t.improvements.get(kind as usize).is_some() {
                    if improvement_has(t, kind, "attract") {
                        crate::elyrion::attract(g, t, p, c, kind, founded, out)?;
                    }
                    if level < xp && p == current {
                        out.push(Action::City(City::LevelUp {
                            player: p,
                            target: c,
                        }));
                    }
                    let level = crate::economy::update_level(g, t, p, c, out)?;
                    let amount = work(g, t, c, level)?;
                    if amount > 0 {
                        out.push(Action::Payload(Payload::IncreaseCurrency {
                            player: p,
                            delay: 10,
                            source: c,
                            amount,
                        }));
                    }
                }
            }
            if let Some(a) = crate::polaris::restore_climate(g, t, index, c)? {
                out.push(a);
            }
        } else {
            let level = built.map_or(0, |m| m.1);
            let amount = player_work(g, t, c, p, level)?;
            if amount > 0 {
                out.push(Action::Payload(Payload::IncreaseCurrency {
                    player: p,
                    delay: 10,
                    source: c,
                    amount,
                }));
            }
        }
    }
    for task in [PACIFIST_TASK, EXPLORER_TASK] {
        out.extend(task_notification(g, t, index, task)?);
    }
    tribe_connection(g, index, out)?;
    diplomacy(g, index, out)?;
    refresh_tech_cache(g, t, p)?;
    if crate::menu::player_ability(g, t, p, "capitalvision") {
        for o in 0..g.players.len() {
            let other = g.players[o].action.id;
            let known = other == p || g.players[index].known_players.contains(&other);
            if other == 255 || !known {
                continue;
            }
            let start = g.players[o].start_tile;
            if !g.tile(start).ok_or("capital tile")?.explorers.contains(&p) {
                out.push(Action::Explore {
                    player: p,
                    coordinates: start,
                });
            }
        }
    }
    Ok(())
}

fn knows(g: &Game, i: usize, other: i32) -> bool {
    other == 255 || g.players[i].action.id == other || g.players[i].known_players.contains(&other)
}

/// Tribe connection: Meet any unknown player whose unit or city the player can see
fn tribe_connection(g: &mut Game, i: usize, out: &mut Vec<Action>) -> Result<(), String> {
    let p = g.players[i].action.id;
    let others = g.players.iter().filter(|q| q.action.id != 255).count();
    if p == 255 || g.players[i].known_players.len() >= others {
        return Ok(());
    }
    for n in 0..g.tiles.len() {
        let tile = &g.tiles[n];
        if !tile.explorers.contains(&p) {
            continue;
        }
        // read before `relation_of` below, which writes to the game
        let (c, tile_owner, tile_unit) = (tile.coordinates, tile.owner, tile.unit);
        let city = tile.improvement.as_ref().is_some_and(|m| m.kind == CITY);
        let mut unit = None;
        if let Some(u) = tile_unit {
            let (owner, effects) = (g.units[u as usize].owner, g.units[u as usize].effects);
            let hidden = crate::menu::unit_effect_list(g, effects).contains(&3)
                && owner != p
                && relation_of(g, i, owner)?.state != PEACE;
            if !hidden {
                unit = Some(owner);
            }
        }
        let unit_unknown = unit.is_some_and(|o| o != p && !knows(g, i, o));
        let city_unknown = city && tile_owner != 0 && tile_owner != p && !knows(g, i, tile_owner);
        if unit_unknown {
            out.push(Action::Meet {
                player: p,
                other: unit.unwrap(),
                coordinates: c,
                technology: 0,
            });
        }
        if city_unknown && (!unit_unknown || unit != Some(tile_owner)) {
            out.push(Action::Meet {
                player: p,
                other: tile_owner,
                coordinates: c,
                technology: 0,
            });
        }
    }
    Ok(())
}

/// Diplomacy check: deliver pending messages (newest first) and end expired cool-downs
fn diplomacy(g: &mut Game, i: usize, out: &mut Vec<Action>) -> Result<(), String> {
    let p = g.players[i].action.id;
    let count = g.players[i].messages.len();
    for n in (0..count).rev() {
        let m = g.players[i].messages[n];
        let sender = m.sender;
        if g.player(sender).is_none() {
            return Err("message from a missing player".into());
        }
        if alive(g, sender)? {
            out.push(Action::TurnEffect(TurnEffect::ReceiveDiplomacyMessage {
                player: p,
                sender,
                kind: m.kind,
            }));
            if !knows(g, i, sender) {
                out.push(Action::Meet {
                    player: p,
                    other: sender,
                    coordinates: g.players[i].start_tile,
                    technology: 0,
                });
            }
        } else {
            let list = &mut g.players[i].messages;
            if let Some(k) = list.iter().position(|x| *x == m) {
                list.remove(k);
            }
        }
    }
    let turn = i64::from(g.current_turn);
    for &opponent in g.player_ids().iter() {
        if opponent == 0 {
            continue;
        }
        let r = relation_of(g, i, opponent)?;
        let broken = i64::from(r.last_peace_broken_turn);
        if r.state == COOL_DOWN
            && (opponent < p && broken < turn || p < opponent && broken + 1 < turn)
        {
            relation_entry(g, i, opponent)?.state = NEUTRAL;
        }
    }
    Ok(())
}

/// Receive a diplomacy message; it queues nothing, and does nothing when the message is gone
pub fn receive_message(g: &mut Game, player: i32, sender: i32, kind: i64) -> Result<(), String> {
    let i = player_index(g, player)?;
    let list = &mut g.players[i].messages;
    let Some(k) = list
        .iter()
        .position(|m| sender == m.sender && kind == m.kind)
    else {
        return Ok(());
    };
    list.remove(k);
    match kind {
        PEACE_REQUEST_MESSAGE => {
            g.triggers.push(crate::state::Trigger {
                player,
                opponent: sender,
                kind: PEACE_REQUEST_TRIGGER,
                coordinates: Xy { x: 0, y: 0 },
            });
        }
        EMBASSY_MESSAGE => {}
        _ => return Err("unsupported diplomacy message".into()),
    }
    Ok(())
}

/// GameOver: valid unless the match is already in its final turn or ended; moves it to the final turn
pub fn game_over(g: &mut Game) {
    if g.current_state == FINAL_TURN || g.current_state == ENDED {
        return;
    }
    g.current_state = FINAL_TURN;
}

pub fn end_match(g: &mut Game, p: i32, out: &mut Vec<Action>) {
    crate::step::base(out, p, 22);
    g.current_state = ENDED;
    out.push(Action::TurnEffect(TurnEffect::EndMatch { player: 0 }));
}
