//! City level-up rewards: the CityReward command and economy action, ExpandCity and the
//! Explorer reward's scout walk.

use crate::action::{Action, City, Economy, Payload};
use crate::random::hash_words;
use crate::state::{Game, Xy};
use crate::step::{player_index, relation_entry};
use crate::tables::Tables;

const CITY: i32 = 1;
const MOUNTAIN: i32 = 4;
const BRIDGE: i32 = 48;
const LIGHTHOUSE: i32 = 47;
const GIANT: i32 = 12;
const POLARIS: i32 = 16;
const NULL: Xy = Xy { x: -1, y: -1 };
const CITY_LEVEL_UP: i64 = 1;

/// The CityReward command's children: the level-up trigger is consumed first.
pub fn reward_command(
    g: &mut Game,
    p: i32,
    reward: i32,
    target: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let n = g
        .triggers
        .iter()
        .rposition(|x| x.player == p && x.kind == CITY_LEVEL_UP)
        .ok_or("city reward without its trigger")?;
    g.triggers.remove(n);
    crate::step::base(out, p, 11);
    out.push(Action::Economy(Economy::CityReward {
        player: p,
        reward,
        target,
    }));
    Ok(())
}

/// The CityReward economy action: record the reward and queue its effects.
pub fn city_reward(
    g: &mut Game,
    t: &Tables,
    p: i32,
    reward: i64,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let Some(i) = g.index(c) else {
        return Ok(());
    };
    if g.tiles[i].improvement.is_none() || player_index(g, p).is_err() {
        return Ok(());
    }
    g.tiles[i]
        .improvement
        .as_mut()
        .unwrap()
        .rewards
        .try_push(i32::try_from(reward).map_err(|_| "reward out of range")?)?;
    if reward == 3 || reward == 2 {
        out.push(Action::Economy(Economy::ModifyProduction {
            player: p,
            amount: 1,
            target: c,
        }));
    }
    match reward {
        2 => {
            out.push(Action::Economy(Economy::ModifyScore {
                player: p,
                amount: 250,
                target: c,
            }));
            out.push(Action::Payload(Payload::IncreaseScore {
                player: p,
                amount: 250,
                source: c,
                delay: 50,
            }));
        }
        4 => {
            g.current_unit_id = g.current_unit_id.wrapping_add(1);
            let seed = crate::step::random_hash(g)?.hash(&[c.x, c.y]);
            out.push(Action::Economy(Economy::Scout {
                player: p,
                unit: g.current_unit_id,
                remaining: 12,
                seed,
                from: c,
                path: Default::default(),
                behaviour: 0,
            }));
        }
        5 => {
            // construct border growth: widen the border now, then expand, route, connect
            let m = g.tiles[i].improvement.as_mut().unwrap();
            m.border_size += 1;
            let others: crate::inline::Seats<i32> = g
                .players
                .iter()
                .map(|q| q.action.id)
                .filter(|&q| q != 255 && q != p)
                .collect();
            let pi = player_index(g, p)?;
            // pushed in reverse of the game's buffer, which ends with Polaris's freeze
            if g.players[pi].tribe == POLARIS {
                let radius = g.tiles[i].improvement.as_ref().unwrap().border_size;
                out.push(Action::Economy(Economy::FreezeArea {
                    player: p,
                    target: c,
                    radius,
                    freeze_units: false,
                    only_owned_tiles: true,
                }));
            }
            out.push(Action::UpdateCityConnections {
                actor: p,
                players: others,
            });
            out.push(Action::UpdateRoutes { player: p });
            out.push(Action::Economy(Economy::ExpandCity {
                player: p,
                target: c,
            }));
        }
        6 => crate::units::train_on_occupied(g, t, p, GIANT, c, out)?,
        7 => {
            for delay in [40, 40, 40, 40, 190] {
                out.push(Action::Payload(Payload::IncreaseCurrency {
                    player: p,
                    source: c,
                    delay,
                    amount: 1,
                }));
            }
        }
        8 => {
            for _ in 0..3 {
                out.push(Action::City(City::IncreasePopulation {
                    player: p,
                    source: c,
                    target: c,
                    delay: 0,
                }));
            }
        }
        1 | 3 => {}
        r => return Err(format!("city reward {r} not implemented")),
    }
    Ok(())
}

/// ModifyProduction / ModifyScore on the target improvement
pub fn modify(g: &mut Game, inner: &str, target: Xy, amount: i32) -> Result<(), String> {
    let i = g.index(target).ok_or("modify tile")?;
    let m = g.tiles[i]
        .improvement
        .as_mut()
        .ok_or("modify without an improvement")?;
    if inner == "ModifyProduction" {
        m.production = i32::from((m.production as u16).wrapping_add(amount as u16));
    } else {
        m.base_score = i32::from((m.base_score as u16).wrapping_add(amount as u16));
    }
    Ok(())
}

/// Expand a city: claim the widened border, then climate, levels and exploration
pub fn expand_city(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    city: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let center = g.index(city).ok_or("expand tile")?;
    let radius = g.tiles[center]
        .improvement
        .as_ref()
        .ok_or("expand without a city")?
        .border_size;
    let pi = player_index(g, actor)?;
    let h = g.tiles.len() as i32 / g.width;
    let mut area = Vec::new();
    for xx in (city.x - radius).max(0)..=(city.x + radius).min(g.width - 1) {
        for yy in (city.y - radius).max(0)..=(city.y + radius).min(h - 1) {
            let i = g.index(Xy { x: xx, y: yy }).unwrap();
            let r = g.tiles[i].ruling_city;
            if r == NULL || r == city {
                area.push(i);
            }
        }
    }
    let mut claimed = Vec::new();
    for i in area {
        if g.tiles[i].owner == actor {
            continue;
        }
        let mut amount = 20;
        if g.tiles[i].improvement.is_some() {
            amount += crate::economy::improvement_score(t, g, i);
        }
        out.push(Action::Payload(Payload::IncreaseScore {
            player: actor,
            amount,
            source: g.tiles[i].coordinates,
            delay: 100,
        }));
        g.tiles[i].owner = actor;
        g.tiles[i].ruling_city = city;
        claimed.push(i);
    }
    for i in claimed {
        let tile = &g.tiles[i];
        let population = tile
            .improvement
            .as_ref()
            .and_then(|m| {
                t.improvements
                    .get(m.kind as usize)
                    .map(|d| crate::economy::population_at_level(d, m.level))
            })
            .unwrap_or(0);
        for _ in 0..population {
            out.push(Action::City(City::IncreasePopulation {
                player: actor,
                source: tile.coordinates,
                target: city,
                delay: 60,
            }));
        }
    }
    let radius_area = crate::explore::area_sorted(g, city, radius)?;
    let mut sorted: Vec<usize> = radius_area
        .iter()
        .copied()
        .filter(|&i| g.tiles[i].ruling_city == NULL || g.tiles[i].ruling_city == city)
        .collect();
    sorted.reverse();
    crate::polaris::claim_area(g, t, actor, pi, &sorted, out)?;
    for xx in (city.x - radius).max(0)..=(city.x + radius).min(g.width - 1) {
        for yy in (city.y - radius).max(0)..=(city.y + radius).min(h - 1) {
            let c = Xy { x: xx, y: yy };
            if !g.tile(c).unwrap().explorers.contains(&actor) {
                out.push(Action::Explore {
                    player: actor,
                    coordinates: c,
                });
            }
        }
    }
    Ok(())
}

const RADIUS: i32 = 5;
const WIDTH: usize = 11;
const SIZE: usize = 121;
const OFFSETS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

fn fog_score(lookup: &[u32], index: usize, radius: usize) -> i32 {
    let (x, y) = (index % WIDTH, index / WIDTH);
    let height = lookup.len() / WIDTH;
    let (mut fog, mut lighthouse) = (0, false);
    for yy in y.saturating_sub(radius)..=(y + radius).min(height - 1) {
        for xx in x.saturating_sub(radius)..=(x + radius).min(WIDTH - 1) {
            let f = lookup[yy * WIDTH + xx];
            fog += i32::from(f & 8 != 0);
            lighthouse |= f & 0x28 == 0x28;
        }
    }
    if radius == 2 && fog == 0 && !lighthouse {
        return 9999;
    }
    (if lighthouse { 40 } else { 73 }) - 10 * fog.min(4)
}

fn scout_coordinate(center: Xy, index: usize) -> Xy {
    Xy {
        x: center.x - RADIUS + (index % WIDTH) as i32,
        y: center.y - RADIUS + (index / WIDTH) as i32,
    }
}

fn improvement_has(t: &Tables, kind: i32, a: &str) -> bool {
    t.improvements
        .get(kind as usize)
        .is_some_and(|d| d.improvement_abilities.has(a))
}

fn scout_lookup(g: &Game, t: &Tables, p: i32, center: Xy, allowed: u32) -> [u32; SIZE] {
    std::array::from_fn(|index| {
        let Some(tile) = g.tile(scout_coordinate(center, index)) else {
            return 0;
        };
        let explored = p == 255 || tile.explorers.contains(&p);
        let kind = tile.improvement.as_ref().map(|m| m.kind);
        let city = kind == Some(CITY);
        let route = kind.is_some_and(|k| improvement_has(t, k, "bridge"));
        let movable = if city || route {
            city
        } else {
            crate::routes::has_terrain(allowed, tile.terrain)
        };
        1 | if movable { 6 } else { 0 }
            | if !explored { 8 } else { 0 }
            | if route { 16 } else { 0 }
            | if kind == Some(LIGHTHOUSE) { 32 } else { 0 }
            | if explored && tile.terrain == MOUNTAIN {
                64
            } else {
                0
            }
    })
}

/// Whether the scout can cross a route, with its relation lookups (tile owner, invisible occupant)
fn can_cross(
    g: &mut Game,
    p: i32,
    center: Xy,
    from: usize,
    to: usize,
    allowed: u32,
) -> Result<bool, String> {
    let (origin, target) = (scout_coordinate(center, from), scout_coordinate(center, to));
    let tile = *g.tile(target).ok_or("scout tile")?;
    let pi = player_index(g, p)?;
    if tile.owner != 0 && tile.owner != p {
        relation_entry(g, pi, tile.owner)?;
    }
    if !tile.explorers.contains(&p) {
        return Ok(false);
    }
    if let Some(n) = tile.unit {
        let owner = g.units[n as usize].owner;
        if owner != p && crate::menu::unit_effect_list(g, g.units[n as usize].effects).contains(&3)
        {
            relation_entry(g, pi, owner)?;
        }
    }
    if tile.improvement.as_ref().is_some_and(|m| m.kind == BRIDGE)
        && !crate::routes::has_terrain(allowed, tile.terrain)
    {
        let anchor = |dx: i32, dy: i32| {
            g.tile(Xy {
                x: target.x + dx,
                y: target.y + dy,
            })
            .is_some_and(|x| ![1, 2].contains(&x.terrain))
        };
        let vertical = anchor(0, -1) && anchor(0, 1);
        let horizontal = !vertical && anchor(1, 0) && anchor(-1, 0);
        if vertical && origin.y == target.y && origin.x != target.x
            || horizontal && origin.x == target.x && origin.y != target.y
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Scout fog map: distances toward fog over the 11x11 window
fn fog_map(
    g: &mut Game,
    p: i32,
    center: Xy,
    allowed: u32,
    lookup: &mut [u32; SIZE],
) -> Result<[i32; SIZE], String> {
    let mut map = lookup.map(|f| if f & 2 != 0 { 1000 } else { 9999 });
    // each tile is queued at most once before the loop; each of its 499 dequeues queues at most 8
    let (mut queue, mut queued) = ([0u8; SIZE + 499 * 8], 0);
    for i in 0..SIZE {
        if lookup[i] & 8 != 0 {
            map[i] = 0;
            queue[queued] = i as u8;
            queued += 1;
        }
    }
    for i in 0..SIZE {
        if lookup[i] & 0x4a == 0x42 {
            let score = fog_score(lookup, i, 2);
            if score != 9999 {
                map[i] = score + 100;
                queue[queued] = i as u8;
                queued += 1;
                lookup[i] |= 128;
            }
        }
    }
    let mut cursor = 0;
    while cursor < queued && cursor <= 498 {
        let from = usize::from(queue[cursor]);
        let step = map[from] + 100;
        let index = from as i32;
        let right = if from % WIDTH < WIDTH - 1 { 1 } else { 9999 };
        let left = if from % WIDTH > 0 { -1 } else { -9999 };
        for to in [
            index + 11,
            index - 11,
            index + right,
            index + left,
            index + 11 + left,
            index + 11 + right,
            index - 11 + left,
            index - 11 + right,
        ] {
            if to < 0 || to as usize >= map.len() {
                continue;
            }
            let to = to as usize;
            let mut step = step;
            if map[to] <= step || lookup[to] & 128 != 0 {
                continue;
            }
            let crossed = lookup[to] & 16 != 0 && can_cross(g, p, center, from, to, allowed)?;
            if !crossed && 3 & !lookup[to] != 0 {
                continue;
            }
            if step == 100 && lookup[to] & 8 == 0 {
                step = fog_score(lookup, to, 1) + 100;
            }
            map[to] = step;
            queue[queued] = to as u8;
            queued += 1;
        }
        cursor += 1;
    }
    Ok(map)
}

/// The Scout action: one step of the explorer toward fog, then the next step queued
pub fn scout(g: &mut Game, t: &Tables, x: &Economy, out: &mut Vec<Action>) -> Result<(), String> {
    let Economy::Scout {
        player: p,
        unit,
        remaining,
        seed: action_seed,
        from,
        ref path,
        behaviour,
    } = *x
    else {
        return Err("not a scout action".into());
    };
    if behaviour != 0 {
        return Err("tutorial scout not implemented".into());
    }
    let mut path = *path;
    if path.is_empty() {
        path.try_push(from)?;
    }
    let allowed = crate::routes::movements(g, t, p)?;
    let mut lookup = scout_lookup(g, t, p, from, allowed);
    let mut map = fog_map(g, p, from, allowed, &mut lookup)?;
    // the backtracking penalty
    let mut penalty = 1;
    for (k, c) in path.iter().enumerate() {
        let (dx, dy) = (c.x - (from.x - RADIUS), c.y - (from.y - RADIUS));
        if (0..11).contains(&dx) && (0..11).contains(&dy) {
            map[dy as usize * WIDTH + dx as usize] += penalty;
        }
        penalty += if path.len() < 9 { 1 } else { (k & 1) as i32 };
    }
    let (mut best, mut candidates) = (i32::MAX, crate::inline::Inline::<usize, 8>::new());
    for (k, &(dx, dy)) in OFFSETS.iter().enumerate() {
        let score = map[(60 + dx + dy * 11) as usize];
        if score < best {
            best = score;
            candidates.clear();
            candidates.push(k);
        } else if score == best {
            candidates.push(k);
        }
    }
    if best > 9998 || candidates.is_empty() {
        return Ok(());
    }
    let hash = crate::step::random_hash(g)?;
    let pick = if candidates.len() == 1 {
        0
    } else {
        hash.range(0, candidates.len() as i32, &[action_seed as i32])
            .map_err(|_| "scout hash range")? as usize
    };
    let (dx, dy) = OFFSETS[candidates[pick]];
    let Some(target) = g
        .tile(Xy {
            x: from.x + dx,
            y: from.y + dy,
        })
        .map(|c| c.coordinates)
    else {
        return Ok(());
    };
    path.try_push(target)?;
    let sight = if g.tile(target).unwrap().terrain == MOUNTAIN {
        2
    } else {
        1
    };
    if remaining >= 2 {
        let seed = hash_words(hash.0 as u32, &[action_seed]);
        out.push(Action::Economy(Economy::Scout {
            player: p,
            unit,
            remaining: remaining - 1,
            seed,
            from: target,
            path,
            behaviour: 0,
        }));
    }
    crate::explore::explore_from(g, p, target, sight, out)?;
    Ok(())
}
