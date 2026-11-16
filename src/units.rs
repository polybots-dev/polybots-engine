//! Unit creation and transport: training a unit (with the vehicle's Warrior passenger), the Train
//! economy action, Embark and Disembark.

use crate::action::{Action, Combat, Economy, MoveEffect, Payload, TurnEffect};
use crate::state::{Game, Unit, Xy};
use crate::step::player_index;
use crate::tables::Tables;

const CITY: i32 = 1;
const WARRIOR: i32 = 2;
const POLYTAUR: i32 = 15;
const GIANT: i32 = 12;
const DAGGER: i32 = 37;
const CLOAK_BOAT: i32 = 39;
const PIRATE: i32 = 40;
const TRANSPORTSHIP: i32 = 43;
const JUGGERNAUT: i32 = 45;
const NULL: Xy = Xy { x: -1, y: -1 };

fn has(t: &Tables, kind: i32, a: &str) -> Result<bool, String> {
    Ok(t.units
        .get(kind as usize)
        .ok_or("unknown unit kind")?
        .unit_abilities
        .has(a))
}

/// Score of a unit: five per star of the unit and its direct passenger
pub(crate) fn unit_score(g: &Game, t: &Tables, u: &Unit) -> i32 {
    let cost = |k: i32| t.units.get(k as usize).map_or(0, |d| d.cost);
    let mut total = cost(u.kind);
    if let Some(p) = u.passenger.and_then(|n| g.units.get(n as usize)) {
        total = total.wrapping_add(cost(p.kind));
    }
    total.wrapping_mul(5)
}

/// A new unit's facing: the direction toward the map centre (weighted, first best)
fn facing(g: &Game, c: Xy) -> i32 {
    let (w, h) = (g.width, g.tiles.len() as i32 / g.width);
    let (dx, dy) = (w / 2 - c.x, h / 2 - c.y);
    let vectors = [
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
        (1, 0),
        (1, -1),
        (0, -1),
    ];
    let (mut best, mut score) = (0, 0);
    for (i, &(x, y)) in vectors.iter().enumerate() {
        let weight = if x * x + y * y == 1 { 1393 } else { 985 };
        let next = (x * dx + y * dy) * weight;
        if score < next {
            best = i;
            score = next;
        }
    }
    best as i32
}

/// A new unit node (and passenger for a vehicle) placed on `c`. The
/// creation queues the Poison of a poison improvement under it; a Tentacles unit refreshes the
/// tentacle zones
pub(crate) fn create_unit(
    g: &mut Game,
    t: &Tables,
    player: i32,
    kind: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<u32, String> {
    let i = g.index(c).ok_or("train tile")?;
    let tile = &g.tiles[i];
    let home = if tile.improvement.as_ref().is_some_and(|m| m.kind == CITY)
        && !has(t, kind, "independent")?
    {
        c
    } else {
        NULL
    };
    let (climate, skin) = if kind == POLYTAUR {
        (tile.climate, tile.skin)
    } else {
        (0, 0)
    };
    let data = t.units.get(kind as usize).ok_or("unknown unit kind")?;
    g.current_unit_id = g.current_unit_id.wrapping_add(1);
    let effects = {
        g.effect_lists.push(Default::default());
        (g.effect_lists.len() - 1) as u32
    };
    let unit = Unit {
        id: g.current_unit_id,
        leader: 0,
        follower: 0,
        owner: player,
        kind,
        previous_turn_end_coordinates: Xy { x: 0, y: 0 },
        coordinates: c,
        home,
        passenger: None,
        health: data.health,
        promotion_level: 0,
        xp: 0,
        moved: true,
        attacked: true,
        direction: facing(g, c),
        flipped: false,
        created_turn: g.current_turn,
        effects,
        is_temporary_explorer: false,
        birth_climate: climate,
        birth_climate_skin: skin,
    };
    let node = g.units.len() as u32;
    g.units.push(unit);
    if has(t, kind, "carry")? {
        let passenger = create_unit(g, t, player, WARRIOR, c, out)?;
        let health = g.units[passenger as usize].health;
        let protect = has(t, kind, "protect")?;
        let v = &mut g.units[node as usize];
        v.passenger = Some(passenger);
        if !protect {
            v.health = health;
        }
    }
    g.tiles[i].unit = Some(node);
    // The step-on-poison check for the new unit: a poison improvement poisons it, unless its
    // trainer's tribe resists or it flies (the trainer's tribe, as the game reads it)
    if let Some(m) = &g.tiles[i].improvement {
        let toxic = t
            .improvements
            .get(m.kind as usize)
            .is_some_and(|d| d.improvement_abilities.has("poison"));
        let resists = g
            .player(player)
            .and_then(|p| t.tribes.get(p.tribe as usize))
            .is_some_and(|d| d.tribe_abilities.has("poisonresist"));
        if toxic && !resists && !has(t, kind, "fly")? {
            out.push(Action::Combat(Combat::Poison {
                player: g.tiles[i].owner,
                origin: c,
                target: c,
            }));
        }
    }
    if has(t, kind, "tentacles")? {
        crate::aquarion::update_tentacles(g, t)?;
    }
    Ok(node)
}

/// Train: pay, create, explore around, surrounding-area levels, score
#[allow(clippy::too_many_arguments)] // the rule's inputs, passed as the step holds them
pub fn train(
    g: &mut Game,
    t: &Tables,
    player: i32,
    kind: i32,
    c: Xy,
    cost: i32,
    facing_target: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let Some(i) = g.index(c) else {
        return Ok(());
    };
    if t.units.get(kind as usize).is_none() {
        return Ok(());
    }
    let Ok(p) = player_index(g, player) else {
        return Ok(());
    };
    if g.tiles[i].unit.is_some() {
        let cur = &mut g.players[p].action.currency;
        *cur = cur.wrapping_sub(cost).max(0);
        return train_on_occupied(g, t, player, kind, c, out);
    }
    let sight = 1 + i32::from(has(t, kind, "scout")?);
    let cur = &mut g.players[p].action.currency;
    *cur = cur.wrapping_sub(cost).max(0);
    let node = create_unit(g, t, player, kind, c, out)?;
    if facing_target != NULL {
        let d = crate::step::facing_direction(c, facing_target);
        g.units[node as usize].direction = d;
        g.units[node as usize].flipped = d < 4;
    }
    let amount = unit_score(g, t, &g.units[node as usize]);
    if has(t, kind, "tentacles")? {
        crate::attack::area_damage(g, t, node, c, out)?;
        crate::aquarion::update_tentacles(g, t)?;
    }
    crate::explore::explore_from(g, player, c, sight, out)?;
    crate::economy::surrounding(g, t, player, c, out)?;
    out.push(Action::Payload(Payload::IncreaseScore {
        player,
        amount,
        source: c,
        delay: 0,
    }));
    Ok(())
}

/// Embark: the land unit becomes the passenger of a new vehicle on its tile
pub fn embark(
    g: &mut Game,
    t: &Tables,
    player: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    if player_index(g, player).is_err() {
        return Ok(());
    }
    let i = g.index(c).ok_or("embark tile")?;
    let old = g.tiles[i].unit.ok_or("embark with no unit")?;
    let kind = g.units[old as usize].kind;
    let vehicle = match kind {
        GIANT => JUGGERNAUT,
        DAGGER => PIRATE,
        _ if has(t, kind, "hide")? => CLOAK_BOAT,
        _ => TRANSPORTSHIP,
    };
    let new = create_unit(g, t, player, vehicle, c, out)?;
    let protect = has(t, vehicle, "protect")?;
    let p = g.units[old as usize];
    let v = &mut g.units[new as usize];
    if !protect {
        v.health = p.health;
    }
    v.home = p.home;
    v.direction = p.direction;
    v.flipped = p.flipped;
    v.passenger = Some(old);
    v.effects = p.effects;
    v.moved = true;
    v.attacked = true;
    if has(t, vehicle, "stomp")? {
        crate::attack::area_damage(g, t, new, c, out)?;
    }
    Ok(())
}

/// Disembark: the passenger takes the vehicle's tile, health and facing
pub fn disembark(
    g: &mut Game,
    t: &Tables,
    player: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let i = g.index(c).ok_or("disembark tile")?;
    let Some(vehicle) = g.tiles[i].unit else {
        return Ok(());
    };
    let v = g.units[vehicle as usize];
    let Some(passenger) = v.passenger else {
        return Ok(());
    };
    let protect = has(t, v.kind, "protect")?;
    let p = &mut g.units[passenger as usize];
    if !protect {
        p.health = v.health;
    }
    p.direction = v.direction;
    p.flipped = v.flipped;
    p.moved = true;
    p.attacked = true;
    p.coordinates = c;
    g.tiles[i].unit = Some(passenger);
    let kind = g.units[passenger as usize].kind;
    if has(t, kind, "grow")? {
        // a Grow passenger older than two turns comes ashore grown, if its tribe can train the
        // next stage
        crate::step::refresh_tech_cache(g, t, player)?;
        let age = g
            .current_turn
            .wrapping_sub(g.units[passenger as usize].created_turn) as u32;
        let mut next = None;
        upgradable(g, t, player, kind, |id| {
            next = Some(id);
            false
        })?;
        if let Some(next) = next {
            if age > 2 {
                out.push(Action::TurnEffect(TurnEffect::Upgrade {
                    player,
                    coordinates: c,
                    unit: next as i32,
                    cost: 0,
                }));
            }
        }
    }
    let difference =
        unit_score(g, t, &v).wrapping_sub(unit_score(g, t, &g.units[passenger as usize]));
    out.push(Action::Combat(Combat::DecreaseScore {
        player,
        amount: difference,
    }));
    Ok(())
}

/// The upgradable units: the units
/// `player`'s techs unlock, through its tribe, whose upgradesFrom is `base`, in tech order.
/// Hidden units count; a unique one only while the player owns none. Each is passed to `visit`,
/// which returns false to stop (a visitor rather than a list keeps the legal list allocation-free).
pub(crate) fn upgradable(
    g: &Game,
    t: &Tables,
    player: i32,
    base: i32,
    mut visit: impl FnMut(usize) -> bool,
) -> Result<(), String> {
    let p = g.player(player).ok_or("missing player")?;
    let Ok(name) = t.units.name(base as usize) else {
        return Ok(());
    };
    let tree = t.tribe_tree(p.tribe)?;
    for &tech in &p.available_tech {
        if t.techs.get(tech as usize).is_none() {
            continue;
        }
        for &id in tree.units_of(tech)? {
            let d = t.units.get(id).unwrap();
            let owned = || {
                g.tiles
                    .iter()
                    .filter_map(|x| x.unit)
                    .filter_map(|n| g.units.get(n as usize))
                    .any(|x| x.owner == player && x.kind == id as i32)
            };
            if d.unit_abilities.has("unique") && owned() {
                continue;
            }
            if d.upgrades_from.as_deref() == Some(name) && !visit(id) {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Upgrade: a new unit of `kind` replaces the tile's unit, keeping its identity and flags
pub fn upgrade(
    g: &mut Game,
    t: &Tables,
    player: i32,
    c: Xy,
    kind: i32,
    cost: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let Some(i) = g.index(c) else {
        return Ok(());
    };
    if t.units.get(kind as usize).is_none() {
        return Ok(());
    }
    let Ok(p) = player_index(g, player) else {
        return Ok(());
    };
    let original = g.tiles[i].unit.ok_or("upgrade with no unit")?;
    let cur = &mut g.players[p].action.currency;
    *cur = cur.wrapping_sub(cost).max(0);
    let new = create_unit(g, t, player, kind, c, out)?;
    let old = g.units[original as usize];
    let n = &mut g.units[new as usize];
    if old.passenger.is_some() {
        n.passenger = old.passenger;
    }
    n.home = old.home;
    n.moved = old.moved;
    n.attacked = old.attacked;
    n.flipped = old.flipped;
    n.direction = old.direction;
    n.follower = old.follower;
    n.id = old.id;
    n.effects = old.effects;
    let new_health = n.health;
    let health = if has(t, kind, "carry")? {
        old.health
    } else {
        let missing = crate::combat::max_health(g, t, &old)? - old.health;
        new_health - missing.min(new_health - 1)
    };
    g.units[new as usize].health = health;
    let score = unit_score(g, t, &g.units[new as usize]).wrapping_sub(unit_score(g, t, &old));
    out.push(Action::Payload(Payload::IncreaseScore {
        player,
        amount: score,
        delay: 0,
        source: c,
    }));
    let sight = 1 + i32::from(has(t, kind, "scout")?);
    crate::explore::explore_from(g, player, c, sight, out)?;
    Ok(())
}

/// Exhaust a unit: no more moves or attacks, DoubleReady and Boosted removed; every
/// follower in the chain is spent too (flags only)
pub(crate) fn make_exhausted(g: &mut Game, node: u32) -> Result<(), String> {
    g.units[node as usize].moved = true;
    g.units[node as usize].attacked = true;
    let list = g.units[node as usize].effects as usize;
    let e = g.effect_lists.get_mut(list).ok_or("effect list")?;
    for x in [7, 2] {
        if let Some(k) = e.iter().position(|v| *v == x) {
            e.remove(k);
        }
    }
    for f in crate::linked::followers(g, node)? {
        g.units[f as usize].moved = true;
        g.units[f as usize].attacked = true;
    }
    Ok(())
}

/// Promotion: the unit (its passenger if carried) gains a level and is healed to its new maximum
pub fn promote(g: &mut Game, t: &Tables, c: Xy) -> Result<(), String> {
    let n = g
        .tile(c)
        .and_then(|t| t.unit)
        .ok_or("promotion with no unit")?;
    let selected = g.units[n as usize].passenger.unwrap_or(n);
    g.units[selected as usize].promotion_level += 1;
    let max = crate::combat::max_health(g, t, &g.units[n as usize].clone())?;
    g.units[n as usize].health = i32::from(max as u16);
    Ok(())
}

/// The average of two directions: opposite directions give the first turned a quarter; any
/// other sum the direction with the strictly greatest weighted dot product (the first on a tie)
fn average(first: i32, second: i32) -> i32 {
    let v = |d: i32| VECTORS.get(d as usize).copied().unwrap_or((0, 0));
    let (a, b) = (v(first), v(second));
    let (x, y) = (a.0 + b.0, a.1 + b.1);
    if x == 0 && y == 0 {
        return (first + 2) % 8;
    }
    let (mut best, mut score) = (0, 0);
    for (i, &(dx, dy)) in VECTORS.iter().enumerate() {
        let weight = if dx * dx + dy * dy == 1 { 1393 } else { 985 };
        let dot = (dx * x + dy * y) * weight;
        if score < dot {
            score = dot;
            best = i as i32;
        }
    }
    best
}

const VECTORS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
];

/// Push: the occupant steps aside (one tile, its own movement rules) if it can
fn try_push(g: &mut Game, t: &Tables, actor: i32, c: Xy) -> Result<Option<Action>, String> {
    let i = g.index(c).ok_or("push tile")?;
    let n = g.tiles[i].unit.ok_or("push with no unit")?;
    let unit = g.units[n as usize];
    let mut direction = if unit.owner == actor {
        unit.direction
    } else if unit.direction < 4 {
        unit.direction + 4
    } else {
        unit.direction - 4
    };
    // a chain member is pushed along the chain: toward its leader, its follower, or between them
    let toward = |id: u32| {
        crate::linked::find(g, id).map_or(0, |n| {
            crate::step::facing_direction(c, g.units[n as usize].coordinates)
        })
    };
    direction = match (unit.leader != 0, unit.follower != 0) {
        (true, true) => average(toward(unit.leader), toward(unit.follower)),
        (true, false) => toward(unit.leader),
        (false, true) => toward(unit.follower),
        (false, false) => direction,
    };
    let vehicle = has(t, unit.kind, "carry")?;
    let source_water = [1, 2].contains(&g.tiles[i].terrain);
    let mut fallback = None;
    let push = |path: crate::action::Path| {
        Action::MoveEffect(MoveEffect::Move {
            player: unit.owner,
            unit: unit.id,
            path,
            should_animate: true,
            reason: 2,
        })
    };
    for offset in [0, -1, 1, -2, 2, -3, 3, -4] {
        let d = ((direction + offset + 8) % 8) as usize;
        let target = Xy {
            x: c.x + VECTORS[d].0,
            y: c.y + VECTORS[d].1,
        };
        let Some(next) = g.tile(target) else { continue };
        if next.unit.is_some() {
            continue;
        }
        if source_water && vehicle && ![1, 2].contains(&next.terrain) && fallback.is_none() {
            fallback = Some(target);
            continue;
        }
        crate::step::refresh_tech_cache(g, t, unit.owner)?;
        let path = crate::movement::unit_path_budget(g, t, &unit, target, 1)?;
        if let Some(path) = path {
            return Ok(Some(push(path)));
        }
    }
    if let Some(target) = fallback {
        crate::step::refresh_tech_cache(g, t, unit.owner)?;
        let path = crate::movement::unit_path_budget(g, t, &unit, target, 1)?;
        if let Some(path) = path {
            return Ok(Some(push(path)));
        }
    }
    Ok(None)
}

/// Queue a free Train, pushing (or killing) the occupant first
pub(crate) fn train_on_occupied(
    g: &mut Game,
    t: &Tables,
    player: i32,
    kind: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let pi = player_index(g, player)?;
    // the game resolves the kind through the player's tribe again
    let kind = t.tribe_unit(g.players[pi].tribe, kind)?;
    out.push(Action::Economy(Economy::Train {
        player,
        kind,
        target: c,
        cost: 0,
        direction_target: NULL,
        train_action_aborted: false,
    }));
    let Some(occupant) = g.tile(c).ok_or("train tile")?.unit else {
        return Ok(());
    };
    match try_push(g, t, player, c)? {
        Some(push) => out.push(push),
        None => {
            out.push(Action::Combat(Combat::Kill {
                player,
                coordinates: c,
            }));
            // rules 124 adds a Houdini action after the Kill; rules 123 has none
            if g.fixed.version == 124 {
                out.push(Action::Economy(Economy::Houdini { player, target: c }));
            }
        }
    }
    let u = g.units[occupant as usize];
    if crate::menu::unit_effect_list(g, u.effects).contains(&3) && u.owner != player {
        let peace = u.owner != 0 && {
            crate::step::relation_entry(g, pi, u.owner)?.state == crate::state::PEACE
        };
        if !peace {
            out.push(Action::MoveEffect(MoveEffect::Reveal {
                player,
                coordinates: c,
                origin: NULL,
                show_popup: false,
            }));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));

    /// A free Train onto player 2's starting unit, which has nowhere to go (every neighbour
    /// is taken): the occupant is killed. Rules 124 then queues a Houdini action (an
    /// empty Execute); rules 123 has no Houdini, and this engine refuses
    /// one at 123. No replay can show this, since the action does nothing.
    #[test]
    fn houdini_follows_a_failed_push_only_at_rules124() {
        let t = Tables::compile(CATALOG).unwrap();
        for rules in [123, 124] {
            let config = crate::reset::Config {
                rules,
                seed: 3,
                size: 11,
                map: 2,
                mode: 4,
                tribes: vec![7, 7],
                action_limit: 4096,
            };
            let mut g = crate::reset::reset(&config, CATALOG, &t).unwrap();
            let (at, occupant) = g
                .tiles
                .iter()
                .find_map(|tile| {
                    let n = tile.unit?;
                    (g.units[n as usize].owner == 2).then_some((tile.coordinates, n))
                })
                .unwrap();
            for (dx, dy) in VECTORS {
                if let Some(i) = g.index(Xy {
                    x: at.x + dx,
                    y: at.y + dy,
                }) {
                    g.tiles[i].unit = Some(occupant);
                }
            }
            let mut out = Vec::new();
            train_on_occupied(&mut g, &t, 1, WARRIOR, at, &mut out).unwrap();
            let houdini = out
                .iter()
                .any(|a| matches!(a, Action::Economy(Economy::Houdini { .. })));
            assert!(
                out.iter()
                    .any(|a| matches!(a, Action::Combat(Combat::Kill { .. }))),
                "rules {rules}: {out:?}"
            );
            assert_eq!(houdini, rules == 124, "rules {rules}: {out:?}");
        }
    }
}
