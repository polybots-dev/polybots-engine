//! Ruins: the ExamineRuins command, its seeded reward choice and the Ruins action.

use crate::action::{Action, City, Combat, Economy, MoveEffect, Payload};
use crate::state::{Game, Xy};
use crate::step::player_index;
use crate::tables::Tables;

const OCEAN: i32 = 2;
const FIELD: i32 = 3;
const ICE: i32 = 6;
const FLOODED: i32 = 1;
/// The reward that raises a lost city
const LOST_CITY: i64 = 9;
const CITY: i32 = 1;
const RUIN: i32 = 2;
const WATER: i32 = 1;
const LAND: [i32; 4] = [3, 4, 5, 6];
const INVISIBLE: i64 = 3;
const NULL: Xy = Xy { x: -1, y: -1 };
const RESOURCES: i64 = 1;
const POPULATION: i64 = 2;
const EXPLORER: i64 = 3;
const FREE_TECH: i64 = 4;
const BATTLESHIP: i64 = 6;
const SWORDSMAN: i64 = 8;

/// tiles within `radius` of `c` (x outer), excluding `c`; with `diagonal` false only the cross
fn area(g: &Game, c: Xy, radius: i32, diagonal: bool) -> Vec<usize> {
    let h = g.tiles.len() as i32 / g.width;
    let mut out = Vec::new();
    for x in (c.x - radius).max(0)..=(c.x + radius).min(g.width - 1) {
        for y in (c.y - radius).max(0)..=(c.y + radius).min(h - 1) {
            if (x != c.x || y != c.y) && (diagonal || x == c.x || y == c.y) {
                out.push(g.index(Xy { x, y }).unwrap());
            }
        }
    }
    out
}

/// A valid lost city: an unowned ocean tile off the board's edge with no city or ruin
/// within two tiles
fn valid_lost_city(g: &Game, i: usize) -> bool {
    let x = &g.tiles[i];
    let c = x.coordinates;
    let h = g.tiles.len() as i32 / g.width;
    if c.x <= 0
        || c.y <= 0
        || c.x >= g.width - 1
        || c.y >= h - 1
        || x.owner != 0
        || x.terrain != OCEAN
    {
        return false;
    }
    !area(g, c, 2, true).into_iter().any(|j| {
        g.tiles[j]
            .improvement
            .as_ref()
            .is_some_and(|m| m.kind == CITY || m.kind == RUIN)
    })
}

/// Select the reward: a seeded shuffle of the six rewards, the first valid one wins
fn select_reward(g: &mut Game, t: &Tables, p: i32, c: Xy) -> Result<i64, String> {
    let i = g.index(c).ok_or("ruins tile")?;
    let pi = player_index(g, p)?;
    let terrain = g.tiles[i].terrain;
    if !LAND.contains(&terrain)
        && t.tribes
            .get(g.players[pi].tribe as usize)
            .is_some_and(|d| d.tribe_abilities.has("flooded"))
        && valid_lost_city(g, i)
    {
        return Ok(LOST_CITY);
    }
    let mut candidates = [
        RESOURCES, POPULATION, SWORDSMAN, BATTLESHIP, EXPLORER, FREE_TECH,
    ];
    let hash = crate::step::random_hash(g)?;
    for k in 0..5 {
        let j = hash
            .range(k, 6, &[c.x, c.y, k])
            .map_err(|_| "ruin shuffle range")?;
        candidates.swap(k as usize, j as usize);
    }
    for candidate in candidates {
        let valid = match candidate {
            EXPLORER => {
                let mut count = 0;
                for n in area(g, c, 1, true) {
                    let allowed = crate::routes::movements(g, t, p)?;
                    let x = &g.tiles[n];
                    count += usize::from(
                        crate::routes::has_terrain(allowed, x.terrain)
                            || x.improvement.as_ref().is_some_and(|m| {
                                t.improvements
                                    .get(m.kind as usize)
                                    .is_some_and(|d| d.improvement_abilities.has("bridge"))
                            }),
                    );
                }
                count > 0
                    && area(g, c, 2, true)
                        .into_iter()
                        .any(|n| !g.tiles[n].explorers.contains(&p))
            }
            FREE_TECH => !crate::menu::unlockable_techs(g, t, p)?.is_empty(),
            BATTLESHIP => {
                (terrain == WATER || terrain == OCEAN)
                    && crate::routes::has_terrain(crate::routes::movements(g, t, p)?, OCEAN)
            }
            SWORDSMAN => {
                let others: Vec<i32> = g
                    .players
                    .iter()
                    .map(|q| q.action.id)
                    .filter(|&q| q != p)
                    .collect();
                terrain != WATER && terrain != OCEAN && {
                    let mut any = false;
                    for q in others {
                        any |= crate::menu::alive(g, q)?;
                    }
                    any
                }
            }
            _ => true,
        };
        if valid {
            return Ok(candidate);
        }
    }
    Ok(RESOURCES)
}

/// The ExamineRuins command's children
pub fn examine_command(
    g: &mut Game,
    t: &Tables,
    p: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    crate::step::base(out, p, 14);
    let unit = g.tile(c).ok_or("ruins tile")?.unit;
    let reward = select_reward(g, t, p, c)?;
    out.push(Action::Ruins {
        player: p,
        reward,
        coordinates: c,
    });
    if let Some(n) = unit {
        if crate::menu::unit_effect_list(g, g.units[n as usize].effects).contains(&INVISIBLE) {
            out.push(Action::MoveEffect(MoveEffect::Reveal {
                player: p,
                coordinates: c,
                origin: NULL,
                show_popup: false,
            }));
        }
    }
    Ok(())
}

/// The Ruins action: clear the ruin, exhaust the unit, grant the reward
pub fn ruins(
    g: &mut Game,
    t: &Tables,
    p: i32,
    reward: i64,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let Ok(pi) = player_index(g, p) else {
        return Ok(());
    };
    let i = g.index(c).ok_or("ruins tile")?;
    g.tiles[i].improvement = None;
    if let Some(n) = g.tiles[i].unit {
        crate::units::make_exhausted(g, n)?;
    }
    match reward {
        RESOURCES => {
            for remaining in (1..=10).rev() {
                out.push(Action::Payload(Payload::IncreaseCurrency {
                    player: p,
                    delay: if remaining == 1 { 190 } else { 40 },
                    source: c,
                    amount: 1,
                }));
            }
        }
        POPULATION => {
            let capital = crate::step::current_capital(g, g.players[pi].action.id)?;
            if capital != NULL {
                for delay in [0, 60, 120] {
                    out.push(Action::City(City::IncreasePopulation {
                        player: p,
                        source: c,
                        target: capital,
                        delay,
                    }));
                }
            }
        }
        EXPLORER => {
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
        FREE_TECH => {
            let list = crate::menu::unlockable_techs(g, t, p)?;
            let choice = crate::step::random_hash(g)?
                .range(0, list.len() as i32, &[c.x, c.y])
                .map_err(|_| "empty ruin technology choice")?;
            out.push(Action::Research {
                player: p,
                technology: list[choice as usize] as i64,
                cost: 0,
            });
        }
        r @ (5..=8) => {
            let kind = match r {
                5 => 12,
                6 => 44,
                7 => 16,
                _ => 11,
            };
            if r == BATTLESHIP || r == SWORDSMAN {
                out.push(Action::Combat(Combat::Promote {
                    player: p,
                    coordinates: c,
                }));
            }
            crate::units::train_on_occupied(g, t, p, kind, c, out)?;
        }
        LOST_CITY => lost_city(g, t, p, pi, c, out)?,
        r => return Err(format!("ruins reward {r} not implemented")),
    }
    Ok(())
}

/// Lost city: the cardinal tiles become water (not ice), a level-1 city rises on a
/// flooded field, a new (detached) continent of the player's takes the area's unclaimed
/// tiles, resources are placed around it with the gameplay hash, and the player captures it
fn lost_city(
    g: &mut Game,
    t: &Tables,
    p: i32,
    pi: usize,
    target: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    use crate::mapgen::candidate::{
        fresh_continent, place_resource, set_continent_player, Continent,
    };
    let center = g.index(target).ok_or("lost city centre")?;
    for i in area(g, target, 1, false) {
        if g.tiles[i].terrain != ICE {
            g.tiles[i].terrain = WATER;
            g.tiles[i].altitude = -1;
        }
    }
    g.tiles[center].improvement = Some(crate::state::Improvement {
        kind: CITY,
        owner: 0,
        founder: 0,
        level: 1,
        founded: 0,
        xp: 0,
        population: 0,
        production: 1,
        base_score: 0,
        border_size: 1,
        upgrade: 0,
        connected_to_capital_of_player: 0,
        named: false,
        rewards: Default::default(),
        effects: Default::default(),
    });
    if g.side.names[center].is_some() {
        std::sync::Arc::make_mut(&mut g.side).names[center] = None;
    }
    g.tiles[center].terrain = FIELD;
    if !g.tiles[center].effects.contains(&FLOODED) {
        g.tiles[center].effects.try_push(FLOODED)?;
    }
    let mut continents: Vec<Continent> =
        serde_json::from_value(g.continents().clone()).map_err(|e| format!("continents: {e}"))?;
    let mut continent = fresh_continent();
    set_continent_player(g, t, &mut continent, pi, false)?;
    continent.detached = true;
    let id = continents.len() as i32;
    continents.push(continent);
    let resources: Vec<i32> = (0..t.resources.len())
        .filter(|&k| t.resources.get(k).is_some())
        .map(|k| k as i32)
        .collect();
    let hash = crate::step::random_hash(g)?;
    let near = area(g, target, 1, true);
    for (list, threshold) in [
        (near.clone(), 1.0_f32),
        (area(g, target, 2, true), f32::from_bits(0x3eaaaaab)),
    ] {
        for i in list {
            if threshold < 1.0 && near.contains(&i) {
                continue;
            }
            let x = &g.tiles[i];
            let c = x.coordinates;
            if hash.value(&[c.x, c.y]) <= threshold
                && x.resource.is_none()
                && x.improvement.is_none()
                && x.owner == 0
            {
                if g.tiles[i].continent.is_none() {
                    g.tiles[i].continent = Some(id);
                }
                place_resource(g, t, i, &resources, &continents, hash)?;
            }
        }
    }
    g.set_continents(serde_json::to_value(&continents).map_err(|e| e.to_string())?);
    crate::mapgen::candidate::generate_shorelines(g);
    out.push(Action::UpdateCityConnections {
        actor: p,
        players: crate::inline::Seats::from_iter([p]),
    });
    out.push(Action::UpdateRoutes { player: p });
    out.push(Action::Combat(Combat::CaptureCity {
        player: p,
        coordinates: target,
        old_owner: 0,
    }));
    Ok(())
}
