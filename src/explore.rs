//! Exploration and first contact: exploring from a tile (queued form), Explore and Meet actions
//! and the lighthouse corner reward.

use crate::action::{Action, City, Economy, Payload};
use crate::state::PEACE;
use crate::state::{Game, Xy};
use crate::step::{player_index, refresh_tech_cache, relation_entry, task_notification};
use crate::tables::Tables;

const MOUNTAIN: i32 = 4;
const CITY: i32 = 1;
const INVISIBLE: i64 = 3;
const LIGHTHOUSE_TASK: usize = 8;

pub(crate) fn explored(g: &Game, i: usize, p: i32) -> bool {
    p == 255 || g.tiles[i].explorers.contains(&p)
}

/// The tiles of a sorted area, held in place. 121 is the square of radius 5; the catalogue's
/// sight ranges and city borders stay well inside it, and a larger radius is refused.
pub(crate) type Area = crate::inline::Inline<usize, 121>;

/// The square of `radius` around `c`, ordered by Manhattan distance with the
/// game's introsort (not stable: equal distances keep its exact order)
pub(crate) fn area_sorted(g: &Game, c: Xy, radius: i32) -> Result<Area, String> {
    g.index(c).ok_or("area centre off the board")?;
    let mut area = Area::new();
    if radius < 0 {
        return Ok(area);
    }
    if radius > 5 {
        return Err("area radius above 5".into());
    }
    let h = g.tiles.len() as i32 / g.width;
    let mut pairs = [(0f32, 0usize); 121];
    let mut n = 0;
    for x in (c.x - radius).max(0)..=(c.x + radius).min(g.width - 1) {
        for y in (c.y - radius).max(0)..=(c.y + radius).min(h - 1) {
            pairs[n] = (
                ((x - c.x).abs() + (y - c.y).abs()) as f32,
                g.index(Xy { x, y }).unwrap(),
            );
            n += 1;
        }
    }
    sort_slots(&mut pairs[..n]);
    for pair in &pairs[..n] {
        area.push(pair.1);
    }
    Ok(area)
}

/// The game's introsort over (key, slot) pairs
pub(crate) fn sort_slots(pairs: &mut [(f32, usize)]) {
    if pairs.len() > 1 {
        let depth = (usize::BITS - pairs.len().leading_zeros()) as i32 * 2;
        let hi = pairs.len() - 1;
        intro(pairs, 0, hi, depth);
    }
}

fn swap_greater(p: &mut [(f32, usize)], a: usize, b: usize) {
    if a != b && p[a].0 > p[b].0 {
        p.swap(a, b);
    }
}

fn intro(p: &mut [(f32, usize)], lo: usize, mut hi: usize, mut depth: i32) {
    while hi > lo {
        depth -= 1;
        let n = hi - lo + 1;
        if n <= 16 {
            match n {
                2 => swap_greater(p, lo, hi),
                3 => {
                    swap_greater(p, lo, hi - 1);
                    swap_greater(p, lo, hi);
                    swap_greater(p, hi - 1, hi);
                }
                _ => {
                    for i in lo + 1..=hi {
                        let saved = p[i];
                        let mut at = i;
                        while at > lo && saved.0 < p[at - 1].0 {
                            p[at] = p[at - 1];
                            at -= 1;
                        }
                        p[at] = saved;
                    }
                }
            }
            return;
        }
        if depth == -1 {
            heap(p, lo, hi);
            return;
        }
        let pivot = partition(p, lo, hi);
        intro(p, pivot + 1, hi, depth);
        hi = pivot - 1;
    }
}

fn partition(p: &mut [(f32, usize)], lo: usize, hi: usize) -> usize {
    let mid = lo + (hi - lo) / 2;
    swap_greater(p, lo, mid);
    swap_greater(p, lo, hi);
    swap_greater(p, mid, hi);
    let pivot = p[mid].0;
    p.swap(mid, hi - 1);
    let (mut l, mut r) = (lo, hi - 1);
    loop {
        l += 1;
        while p[l].0 < pivot {
            l += 1;
        }
        r -= 1;
        while pivot < p[r].0 {
            r -= 1;
        }
        if l >= r {
            break;
        }
        p.swap(l, r);
    }
    p.swap(l, hi - 1);
    l
}

fn heap(p: &mut [(f32, usize)], lo: usize, hi: usize) {
    let n = hi - lo + 1;
    for i in (1..=n / 2).rev() {
        down(p, i, n, lo);
    }
    for i in (2..=n).rev() {
        p.swap(lo, lo + i - 1);
        down(p, 1, i - 1, lo);
    }
}

fn down(p: &mut [(f32, usize)], mut i: usize, n: usize, lo: usize) {
    let saved = p[lo + i - 1];
    while i <= n / 2 {
        let mut child = 2 * i;
        if child < n && p[lo + child - 1].0 < p[lo + child].0 {
            child += 1;
        }
        if saved.0 >= p[lo + child - 1].0 {
            break;
        }
        p[lo + i - 1] = p[lo + child - 1];
        i = child;
    }
    p[lo + i - 1] = saved;
}

/// Explore from a tile: one Explore per unexplored tile in sight, far to near
pub fn explore_from(
    g: &Game,
    p: i32,
    c: Xy,
    sight: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    player_index(g, p)?;
    let i = g.index(c).ok_or("exploration origin off the board")?;
    let radius = if g.tiles[i].terrain == MOUNTAIN {
        2
    } else {
        sight
    };
    for &j in area_sorted(g, c, radius)?.iter().rev() {
        if !explored(g, j, p) {
            out.push(Action::Explore {
                player: p,
                coordinates: g.tiles[j].coordinates,
            });
        }
    }
    Ok(())
}

pub(crate) fn knows(g: &Game, p: i32, other: i32) -> Result<bool, String> {
    let me = g.player(p).ok_or("missing player")?;
    Ok(other == 255 || p == other || me.known_players.contains(&other))
}

/// The visible unit's owner (exploration form), inserting a relation for an invisible foreign unit
pub(crate) fn visible_unit_owner(g: &mut Game, p: i32, i: usize) -> Result<Option<i32>, String> {
    if !explored(g, i, p) {
        return Ok(None);
    }
    let Some(n) = g.tiles[i].unit else {
        return Ok(None);
    };
    let u = g.units.get(n as usize).ok_or("unit node out of range")?;
    let owner = u.owner;
    if crate::menu::unit_effect_list(g, u.effects).contains(&INVISIBLE) {
        let pi = player_index(g, p)?;
        if owner != p {
            if owner == 0 {
                return Ok(None);
            }
            if relation_entry(g, pi, owner)?.state != PEACE {
                return Ok(None);
            }
        }
    }
    Ok(Some(owner))
}

pub(crate) fn meet(p: i32, other: i32, c: Xy) -> Action {
    Action::Meet {
        player: p,
        other: other,
        coordinates: c,
        technology: 0,
    }
}

fn current_capital(g: &Game, id: i32) -> Result<Xy, String> {
    crate::step::current_capital(g, id)
}

/// The Explore action: queues nothing when the tile is already explored.
pub fn explore(
    g: &mut Game,
    t: &Tables,
    p: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let Some(i) = g.index(c) else { return Ok(()) };
    if explored(g, i, p) {
        return Ok(());
    }
    g.tiles[i].explorers.push(p);
    g.needs_routes = true;
    out.push(Action::Payload(Payload::IncreaseScore {
        player: p,
        amount: 5,
        delay: 20,
        source: c,
    }));
    let unit_owner = visible_unit_owner(g, p, i)?;
    let unit_contact = match unit_owner {
        Some(o) => !knows(g, p, o)?,
        None => false,
    };
    let city_owner = g.tiles[i]
        .improvement
        .as_ref()
        .filter(|m| m.kind == CITY)
        .map(|_| g.tiles[i].owner);
    let city_contact = match city_owner {
        Some(o) if o != 0 && o != p => !knows(g, p, o)?,
        _ => false,
    };
    if unit_contact {
        out.push(meet(p, unit_owner.unwrap(), c));
    }
    if city_contact && (!unit_contact || unit_owner != city_owner) {
        out.push(meet(p, city_owner.unwrap(), c));
    }
    reveal_lighthouse(g, t, p, c, out)?;
    Ok(())
}

/// What seeing a map corner brings, for a lighthouse (nothing for any other tile)
pub(crate) fn reveal_lighthouse(
    g: &mut Game,
    t: &Tables,
    p: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let (w, h) = (g.width - 1, g.tiles.len() as i32 / g.width - 1);
    if [(w, h), (0, 0), (0, h), (w, 0)].contains(&(c.x, c.y)) {
        let pi = player_index(g, p)?;
        out.extend(task_notification(g, t, pi, LIGHTHOUSE_TASK)?);
        if g.players[pi].task(8).is_none() {
            out.push(Action::EnableTask {
                player: p,
                task: LIGHTHOUSE_TASK as u32,
            });
        }
        let capital = current_capital(g, p)?;
        if capital != (Xy { x: -1, y: -1 }) {
            out.push(Action::City(City::IncreasePopulation {
                player: p,
                source: c,
                target: capital,
                delay: 60,
            }));
        }
        out.push(Action::Economy(Economy::ImprovementLevelUp {
            player: p,
            target: c,
        }));
    }
    Ok(())
}

/// The Meet action: queues nothing when the player already knows the other.
pub fn meet_action(
    g: &mut Game,
    t: &Tables,
    p: i32,
    other: i32,
    coordinates: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    if g.player(p).is_none() || knows(g, p, other)? {
        return Ok(());
    }
    if g.player(other).is_none() {
        return Ok(());
    }
    // capital vision explores the other's original capital
    refresh_tech_cache(g, t, p)?;
    if crate::menu::player_ability(g, t, p, "capitalvision") {
        let start = g.player(other).unwrap().start_tile;
        let si = g.index(start).ok_or("capital off the board")?;
        if !explored(g, si, p) {
            out.push(Action::Explore {
                player: p,
                coordinates: start,
            });
        }
    }
    let pi = player_index(g, p)?;
    g.players[pi].known_players.push(other);
    relation_entry(g, pi, other)?.first_meet = g.current_turn;
    let other_score = g.player(other).unwrap().score as u32;
    let tiers = ((other_score as f32) / 1000.0_f32).ceil() as i32;
    let count = g.players.len() as i32 - 1;
    let amount = tiers.min(5) * 2 + if count == 2 && other == 1 { 3 } else { 1 };
    out.push(Action::Payload(Payload::IncreaseCurrency {
        player: p,
        delay: 40,
        source: coordinates,
        amount: amount,
    }));
    Ok(())
}
