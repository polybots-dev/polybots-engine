//! Linked units (a centipede's head and its segments): finding a unit by its id.

use crate::state::Game;

/// The node of the unit with id `id` that stands on the board
pub(crate) fn find(g: &Game, id: u32) -> Option<u32> {
    g.tiles
        .iter()
        .filter_map(|x| x.unit)
        .find(|&n| g.units[n as usize].id == id)
}

/// The followers of `node`'s chain, head first excluded, in chain order (a cycle is
/// an error)
pub(crate) fn followers(g: &Game, node: u32) -> Result<Vec<u32>, String> {
    let mut out = Vec::new();
    let mut current = node;
    loop {
        let f = g.units[current as usize].follower;
        if f == 0 {
            break;
        }
        let Some(next) = find(g, f) else { break };
        if next == node || out.contains(&next) {
            return Err("cyclic follower chain".into());
        }
        out.push(next);
        current = next;
    }
    Ok(out)
}

use crate::action::{Action, Combat, Payload};
use crate::state::Xy;
use crate::tables::Tables;

const FAN: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
];

/// After a kill by an Eat unit (not of Nature), the chain's tail eats: Combat Eat for it
pub(crate) fn eat_after_kill(
    g: &Game,
    p: i32,
    head: u32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let tail = followers(g, head)?.last().copied().unwrap_or(head);
    out.push(Action::Combat(Combat::Eat {
        player: p,
        unit: g.units[tail as usize].id,
        coordinates: Xy { x: 0, y: 0 },
    }));
    Ok(())
}

/// Eat: a Segment is trained behind the eater (the first free tile a Segment led by it
/// could stand on, fanning out from the opposite of its facing), linked to it, explores, takes
/// the surrounding-area levels and adds its score
pub(crate) fn eat(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    eater: u32,
    spawn: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let Some(n) = find(g, eater) else {
        return Ok(());
    };
    let u = g.units[n as usize];
    let opposite = if u.direction < 4 {
        u.direction + 4
    } else {
        u.direction - 4
    };
    let segment = t.units.id("segment")? as i32;
    let mut selected = None;
    for i in 1..=8 {
        let offset = if (i - 1) & 1 == 0 { i / 2 } else { -(i >> 1) };
        let (dx, dy) = FAN[((offset + opposite + 8) % 8) as usize];
        let target = Xy {
            x: u.coordinates.x.wrapping_add(dx),
            y: u.coordinates.y.wrapping_add(dy),
        };
        let Some(tile) = g.tile(target) else { continue };
        if tile.unit.is_some() {
            continue;
        }
        let mut temporary = crate::infiltrate::profile_unit(t, actor, segment, spawn)?;
        temporary.leader = eater;
        temporary.created_turn = g.current_turn;
        if crate::movement::placement_accessible(g, t, &temporary, target)? {
            selected = Some(target);
            break;
        }
    }
    let Some(target) = selected else {
        return Ok(());
    };
    if g.player(actor).is_none() {
        return Ok(());
    }
    let s = crate::units::create_unit(g, t, actor, segment, target, out)?;
    g.unit_mut(s as usize).leader = eater;
    let new_id = g.units[s as usize].id;
    g.unit_mut(n as usize).follower = new_id;
    let scout = t
        .units
        .get(segment as usize)
        .is_some_and(|d| d.unit_abilities.has("scout"));
    crate::explore::explore_from(g, actor, target, 1 + i32::from(scout), out)?;
    crate::economy::surrounding(g, t, actor, target, out)?;
    let score = crate::units::unit_score(g, t, &g.units[s as usize]);
    out.push(Action::Payload(Payload::IncreaseScore {
        player: actor,
        amount: score,
        source: target,
        delay: 0,
    }));
    Ok(())
}
