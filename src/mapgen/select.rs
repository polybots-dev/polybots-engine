//! Choosing the map: candidates scored for fairness until one is fair enough (the fairness
//! rules), then the generated
//! scores.
use super::{candidate, MapGeneratorSettings};
use crate::state::{Game, Tile, Xy};
use crate::tables::Tables;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

const MOUNTAIN: i32 = 4;
const CITY: i32 = 1;
const RUIN: i32 = 2;

/// What the fairness search reads of a tile, one byte per tile so the search stays in cache.
#[derive(Clone, Copy)]
struct Look(u8);

impl Look {
    const WATER: u8 = 1;
    const MOUNTAIN: u8 = 2;
    /// field, mountain, forest or ice (terrain 3 to 6)
    const LAND: u8 = 4;
    const CITY: u8 = 8;
    const RUIN: u8 = 16;

    fn of(t: &Tile) -> Look {
        let mut b = 0;
        if t.terrain == 1 || t.terrain == 2 {
            b |= Self::WATER;
        }
        if t.terrain == MOUNTAIN {
            b |= Self::MOUNTAIN;
        }
        if (3..=6).contains(&t.terrain) {
            b |= Self::LAND;
        }
        match t.improvement.as_ref().map(|m| m.kind) {
            Some(CITY) => b |= Self::CITY,
            Some(RUIN) => b |= Self::RUIN,
            _ => {}
        }
        Look(b)
    }

    fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }
}

fn turf_score(t: Look, width: i32, cost: i32) -> f32 {
    let factor = ((cost as f32 - 3.0) / (width as f32 * 0.8 - 3.0)).clamp(0.0, 1.0);
    let mut score = if t.has(Look::CITY) {
        let base = (-90.0 * factor + 100.0) + 0.1;
        match cost {
            3 => base + 40.0,
            4 => base + 20.0,
            _ => base,
        }
    } else if t.has(Look::RUIN) {
        let base = (-3.0 * factor + 5.0) + 0.1;
        if cost < 5 {
            base + 5.0
        } else {
            base
        }
    } else {
        0.1
    };
    if t.has(Look::LAND) {
        score += 1.5 - factor;
    }
    score
}

#[derive(Clone, Copy)]
struct Turf {
    index: usize,
    seat: usize,
    cost: i32,
}

/// The inequality: every tile goes to the player who reaches it most cheaply; the result is
/// the richest turf over the poorest
pub fn inequality(g: &Game) -> Result<f32, String> {
    let width = g.width;
    if g.tiles.len() as i32 != width * width {
        return Err("fairness requires a square board".into());
    }
    let seats = g.players.iter().take_while(|p| p.action.id != 255).count();
    if !(2..=4).contains(&seats) {
        return Err("invalid fairness player count".into());
    }
    let mut starts = Vec::new();
    for p in &g.players {
        starts.push(p.start_tile);
    }
    let look: Vec<Look> = g.tiles.iter().map(Look::of).collect();
    let mut scores = vec![0.0_f32; seats];
    let mut visited = vec![false; g.tiles.len()];
    // The game keeps the frontier as a list: a tile is added at the end, improved in
    // place, and the first of the cheapest is taken out. "First" is the earliest added of
    // those still waiting, so each tile's entry carries the order it was added in, and a heap
    // of (cost, order added, tile) yields the same tile. A tile whose cost has since dropped
    // has an older, dearer heap entry, which is skipped when it comes up.
    let mut waiting: BinaryHeap<Reverse<(i32, u32, usize)>> = BinaryHeap::new();
    let mut entry: Vec<Option<(Turf, u32)>> = vec![None; g.tiles.len()];
    let mut added = 0u32;
    let mut expand = |current: Turf,
                      visited: &[bool],
                      waiting: &mut BinaryHeap<Reverse<(i32, u32, usize)>>,
                      entry: &mut [Option<(Turf, u32)>]| {
        let origin = look[current.index];
        // a square board's tile i is at (i mod width, i / width)
        let center = Xy {
            x: current.index as i32 % width,
            y: current.index as i32 / width,
        };
        for x in center.x - 1..=center.x + 1 {
            for y in center.y - 1..=center.y + 1 {
                let Some(index) = g.index(Xy { x, y }) else {
                    continue;
                };
                if visited[index] || index == current.index {
                    continue;
                }
                let target = look[index];
                let step = if target.has(Look::MOUNTAIN) {
                    4
                } else if !target.has(Look::WATER) {
                    1
                } else if origin.has(Look::WATER) {
                    4
                } else if !origin.has(Look::CITY) {
                    1000
                } else if starts.contains(&center) {
                    8
                } else {
                    16
                };
                let next = Turf {
                    index,
                    seat: current.seat,
                    cost: current.cost + step,
                };
                match &mut entry[index] {
                    Some((previous, order)) => {
                        if (next.cost, next.seat) < (previous.cost, previous.seat) {
                            if next.cost < previous.cost {
                                waiting.push(Reverse((next.cost, *order, index)));
                            }
                            *previous = next;
                        }
                    }
                    none => {
                        *none = Some((next, added));
                        waiting.push(Reverse((next.cost, added, index)));
                        added += 1;
                    }
                }
            }
        }
    };
    for (seat, &start) in starts.iter().enumerate().take(seats) {
        let index = g.index(start).ok_or("missing turf start")?;
        visited[index] = true;
        scores[seat] = turf_score(look[index], width, 0);
        expand(
            Turf {
                index,
                seat,
                cost: 0,
            },
            &visited,
            &mut waiting,
            &mut entry,
        );
    }
    while let Some(Reverse((cost, _, index))) = waiting.pop() {
        // skip an entry left behind by a later improvement, or by the tile having been taken
        let Some((next, _)) = entry[index].take_if(|(turf, _)| turf.cost == cost) else {
            continue;
        };
        visited[next.index] = true;
        scores[next.seat] += turf_score(look[next.index], width, next.cost);
        expand(next, &visited, &mut waiting, &mut entry);
    }
    if visited.contains(&false) {
        return Err("fairness did not visit every tile".into());
    }
    let lowest = scores.iter().copied().fold(f32::MAX, f32::min);
    let highest = scores.iter().copied().fold(f32::MIN, f32::max);
    Ok(highest / if lowest > 0.0 { lowest } else { 0.001 })
}

/// The score total for the player in `seat`; it leaves the player's tech
/// cache holding the unlocked techs that have a cost, as the game's query does
fn total_score(g: &mut Game, t: &Tables, seat: usize) -> Result<i32, String> {
    let player = g.players[seat].action.id;
    let (mut tiles, mut explored, mut unit_score, mut city_score, mut culture) =
        (0_i32, 0_i32, 0_i32, 0_i32, 0_i32);
    for i in 0..g.tiles.len() {
        let tile = &g.tiles[i];
        if tile.owner == player {
            tiles = tiles.wrapping_add(1);
            if let Some(m) = &tile.improvement {
                let score = crate::economy::improvement_score(t, g, i);
                if m.kind == CITY {
                    city_score = city_score.wrapping_add(score);
                } else {
                    culture = culture.wrapping_add(score);
                }
            }
        }
        explored = explored.wrapping_add(i32::from(crate::explore::explored(g, i, player)));
        if let Some(n) = tile.unit {
            let u = g.units.get(n as usize).ok_or("unit node out of range")?;
            if u.owner == player {
                unit_score = unit_score.wrapping_add(crate::units::unit_score(g, t, u));
            }
        }
    }
    crate::step::refresh_tech_cache(g, t, player)?;
    let unlocked = crate::step::tech_cache(g, seat)?.to_vec();
    let mut scored = Vec::new();
    let mut tech_score = 0_i32;
    for tech in unlocked {
        let cost = t.techs.get(tech as usize).ok_or("unknown tech")?.cost;
        if cost != 0 {
            scored.push(tech);
            let value = f64::from(cost.wrapping_mul(100) as f32);
            tech_score =
                tech_score.wrapping_add(if value >= 2147483648.0 || value < -2147483648.0 {
                    i32::MIN
                } else {
                    value as i32
                });
        }
    }
    g.players[seat].unlocked_tech_cache = Some(scored.into_iter().collect());
    let territory = tiles
        .wrapping_mul(20)
        .wrapping_add(unit_score)
        .wrapping_add(explored.wrapping_mul(5));
    Ok(culture
        .wrapping_add(city_score)
        .wrapping_add(tech_score)
        .wrapping_add(territory))
}

/// Refreshes the generated scores: every player's score is its computed total (Nature 0)
pub fn refresh_scores(g: &mut Game, t: &Tables) -> Result<(), String> {
    for seat in 0..g.players.len() {
        let total = if g.players[seat].action.id == 255 {
            0
        } else {
            total_score(g, t, seat)?
        };
        g.players[seat].score = i64::from(total as u32);
    }
    Ok(())
}

/// Selects the map candidate: generate candidates from `next_seed` until one is under the
/// fairness limit or the attempts run out, keep the fairest, then refresh the scores.
/// Returns the chosen candidate seed.
pub fn select(
    g: &mut Game,
    t: &Tables,
    settings: &MapGeneratorSettings,
    preferred: Option<usize>,
    mut next_seed: impl FnMut() -> i32,
) -> Result<i32, String> {
    let mut limit = settings.equality_iterations;
    let mut retries = if limit == 1 { 0 } else { 15 };
    let mut best_score = f32::MAX;
    let (mut best_seed, mut last_seed) = (-1, -1);
    let mut attempts = 0;
    // the continents of the candidate now on the board
    let mut drawn = None;
    while attempts < limit {
        let seed = next_seed();
        if !(0..i32::MAX).contains(&seed) {
            return Err("candidate seed outside the GetInt32 range".into());
        }
        drawn = Some(candidate::draw(g, t, seed, settings, preferred)?);
        last_seed = seed;
        attempts += 1;
        let score = inequality(g)?;
        if score >= -999.0 {
            if score < best_score {
                best_seed = seed;
                best_score = score;
            }
            if score < settings.equality_limit {
                best_seed = seed;
                break;
            }
        } else {
            retries -= 1;
            if retries > 0 {
                limit = limit.wrapping_add(1);
            }
        }
    }
    if limit == 1 {
        best_seed = last_seed;
    }
    if last_seed != best_seed {
        drawn = Some(candidate::draw(g, t, best_seed, settings, preferred)?);
    }
    if let Some(drawn) = drawn {
        drawn.record(g)?;
    }
    refresh_scores(g, t)?;
    Ok(best_seed)
}
