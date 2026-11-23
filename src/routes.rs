//! Routes and city connections: the game UpdateRoutes, UpdateCityConnections and CityConnection
//! actions.
//!
//! The game's route queries refresh the unlocked-tech cache and insert default relation
//! records as they go. The route path search reproduces those writes; the connected-cities
//! search does not, since a default record reads the same as none.

use crate::action::{Action, City, CityConnection};
use crate::state::{Game, Xy};
use crate::state::{COOL_DOWN, PEACE};
use crate::step::{player_index, refresh_tech_cache, relation_state, task_notification};
use crate::tables::Tables;
use std::collections::VecDeque;

const WATER: i32 = 1;
const OCEAN: i32 = 2;
const FIELD: i32 = 3;
const MOUNTAIN: i32 = 4;
const FOREST: i32 = 5;
const ICE: i32 = 6;
const WETLAND: i32 = 7;
const CITY: i32 = 1;
const BRIDGE: i32 = 48;
const NETWORK_TASK: usize = 3;

fn improvement_has(t: &Tables, kind: i32, ability: &str) -> bool {
    t.improvements
        .get(kind as usize)
        .is_some_and(|d| d.improvement_abilities.has(ability))
}

/// The improvement's route terrain, range and Network ability
#[derive(Debug, PartialEq)]
pub(crate) struct Router {
    /// the route terrains, as a mask by terrain id
    pub(crate) terrain: u32,
    range: i32,
    connects_cities: bool,
}

/// The improvement kind's router, built once per catalog.
pub(crate) fn router(t: &Tables, kind: i32) -> Result<Option<&Router>, String> {
    let all = t.routers.get_or_init(|| {
        (0..t.improvements.len())
            .map(|k| build_router(t, k as i32))
            .collect()
    });
    match all.get(kind as usize) {
        Some(Ok(r)) => Ok(r.as_ref()),
        Some(Err(e)) => Err(e.clone()),
        None => Ok(None),
    }
}

fn build_router(t: &Tables, kind: i32) -> Result<Option<Router>, String> {
    let Some(d) = t.improvements.get(kind as usize) else {
        return Ok(None);
    };
    let mut terrain = 0u32;
    for v in &d.routes {
        let id = t.terrain.id(v).map_err(|_| "route terrain")?;
        terrain |= 1u32.checked_shl(id as u32).ok_or("route terrain")?;
    }
    Ok(Some(Router {
        terrain,
        range: d.range.unwrap_or(0),
        connects_cities: improvement_has(t, kind, "network"),
    }))
}

/// Whether terrain `id` is in the mask `terrains`.
pub(crate) fn has_terrain(terrains: u32, id: i32) -> bool {
    crate::tables::terrain_bit(id) & terrains != 0
}

/// The terrains player `p`'s techs unlock, from the refreshed cache
pub(crate) fn movements(g: &mut Game, t: &Tables, p: i32) -> Result<u32, String> {
    refresh_tech_cache(g, t, p)?;
    let i = player_index(g, p)?;
    t.movements(crate::step::tech_cache(g, i)?)
}

/// Whether the player can embark: an unlocked, non-hidden improvement with Embark
pub(crate) fn can_embark(g: &mut Game, t: &Tables, p: i32) -> Result<bool, String> {
    refresh_tech_cache(g, t, p)?;
    let i = player_index(g, p)?;
    let tree = t.tribe_tree(g.players[i].tribe)?;
    for &k in crate::step::tech_cache(g, i)? {
        for &id in tree.improvements_of(k)? {
            let d = t.improvements.get(id).unwrap();
            if !d.hidden && d.improvement_abilities.has("embark") {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn is_water(terrain: i32) -> bool {
    terrain == WATER || terrain == OCEAN
}

/// Tile accessibility for the null-unit router profile
fn accessible(
    g: &mut Game,
    t: &Tables,
    p: i32,
    allowed: u32,
    from: usize,
    to: usize,
    reads: &mut Reads,
) -> Result<bool, String> {
    reads.tile(from);
    reads.tile(to);
    let owner = g.tiles[to].owner;
    let r = if owner != 0 {
        Some(reads.relation(g, p, owner)?)
    } else {
        None
    };
    let owned_or_peace = owner == p || r == Some(PEACE);
    if owner != 0 && owner != p && !owned_or_peace {
        return Ok(false);
    }
    if !(p == 255 || g.tiles[to].explorers.contains(&p)) {
        return Ok(false);
    }
    let target = &g.tiles[to];
    let kind = target.improvement.as_ref().map(|m| m.kind);
    if let Some(k) = kind {
        if improvement_has(t, k, "bridge") {
            if k == BRIDGE && !has_terrain(allowed, target.terrain) {
                let c = target.coordinates;
                for (dx, dy) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
                    if let Some(i) = g.index(Xy {
                        x: c.x + dx,
                        y: c.y + dy,
                    }) {
                        reads.tile(i);
                    }
                }
                let anchor = |dx: i32, dy: i32| {
                    g.index(Xy {
                        x: c.x + dx,
                        y: c.y + dy,
                    })
                    .is_some_and(|i| !is_water(g.tiles[i].terrain))
                };
                let (h, v) = if anchor(0, -1) && anchor(0, 1) {
                    (true, false)
                } else if anchor(-1, 0) && anchor(1, 0) {
                    (false, true)
                } else {
                    (false, false)
                };
                let (dx, dy) = (
                    g.tiles[from].coordinates.x - c.x,
                    g.tiles[from].coordinates.y - c.y,
                );
                if h && dy == 0 && dx != 0 || v && dx == 0 && dy != 0 {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
    }
    let terrain = target.terrain;
    let wet = crate::aquarion::wetland(target) && has_terrain(allowed, WETLAND);
    if is_water(terrain) {
        if wet {
            return Ok(true);
        }
        let origin = &g.tiles[from];
        if [FIELD, MOUNTAIN, FOREST, ICE].contains(&origin.terrain)
            && !crate::aquarion::wetland(origin)
        {
            let embark = kind.is_some_and(|k| improvement_has(t, k, "embark"));
            if !embark || !owned_or_peace || !reads.embark(g, t, p)? {
                return Ok(false);
            }
        }
    }
    Ok(kind == Some(CITY) || has_terrain(allowed, terrain) || wet)
}

/// Whether there is a router path; it is left in `a.path`, destination first
#[allow(clippy::too_many_arguments)] // the rule's inputs, passed as the step holds them
fn router_path(
    g: &mut Game,
    t: &Tables,
    p: i32,
    start: usize,
    dest: usize,
    allowed: u32,
    max_cost: i32,
    a: &mut Arrays,
    reads: &mut Reads,
) -> Result<bool, String> {
    let Arrays {
        open,
        costs,
        came_from,
        path,
        route,
        ..
    } = a;
    reads.tile(start);
    reads.tile(dest);
    path.clear();
    path.push(dest);
    if start == dest {
        return Ok(true);
    }
    if g.tiles[dest]
        .improvement
        .as_ref()
        .is_some_and(|m| m.kind == CITY)
        && g.tiles[dest].owner != 0
    {
        let r = reads.relation(g, p, g.tiles[dest].owner)?;
        if r == PEACE || r == COOL_DOWN {
            return Ok(false);
        }
    }
    let (s, d) = (g.tiles[start].coordinates, g.tiles[dest].coordinates);
    open.clear();
    open.push((start, ((d.x - s.x).abs() + (d.y - s.y).abs()) as f32));
    reset(costs, g.tiles.len(), 0);
    reset(came_from, g.tiles.len(), 0);
    let limit = max_cost.wrapping_mul(10) as f32;
    for _ in 0..1000 {
        if open.is_empty() {
            return Ok(false);
        }
        let current = open.remove(0).0;
        if current == dest {
            let mut c = dest;
            for _ in 0..=g.tiles.len() {
                c = came_from[c];
                path.push(c);
                if c == start {
                    return Ok(true);
                }
            }
            return Err("broken router predecessor chain".into());
        }
        let cost = costs[current];
        if (cost as f32) < limit && allowed != 0 {
            let cc = g.tiles[current].coordinates;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let Some(next) = g.index(Xy {
                        x: cc.x + dx,
                        y: cc.y + dy,
                    }) else {
                        continue;
                    };
                    if !accessible(g, t, p, allowed, current, next, reads)? {
                        continue;
                    }
                    let n = &g.tiles[next];
                    let step = if n.has_road
                        || route[next]
                        || n.improvement
                            .as_ref()
                            .is_some_and(|m| m.kind == CITY || m.kind == BRIDGE)
                    {
                        5
                    } else {
                        10
                    };
                    let new_cost = cost.wrapping_add(step);
                    if costs[next] < 1 || new_cost < costs[next] {
                        costs[next] = new_cost;
                        came_from[next] = current;
                        let nc = n.coordinates;
                        let (ex, ey) = (d.x - nc.x, d.y - nc.y);
                        let priority =
                            (f64::from(ex * ex + ey * ey).sqrt()) as f32 + new_cost as f32;
                        let at = open
                            .iter()
                            .position(|&(_, q)| q > priority)
                            .unwrap_or(open.len());
                        open.insert(at, (next, priority));
                    }
                }
            }
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)] // the rule's inputs, and what the pass keeps
fn find_path_between(
    g: &mut Game,
    t: &Tables,
    p: i32,
    origin: usize,
    dest: usize,
    moves: u32,
    arrays: &mut Arrays,
    kept: &mut RouteSearches,
) -> Result<(), String> {
    let ok = g.tiles[origin].improvement.as_ref().unwrap().kind;
    let dk = g.tiles[dest].improvement.as_ref().unwrap().kind;
    let (Some(o), Some(dd)) = (router(t, ok)?, router(t, dk)?) else {
        return Ok(());
    };
    let (a, b) = (g.tiles[origin].coordinates, g.tiles[dest].coordinates);
    if (a.x - b.x).abs().max((a.y - b.y).abs()) > o.range {
        return Ok(());
    }
    let allowed = if dk == CITY {
        o.terrain
    } else {
        o.terrain & dd.terrain & moves
    };
    if allowed == 0 {
        return Ok(());
    }
    let range = o.range;
    let n = g.tiles.len();
    let w = n.div_ceil(64);
    if let Some(mut e) = kept.take(p, origin, dest) {
        if e.still_holds(&kept.words, w, g, t, p, allowed, &arrays.route_bits)? {
            kept.reused += 1;
            let path = e.path(&kept.words, w);
            if crate::shadow::on() {
                // the full search must find the same path and insert no relation record
                let records = g.player(p).map(|x| x.relations);
                let mut scratch = Reads::default();
                scratch.clear(n);
                let found =
                    router_path(g, t, p, origin, dest, allowed, range, arrays, &mut scratch)?;
                let same = match path {
                    Some(kept) => {
                        found
                            && kept
                                .iter()
                                .map(|&i| i as usize)
                                .eq(arrays.path.iter().copied())
                    }
                    None => !found,
                };
                if !same || g.player(p).map(|x| x.relations) != records {
                    return Err(crate::shadow::differs("routes", "kept search"));
                }
            }
            for &i in path.unwrap_or_default() {
                mark_route(arrays, i as usize);
            }
            // into this update's words
            let at = kept.words.len() - kept.words_last;
            kept.words
                .extend_from_within(e.at..e.at + 2 * w + e.path_len);
            e.at = at;
            kept.searches.push(Some(e));
            return Ok(());
        }
    }
    kept.searched += 1;
    let mut reads = std::mem::take(&mut kept.scratch);
    reads.clear(n);
    // the route marks found so far, before the search adds its own
    let from = kept.words.len();
    kept.words.extend_from_slice(&arrays.route_bits);
    let found = router_path(g, t, p, origin, dest, allowed, range, arrays, &mut reads);
    let found = match found {
        Ok(found) => found,
        Err(e) => {
            kept.scratch = reads;
            return Err(e);
        }
    };
    for (seen, m) in kept.words[from..from + w].iter_mut().zip(&reads.tiles) {
        *seen &= m;
    }
    kept.words.extend_from_slice(&reads.tiles);
    let mut path_len = 0;
    if found {
        path_len = arrays.path.len();
        kept.words.extend(arrays.path.iter().map(|&i| i as u64));
        for k in 0..path_len {
            let i = arrays.path[k];
            mark_route(arrays, i);
        }
    }
    // a search that read more relations than a kept one holds (only with more players than
    // `keeps` allows, when nothing is kept anyway) is never reused
    let fits = reads.relations.len() <= MAX_RELATIONS;
    let count = reads.relations.len().min(MAX_RELATIONS);
    let mut relations = [(0, 0); MAX_RELATIONS];
    relations[..count].copy_from_slice(&reads.relations[..count]);
    kept.searches.push(Some(RouteSearch {
        player: p,
        origin,
        dest,
        allowed,
        valid: fits,
        at: from - kept.words_last,
        path_len,
        relations,
        relation_count: count as u8,
        embark: reads.embark,
        found,
    }));
    kept.scratch = reads;
    Ok(())
}

fn mark_route(a: &mut Arrays, i: usize) {
    a.route[i] = true;
    a.route_bits[i / 64] |= 1 << (i % 64);
}

/// What one route search read: the tiles, the relation states in the order first looked up
/// (through the inserting lookup), and whether the player can embark, if asked.
#[derive(Clone, Default)]
struct Reads {
    tiles: Vec<u64>,
    relations: Vec<(i32, i32)>,
    embark: Option<bool>,
}

impl Reads {
    /// nothing read yet, on a board of `n` tiles (in the buffers it has)
    fn clear(&mut self, n: usize) {
        self.tiles.clear();
        self.tiles.resize(n.div_ceil(64), 0);
        self.relations.clear();
        self.relations.reserve(MAX_RELATIONS);
        self.embark = None;
    }
    fn tile(&mut self, i: usize) {
        self.tiles[i / 64] |= 1 << (i % 64);
    }
    fn relation(&mut self, g: &mut Game, p: i32, owner: i32) -> Result<i32, String> {
        let r = relation_state(g, p, owner)?;
        if !self.relations.iter().any(|&(o, _)| o == owner) {
            self.relations.push((owner, r));
        }
        Ok(r)
    }
    fn embark(&mut self, g: &mut Game, t: &Tables, p: i32) -> Result<bool, String> {
        let e = can_embark(g, t, p)?;
        self.embark = Some(e);
        Ok(e)
    }
}

/// The most relation records a kept search holds: one per other owner, and the searches are
/// kept only with at most seven players (`keeps`).
const MAX_RELATIONS: usize = 8;

/// One kept route search (see `RouteSearches`). Its tiles read, the route marks it saw and
/// its path are in the update's word buffer from `at`: `route_seen` (the route marks found
/// earlier in the same update, on the tiles read), then `reads` (each `w` words), then the
/// path (`path_len` tile indices, destination first).
#[derive(Clone, Copy)]
struct RouteSearch {
    player: i32,
    origin: usize,
    dest: usize,
    allowed: u32,
    /// cleared when a tile it read changes key
    valid: bool,
    at: usize,
    path_len: usize,
    relations: [(i32, i32); MAX_RELATIONS],
    relation_count: u8,
    embark: Option<bool>,
    found: bool,
}

impl RouteSearch {
    fn route_seen<'a>(&self, words: &'a [u64], w: usize) -> &'a [u64] {
        &words[self.at..self.at + w]
    }
    fn reads<'a>(&self, words: &'a [u64], w: usize) -> &'a [u64] {
        &words[self.at + w..self.at + 2 * w]
    }
    fn path<'a>(&self, words: &'a [u64], w: usize) -> Option<&'a [u64]> {
        let from = self.at + 2 * w;
        self.found.then_some(&words[from..from + self.path_len])
    }

    /// Whether the search would read the same now. The relation records it read must all
    /// exist (a missing one would be inserted by the search, in its order), so reusing
    /// writes nothing; asking whether the player can embark refreshes a tech cache the
    /// update has just refreshed.
    #[allow(clippy::too_many_arguments)] // the search's inputs, as find_path_between has them
    fn still_holds(
        &self,
        words: &[u64],
        w: usize,
        g: &mut Game,
        t: &Tables,
        p: i32,
        allowed: u32,
        route: &[u64],
    ) -> Result<bool, String> {
        if !self.valid || self.allowed != allowed {
            return Ok(false);
        }
        let routes_same = route
            .iter()
            .zip(self.reads(words, w))
            .zip(self.route_seen(words, w))
            .all(|((r, m), seen)| r & m == *seen);
        if !routes_same {
            return Ok(false);
        }
        let me = g.player(p).ok_or("missing player")?;
        if !self.relations[..self.relation_count as usize]
            .iter()
            .all(|&(o, state)| me.relation(o).is_some_and(|r| r.state == state))
        {
            return Ok(false);
        }
        if let Some(e) = self.embark {
            if can_embark(g, t, p)? != e {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// What a route search reads of a tile.
#[derive(Clone, Copy, PartialEq)]
struct RouteKey {
    owner: i32,
    improvement: i32,
    terrain: i32,
    wetland: bool,
    road: bool,
    /// the players (by index) that explored it
    explorers: u8,
}

fn route_key(g: &Game, ids: &[i32], i: usize) -> RouteKey {
    let x = &g.tiles[i];
    let mut explorers = 0u8;
    for (k, id) in ids.iter().enumerate().take(8) {
        if x.explorers.contains(id) {
            explorers |= 1 << k;
        }
    }
    RouteKey {
        owner: x.owner,
        improvement: x.improvement.as_ref().map_or(-1, |m| m.kind),
        terrain: x.terrain,
        wetland: crate::aquarion::wetland(x),
        road: x.has_road,
        explorers,
    }
}

/// The route update's searches, one per (player, origin, destination) searched last time,
/// in the order searched. A search is reused when nothing it read changed: the keys of the
/// tiles it read (field stamps, then a key compare), the route marks found before it in the
/// same update on those tiles, the relation states it looked up, whether the player can
/// embark (if it asked), and its terrain mask.
///
/// During an update `searches` holds last update's searches (`last` of them, taken as this
/// one reaches them) and then this update's, and `words` their words in the same order; the
/// end of the update drops last update's part of both. So nothing here allocates once the two
/// have grown to what any update needed.
#[derive(Clone, Default)]
pub(crate) struct RouteSearches {
    clock: u64,
    keys: Vec<RouteKey>,
    ids: Vec<i32>,
    searches: Vec<Option<RouteSearch>>,
    last: usize,
    cursor: usize,
    /// the searches' words (see `RouteSearch`): last update's `words_last`, then this one's
    words: Vec<u64>,
    words_last: usize,
    /// what the search being made read, in buffers kept between searches
    scratch: Reads,
    reused: u64,
    searched: u64,
}

impl RouteSearches {
    /// bring the keys up to the board's clock; start this update's list
    fn begin(&mut self, g: &Game) -> Result<(), String> {
        let n = g.tiles.len();
        let w = n.div_ceil(64);
        // what the last update made becomes last update's part
        (self.last, self.words_last) = (self.searches.len(), self.words.len());
        let same_ids = g
            .players
            .iter()
            .map(|p| p.action.id)
            .eq(self.ids.iter().copied());
        if self.keys.len() != n || !same_ids {
            self.ids.clear();
            self.ids.extend(g.players.iter().map(|p| p.action.id));
            self.keys = (0..n).map(|i| route_key(g, &self.ids, i)).collect();
            self.searches.clear();
            self.words.clear();
            (self.last, self.words_last) = (0, 0);
        } else {
            for i in 0..n {
                if g.tiles.fields_stamp(i) <= self.clock {
                    continue;
                }
                let k = route_key(g, &self.ids, i);
                if k != self.keys[i] {
                    self.keys[i] = k;
                    for s in self.searches.iter_mut().flatten() {
                        s.valid &= !bit(s.reads(&self.words, w), i);
                    }
                }
            }
        }
        self.clock = g.tiles.clock();
        self.cursor = 0;
        if crate::shadow::on() && (0..n).any(|i| route_key(g, &self.ids, i) != self.keys[i]) {
            return Err(crate::shadow::differs("routes", "unmarked key change"));
        }
        Ok(())
    }

    /// last update's search of this pair, if any
    fn take(&mut self, p: i32, origin: usize, dest: usize) -> Option<RouteSearch> {
        let is = |s: &Option<RouteSearch>| {
            s.as_ref()
                .is_some_and(|s| s.player == p && s.origin == origin && s.dest == dest)
        };
        let last = &mut self.searches[..self.last];
        let k = if last.get(self.cursor).is_some_and(is) {
            self.cursor
        } else {
            last.iter().position(is)?
        };
        self.cursor = k + 1;
        last[k].take()
    }
}

/// UpdateRoutes over every non-Nature player
pub fn update_routes(g: &mut Game, t: &Tables) -> Result<(), String> {
    let mut a = ARRAYS.take();
    let done = if keeps(g) {
        let mut kept = g.tiles.take_kept();
        let done = update_routes_with(g, t, &mut a, &mut kept.routes);
        if done.is_err() {
            // an update cut short leaves the two parts mixed: keep nothing but the counts
            let r = &kept.routes;
            let (reused, searched) = (r.reused, r.searched);
            kept.routes = RouteSearches {
                reused,
                searched,
                ..Default::default()
            };
        }
        g.tiles.put_kept(kept);
        done
    } else {
        // nothing kept: every search runs in full
        update_routes_with(g, t, &mut a, &mut RouteSearches::default())
    };
    ARRAYS.set(a);
    done
}

fn update_routes_with(
    g: &mut Game,
    t: &Tables,
    a: &mut Arrays,
    kept: &mut RouteSearches,
) -> Result<(), String> {
    kept.begin(g)?;
    // every route goes, and the search finds them again (reading what it has found so far)
    reset(&mut a.route, g.tiles.len(), false);
    reset(&mut a.route_bits, g.tiles.len().div_ceil(64), 0);
    let (mut routers, mut cities) = (
        std::mem::take(&mut a.routers),
        std::mem::take(&mut a.cities),
    );
    for &p in g.player_ids().iter().filter(|&&p| p != 255) {
        let moves = movements(g, t, p)?;
        routers.clear();
        cities.clear();
        for (i, x) in g.tiles.iter().enumerate().filter(|(_, x)| x.owner == p) {
            if let Some(m) = &x.improvement {
                if let Some(r) = router(t, m.kind)? {
                    if r.terrain != 0 {
                        routers.push(i);
                    }
                    if m.kind == CITY {
                        cities.push(i);
                    }
                }
            }
        }
        for (n, &origin) in routers.iter().enumerate() {
            let Some(r) = router(t, g.tiles[origin].improvement.as_ref().unwrap().kind)? else {
                continue;
            };
            for &dest in &routers[n + 1..] {
                find_path_between(g, t, p, origin, dest, moves, a, kept)?;
            }
            if r.connects_cities {
                for &dest in &cities {
                    find_path_between(g, t, p, origin, dest, moves, a, kept)?;
                }
            }
        }
    }
    // the game sets HadRoute on a tile that loses its route mark and clears it again at
    // the end: a tile that had a route ends without HadRoute and with the route found; only
    // a tile whose flags change is written
    for i in 0..g.tiles.len() {
        let (had, found) = (g.tiles[i].has_route, a.route[i]);
        if had != found || had && g.tiles[i].had_route {
            let x = g.tile_mut(i);
            x.has_route = found;
            if had {
                x.had_route = false;
            }
        }
    }
    (a.routers, a.cities) = (routers, cities);
    // the searches not reached this time are not needed again
    kept.searches.drain(..kept.last);
    kept.words.drain(..kept.words_last);
    (kept.last, kept.words_last) = (0, 0);
    // the next update holds these and its own at once: room for both, so that only an update
    // that keeps more than this one allocates
    kept.searches.reserve(kept.searches.len());
    kept.words.reserve(kept.words.len());
    Ok(())
}

fn connectable(g: &Game, t: &Tables, i: usize) -> Result<bool, String> {
    let x = &g.tiles[i];
    if x.has_road {
        return Ok(true);
    }
    if let Some(m) = &x.improvement {
        if let Some(r) = router(t, m.kind)? {
            return Ok(m.kind == CITY || r.terrain != 0);
        }
    }
    Ok(false)
}

/// A connection does not pass onto another player's tile unless the two are at peace.
fn owners_allow(g: &Game, from: usize, to: usize) -> bool {
    let (fo, to_owner) = (g.tiles[from].owner, g.tiles[to].owner);
    to_owner == 0
        || to_owner == fo
        || g.player(fo).is_none()
        || crate::menu::relation(g, fo, to_owner) == PEACE
}

/// Directional transport-path check for many edges of one board: whether a tile
/// can carry a connection is worked out once per tile.
pub(crate) struct Links {
    /// per tile: 0 not asked yet, 1 not connectable, 2 connectable
    connectable: Vec<u8>,
}

impl Links {
    pub(crate) fn new(tiles: usize) -> Links {
        Links {
            connectable: vec![0; tiles],
        }
    }

    fn connectable(&mut self, g: &Game, t: &Tables, i: usize) -> Result<bool, String> {
        if self.connectable[i] == 0 {
            self.connectable[i] = 1 + u8::from(connectable(g, t, i)?);
        }
        Ok(self.connectable[i] == 2)
    }

    /// whether a connection is drawn from tile `from` to its neighbour `to`
    pub(crate) fn open(
        &mut self,
        g: &Game,
        t: &Tables,
        from: usize,
        to: usize,
    ) -> Result<bool, String> {
        if !owners_allow(g, from, to) {
            return Ok(false);
        }
        if self.connectable(g, t, from)? && self.connectable(g, t, to)? {
            return Ok(true);
        }
        Ok(g.tiles[from].has_route && g.tiles[to].has_route)
    }
}

fn start_tile(g: &Game, p: i32) -> Result<Xy, String> {
    Ok(g.player(p)
        .ok_or_else(|| format!("missing player {p}"))?
        .start_tile)
}

/// The searches' working arrays. One set is kept per thread between calls, so a search
/// allocates nothing once they have grown to the board.
#[derive(Default)]
struct Arrays {
    // the connected-cities search: distance from the start, open neighbours, tiles by
    // distance, the two count rows of the bound, the replayed queue and its marks
    distance: Vec<u16>,
    lists: Vec<Open>,
    order: Vec<usize>,
    count: Vec<u32>,
    queued: Vec<u32>,
    /// the search's answer: which tiles it visited
    visited: Vec<bool>,
    waiting: Vec<bool>,
    queue: VecDeque<usize>,
    connectable: Vec<u8>,
    // the route path search: open list, cost and predecessor per tile, and the path found
    open: Vec<(usize, f32)>,
    costs: Vec<i32>,
    came_from: Vec<usize>,
    path: Vec<usize>,
    // the route update: one player's routers and cities, and the routes found so far (the
    // search reads them; the tiles are written once at the end)
    routers: Vec<usize>,
    cities: Vec<usize>,
    route: Vec<bool>,
    route_bits: Vec<u64>,
}

thread_local! {
    static ARRAYS: std::cell::Cell<Arrays> = std::cell::Cell::new(Arrays::default());
}

/// A cleared array of `len` copies of `value`, keeping its storage.
fn reset<T: Clone>(v: &mut Vec<T>, len: usize, value: T) {
    v.clear();
    v.resize(len, value);
}

/// The game's cap on dequeues in its connected-cities search.
const DEQUEUE_CAP: u32 = 10_000;

/// A tile's open neighbours in the game's order, and how many there are.
type Open = ([u16; 8], u8);

/// The tiles the game's search from `start` visits. `open(i)` lists the neighbours tile
/// `i` passes to (asked at most once per tile).
///
/// The game keeps a first-in-first-out queue in its connected-cities search, marks a tile
/// visited when it is dequeued, and on every dequeue queues each open neighbour that is not
/// yet visited, so a tile is queued once per time a neighbour was dequeued before it was
/// visited. It stops after `DEQUEUE_CAP` dequeues. When the queue runs dry first, it has
/// visited exactly the tiles reachable from `start`; `dequeues_at_most` decides that without
/// replaying the queue, and only otherwise is the queue replayed entry by entry.
fn visited_from(
    a: &mut Arrays,
    tiles: usize,
    start: usize,
    mut open: impl FnMut(usize) -> Result<Open, String>,
) -> Result<(), String> {
    const UNREACHED: u16 = u16::MAX;
    // breadth-first distance from the start, and each reached tile's open neighbours
    reset(&mut a.distance, tiles, UNREACHED);
    // only its length matters: every reached tile's list is written before it is read
    a.lists.resize(tiles, ([0; 8], 0));
    a.order.clear();
    a.order.push(start);
    a.distance[start] = 0;
    let mut next = 0;
    while let Some(&i) = a.order.get(next) {
        next += 1;
        a.lists[i] = open(i)?;
        let (list, len) = a.lists[i];
        for &n in &list[..usize::from(len)] {
            if a.distance[usize::from(n)] == UNREACHED {
                a.distance[usize::from(n)] = a.distance[i] + 1;
                a.order.push(usize::from(n));
            }
        }
    }
    if dequeues_at_most(a) <= DEQUEUE_CAP {
        a.visited.clear();
        a.visited.extend(a.distance.iter().map(|&d| d != UNREACHED));
    } else {
        replay(a, start);
    }
    Ok(())
}

/// An upper bound on the dequeues the game makes before its queue runs dry.
///
/// Call the entries queued while generation k - 1 is dequeued generation k (the start alone
/// is generation 0). A tile at distance d is first dequeued in generation d, so generation k
/// holds only tiles at distance k - 1 or k. Dequeuing an entry queues every open neighbour at
/// distance k + 1 (none of them is visited yet), and those neighbours at distance k that have
/// not come up yet in this generation; counting all of the latter gives the bound.
/// `order` is the reached tiles by distance.
fn dequeues_at_most(a: &mut Arrays) -> u32 {
    let Arrays {
        distance,
        lists,
        order,
        count,
        queued,
        ..
    } = a;
    reset(count, distance.len(), 0);
    reset(queued, distance.len(), 0);
    count[order[0]] = 1;
    let (mut total, mut from) = (1u32, 0);
    for generation in 0u16.. {
        // the tiles in this generation: distance `generation - 1` (from `from`) and `generation`
        let mut any = false;
        let mut i = from;
        while let Some(&tile) = order.get(i) {
            if distance[tile] > generation {
                break;
            }
            i += 1;
            if count[tile] == 0 {
                continue;
            }
            let (list, len) = lists[tile];
            for &n in &list[..usize::from(len)] {
                let n = usize::from(n);
                if distance[n] == generation || distance[n] == generation + 1 {
                    queued[n] = queued[n].saturating_add(count[tile]);
                    any = true;
                }
            }
        }
        if !any {
            break;
        }
        // the next generation's counts replace this one's
        while order.get(from).is_some_and(|&t| distance[t] < generation) {
            from += 1;
        }
        for &tile in &order[from..] {
            if distance[tile] > generation + 1 {
                break;
            }
            count[tile] = std::mem::take(&mut queued[tile]);
            total = total.saturating_add(count[tile]);
        }
        if total > DEQUEUE_CAP {
            break;
        }
    }
    total
}

/// The game's queue, entry by entry, up to its cap.
///
/// It also stops once no unvisited tile waits in the queue: a visited tile queued all its
/// unvisited neighbours on its first dequeue, so from then on every neighbour of a visited
/// tile is visited, and the dequeues left would visit nothing and queue nothing.
fn replay(a: &mut Arrays, start: usize) {
    let Arrays {
        lists,
        visited,
        waiting,
        queue,
        ..
    } = a;
    reset(visited, lists.len(), false);
    reset(waiting, lists.len(), false);
    waiting[start] = true;
    let mut unvisited_waiting = 1;
    queue.clear();
    queue.push_back(start);
    for _ in 0..DEQUEUE_CAP {
        let Some(i) = queue.pop_front() else { break };
        if !visited[i] {
            visited[i] = true;
            unvisited_waiting -= 1;
        }
        let (list, len) = lists[i];
        for &n in &list[..usize::from(len)] {
            let n = usize::from(n);
            if !visited[n] {
                queue.push_back(n);
                if !waiting[n] {
                    waiting[n] = true;
                    unvisited_waiting += 1;
                }
            }
        }
        if unvisited_waiting == 0 {
            break;
        }
    }
}

/// Connected cities: marks in `a.visited` the tiles the search from `p`'s capital
/// visits, and returns the capital's tile; None when the player has no capital to search from
fn connected(g: &Game, t: &Tables, p: i32, a: &mut Arrays) -> Result<Option<usize>, String> {
    if p == 255 || g.player(p).is_none() {
        return Ok(None);
    }
    let start = g.index(start_tile(g, p)?).ok_or("start tile")?;
    if g.tiles[start].owner != p {
        return Ok(None);
    }
    let mut links = Links {
        connectable: std::mem::take(&mut a.connectable),
    };
    reset(&mut links.connectable, g.tiles.len(), 0);
    let done = visited_from(a, g.tiles.len(), start, |i| {
        let c = g.tiles[i].coordinates;
        let (mut list, mut len) = ([0u16; 8], 0);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let Some(n) = g.index(Xy {
                    x: c.x + dx,
                    y: c.y + dy,
                }) else {
                    continue;
                };
                if links.open(g, t, i, n)? {
                    list[len] = n as u16;
                    len += 1;
                }
            }
        }
        Ok((list, len as u8))
    });
    a.connectable = links.connectable;
    done.map(|()| Some(start))
}

/// What the step's incremental passes keep between calls, inside the board (`board::Tiles`)
/// so that it travels with the stamps it is read against. It assumes one catalog (`Tables`) for the board's whole life, as every
/// caller has; a game stepped with another catalog must `Tiles::forget_kept` first.
#[derive(Clone, Default)]
pub struct Kept {
    connections: Connections,
    routes: RouteSearches,
}

impl Kept {
    /// (searches reused, searches run) by the connected-cities pass
    pub fn connection_counts(&self) -> (u64, u64) {
        (self.connections.reused, self.connections.searched)
    }
    /// (searches reused, searches run) by the route update
    pub fn route_counts(&self) -> (u64, u64) {
        (self.routes.reused, self.routes.searched)
    }
    /// (kept connection searches, kept route searches) now
    pub fn searches(&self) -> (usize, usize) {
        let valid = self.connections.searches.iter().filter(|s| s.valid).count();
        (valid, self.routes.searches.len())
    }
    /// the bytes it holds: its own size and its arrays' capacities
    pub fn bytes(&self) -> usize {
        use std::mem::size_of;
        fn vec<T>(v: &Vec<T>) -> usize {
            v.capacity() * size_of::<T>()
        }
        let c = &self.connections;
        let r = &self.routes;
        let search = |s: &Search| vec(&s.region) + vec(&s.visited);
        size_of::<Kept>()
            + vec(&c.keys)
            + vec(&c.ids)
            + vec(&c.searches)
            + c.searches.iter().map(search).sum::<usize>()
            + vec(&r.keys)
            + vec(&r.ids)
            + vec(&r.searches)
            + vec(&r.words)
            + vec(&r.scratch.tiles)
            + vec(&r.scratch.relations)
    }
}

/// The connected-cities searches kept per player. A search reads, of each tile it reaches and
/// of their neighbours, only the tile's `key` (owner, whether it is connectable, route mark),
/// the player's start tile, and the peace relation of each pair of owners it looked across
/// (from a reached tile to a neighbour of another owner). It is reused while none of those
/// changed.
#[derive(Clone, Default)]
struct Connections {
    /// the board clock up to which `keys` are current
    clock: u64,
    keys: Vec<u32>,
    /// the player ids in order, and the peace relation of every ordered pair of them
    ids: Vec<i32>,
    peace: u64,
    searches: Vec<Search>,
    reused: u64,
    searched: u64,
}

#[derive(Clone)]
struct Search {
    player: i32,
    start: usize,
    /// cleared when a tile of `region` changes key, or the peace of a pair in `pairs`
    valid: bool,
    /// the ordered pairs of player indices whose peace the search read, as bits of `peace`
    pairs: u64,
    /// the tiles the search reached and their neighbours: everything it read
    region: Vec<u64>,
    visited: Vec<u64>,
}

/// The kept searches index pairs of players in one 64-bit word; a game with more players
/// (out of scope: two to four seats and Nature) keeps nothing and runs every search in full.
fn keeps(g: &Game) -> bool {
    g.players.len() <= 7
}

fn bit(set: &[u64], i: usize) -> bool {
    set[i / 64] >> (i % 64) & 1 == 1
}

/// what the connected-cities search reads of tile `i`
fn connection_key(g: &Game, t: &Tables, i: usize) -> Result<u32, String> {
    let x = &g.tiles[i];
    Ok((x.owner as u32) << 2 | u32::from(connectable(g, t, i)?) << 1 | u32::from(x.has_route))
}

/// the peace relations between the players (in `g.players` order), as `owners_allow` reads
/// them
fn peace(g: &Game) -> u64 {
    let n = g.players.len();
    let mut mask = 0u64;
    for (k, a) in g.players.iter().enumerate() {
        for (m, b) in g.players.iter().enumerate() {
            if crate::menu::relation(g, a.action.id, b.action.id) == PEACE {
                mask |= 1 << (k * n + m);
            }
        }
    }
    mask
}

impl Connections {
    /// bring the keys up to the board's clock, dropping every search that read a tile whose
    /// key changed or the peace of a pair whose peace changed, and every search if the players
    /// changed
    fn refresh(&mut self, g: &Game, t: &Tables) -> Result<(), String> {
        let n = g.tiles.len();
        if self.keys.len() != n {
            self.keys = (0..n)
                .map(|i| connection_key(g, t, i))
                .collect::<Result<_, _>>()?;
            self.searches.clear();
        } else {
            for i in 0..n {
                if g.tiles.fields_stamp(i) <= self.clock {
                    continue;
                }
                let k = connection_key(g, t, i)?;
                if k != self.keys[i] {
                    self.keys[i] = k;
                    // an invalid slot may have no region yet (a player without a capital)
                    for s in self.searches.iter_mut().filter(|s| s.valid) {
                        s.valid = !bit(&s.region, i);
                    }
                }
            }
        }
        self.clock = g.tiles.clock();
        let mask = peace(g);
        if !g
            .players
            .iter()
            .map(|p| p.action.id)
            .eq(self.ids.iter().copied())
        {
            self.ids.clear();
            self.ids.extend(g.players.iter().map(|p| p.action.id));
            self.searches.clear();
        } else {
            for s in &mut self.searches {
                s.valid &= s.pairs & (mask ^ self.peace) == 0;
            }
        }
        self.peace = mask;
        Ok(())
    }
}

/// `connected`, reusing the player's last search when nothing it read has changed
fn connected_kept(
    g: &Game,
    t: &Tables,
    p: i32,
    a: &mut Arrays,
    kept: &mut Connections,
) -> Result<Option<usize>, String> {
    // no capital to search from (`connected` answers that at once), or too many players
    if p == 255 || g.player(p).is_none() || !keeps(g) {
        return connected(g, t, p, a);
    }
    kept.refresh(g, t)?;
    let start = g.index(start_tile(g, p)?).ok_or("start tile")?;
    let n = g.tiles.len();
    let slot = kept.searches.iter().position(|s| s.player == p);
    if let Some(k) = slot.filter(|&k| kept.searches[k].valid && kept.searches[k].start == start) {
        kept.reused += 1;
        let visited = &kept.searches[k].visited;
        if crate::shadow::on() {
            if kept
                .keys
                .iter()
                .enumerate()
                .any(|(i, &k)| connection_key(g, t, i).is_ok_and(|now| now != k))
            {
                return Err(crate::shadow::differs(
                    "city connections",
                    "unmarked key change",
                ));
            }
            let full = connected(g, t, p, a)?;
            if full != Some(start) || (0..n).any(|i| a.visited[i] != bit(visited, i)) {
                return Err(crate::shadow::differs("city connections", "kept search"));
            }
        }
        reset(&mut a.visited, n, false);
        for (i, v) in a.visited.iter_mut().enumerate() {
            *v = bit(visited, i);
        }
        return Ok(Some(start));
    }
    kept.searched += 1;
    let found = connected(g, t, p, a)?;
    // the player's slot, kept invalid until a search can be kept in it, so that a redone
    // search reuses its buffers
    let k = slot.unwrap_or_else(|| {
        kept.searches.push(Search {
            player: p,
            start,
            valid: false,
            pairs: 0,
            region: Vec::new(),
            visited: Vec::new(),
        });
        kept.searches.len() - 1
    });
    kept.searches[k].valid = false;
    if found.is_some() {
        // the reached tiles and their neighbours, and the owner pairs `owners_allow` read
        let mut region = std::mem::take(&mut kept.searches[k].region);
        region.clear();
        region.resize(n.div_ceil(64), 0);
        let mut pairs = Some(0u64);
        let index = |owner: i32| kept.ids.iter().position(|&id| id == owner);
        for i in (0..n).filter(|&i| a.distance[i] != u16::MAX) {
            let c = g.tiles[i].coordinates;
            let from = g.tiles[i].owner;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(m) = g.index(Xy {
                        x: c.x + dx,
                        y: c.y + dy,
                    }) else {
                        continue;
                    };
                    region[m / 64] |= 1 << (m % 64);
                    let to = g.tiles[m].owner;
                    if to == 0 || to == from || g.player(from).is_none() {
                        continue;
                    }
                    // an owner that is not a listed player has no bit: such a search is not kept
                    pairs = match (pairs, index(from), index(to)) {
                        (Some(b), Some(f), Some(o)) => Some(b | 1 << (f * kept.ids.len() + o)),
                        _ => None,
                    };
                }
            }
        }
        let s = &mut kept.searches[k];
        s.region = region;
        let Some(pairs) = pairs else {
            return Ok(found);
        };
        s.visited.clear();
        s.visited.resize(n.div_ceil(64), 0);
        for i in (0..n).filter(|&i| a.visited[i]) {
            s.visited[i / 64] |= 1 << (i % 64);
        }
        (s.start, s.pairs, s.valid) = (start, pairs, true);
    }
    Ok(found)
}

/// UpdateCityConnections: queue Connect/Change/Disconnect for each listed player's cities
pub fn update_city_connections(
    g: &mut Game,
    t: &Tables,
    players: &[i32],
    out: &mut Vec<Action>,
) -> Result<(), String> {
    for &p in players {
        if p == 0 {
            continue;
        }
        player_index(g, p)?;
        let mut a = ARRAYS.take();
        let mut kept = g.tiles.take_kept();
        let start = connected_kept(g, t, p, &mut a, &mut kept.connections);
        g.tiles.put_kept(kept);
        let start = start?;
        // the capital itself is not one of its connected cities
        let reached = |i: usize| start.is_some_and(|s| s != i && a.visited[i]);
        for (i, x) in g.tiles.iter().enumerate().filter(|(_, x)| x.owner == p) {
            let Some(city) = x.improvement.as_ref().filter(|m| m.kind == CITY) else {
                continue;
            };
            let to = city.connected_to_capital_of_player;
            let (player, coordinates) = (p, x.coordinates);
            let effect = if reached(i) {
                if to == 0 {
                    Some(CityConnection::Connect {
                        player,
                        coordinates,
                    })
                } else if to != p {
                    Some(CityConnection::Change {
                        player,
                        coordinates,
                    })
                } else {
                    None
                }
            } else if to != 0 {
                Some(CityConnection::Disconnect {
                    player,
                    coordinates,
                })
            } else {
                None
            };
            if let Some(e) = effect {
                out.push(Action::CityConnection(e));
            }
        }
        ARRAYS.set(a);
    }
    Ok(())
}

/// The CityConnection action: queues nothing when invalid
pub fn city_connection(
    g: &mut Game,
    t: &Tables,
    effect: &CityConnection,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let (p, c) = match *effect {
        CityConnection::Connect {
            player,
            coordinates,
        }
        | CityConnection::Change {
            player,
            coordinates,
        }
        | CityConnection::Disconnect {
            player,
            coordinates,
        } => (player, coordinates),
    };
    let i = g.index(c).ok_or("connection tile")?;
    let Some(city) = g.tiles[i].improvement.as_ref() else {
        return if matches!(effect, CityConnection::Change { .. }) {
            Err("connection without an improvement".into())
        } else {
            Ok(())
        };
    };
    let to = city.connected_to_capital_of_player;
    let valid = match effect {
        CityConnection::Connect { .. } => to != p,
        CityConnection::Disconnect { .. } => to != 0,
        CityConnection::Change { .. } => true,
    };
    if !valid {
        return Ok(());
    }
    let pop = |player: i32, source: Xy, target: Xy| {
        Action::City(City::IncreasePopulation {
            player,
            source,
            target,
            delay: 60,
        })
    };
    let depop = |player: i32, target: Xy| {
        Action::City(City::DecreasePopulation {
            player,
            target,
            delay: 60,
        })
    };
    let pi = player_index(g, p);
    match effect {
        CityConnection::Connect { .. } => {
            let start = start_tile(g, p)?;
            let i_p = pi?;
            if g.players[i_p].task(3).is_none() {
                out.push(Action::EnableTask {
                    player: p,
                    task: NETWORK_TASK as u32,
                });
            }
            out.push(pop(p, start, start));
            out.push(pop(p, c, c));
            g.tile_mut(i)
                .improvement
                .as_mut()
                .unwrap()
                .connected_to_capital_of_player = p;
            out.extend(task_notification(g, t, i_p, NETWORK_TASK)?);
        }
        CityConnection::Change { .. } => {
            let start = start_tile(g, p)?;
            let old = start_tile(g, to)?;
            out.push(pop(p, start, start));
            out.push(depop(p, old));
            g.tile_mut(i)
                .improvement
                .as_mut()
                .unwrap()
                .connected_to_capital_of_player = p;
            out.extend(task_notification(g, t, pi?, NETWORK_TASK)?);
        }
        CityConnection::Disconnect { .. } => {
            let old = start_tile(g, to)?;
            out.push(depop(p, old));
            out.push(depop(p, c));
            g.tile_mut(i)
                .improvement
                .as_mut()
                .unwrap()
                .connected_to_capital_of_player = 0;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player that has lost its capital gets a connected-cities slot with no region yet; a
    /// tile changing key after that must not read the empty region.
    #[test]
    fn a_player_without_a_capital_survives_a_key_change() {
        const CATALOG: &str = include_str!("../rules/rules-124.json");
        let t = Tables::compile(CATALOG).unwrap();
        let config = crate::reset::Config {
            rules: 124,
            seed: 3,
            size: 11,
            map: 2,
            mode: 4,
            tribes: vec![7, 7],
            action_limit: 10,
        };
        let mut g = crate::reset::reset(&config, CATALOG, &t).unwrap();
        let ids: Vec<i32> = g
            .players
            .iter()
            .map(|p| p.action.id)
            .filter(|&p| p != 255)
            .collect();
        let (p, other) = (ids[0], ids[1]);
        let start = g.index(start_tile(&g, p).unwrap()).unwrap();
        g.tile_mut(start).owner = other;
        let (mut a, mut kept) = (Arrays::default(), Connections::default());
        assert_eq!(connected_kept(&g, &t, p, &mut a, &mut kept).unwrap(), None);
        let far = (0..g.tiles.len())
            .find(|&i| i != start && g.tiles[i].owner == 0)
            .unwrap();
        g.tile_mut(far).owner = other;
        assert_eq!(connected_kept(&g, &t, p, &mut a, &mut kept).unwrap(), None);
    }

    /// The game's search as written: a queue, visited on dequeue, capped.
    fn reference(start: usize, lists: &[Open]) -> Vec<bool> {
        let mut visited = vec![false; lists.len()];
        let mut queue = VecDeque::from([start]);
        let mut remaining = DEQUEUE_CAP;
        while let Some(i) = queue.pop_front() {
            if remaining == 0 {
                break;
            }
            remaining -= 1;
            visited[i] = true;
            let (list, len) = lists[i];
            for &n in &list[..usize::from(len)] {
                if !visited[usize::from(n)] {
                    queue.push_back(usize::from(n));
                }
            }
        }
        visited
    }

    /// How many dequeues the game makes with no cap, up to `limit`.
    fn dequeues(start: usize, lists: &[Open], limit: u32) -> u32 {
        let mut visited = vec![false; lists.len()];
        let mut queue = VecDeque::from([start]);
        let mut n = 0;
        while let Some(i) = queue.pop_front() {
            n += 1;
            if n == limit {
                break;
            }
            visited[i] = true;
            let (list, len) = lists[i];
            for &m in &list[..usize::from(len)] {
                if !visited[usize::from(m)] {
                    queue.push_back(usize::from(m));
                }
            }
        }
        n
    }

    /// A board of `side` by `side` tiles where each directed edge to a neighbour is open with
    /// probability `density` in 256.
    fn board(side: usize, density: u64, seed: u64) -> Vec<Open> {
        let mut rng = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut lists = vec![([0u16; 8], 0u8); side * side];
        for (i, slot) in lists.iter_mut().enumerate() {
            let (x, y) = ((i % side) as i32, (i / side) as i32);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    let on = nx >= 0 && ny >= 0 && nx < side as i32 && ny < side as i32;
                    if (dx, dy) == (0, 0) || !on {
                        continue;
                    }
                    rng = rng
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    if (rng >> 33) % 256 < density {
                        slot.0[usize::from(slot.1)] = (ny as usize * side + nx as usize) as u16;
                        slot.1 += 1;
                    }
                }
            }
        }
        lists
    }

    #[test]
    fn visits_what_the_game_queue_visits() {
        let (mut shortcut, mut replayed, mut capped) = (0, 0, 0);
        // one set of arrays for every board, as one thread keeps them: what an earlier, larger
        // board left in them must not matter
        let mut a = Arrays::default();
        for seed in (0..600u64).rev() {
            let side = 4 + (seed % 8) as usize;
            let density = [40, 70, 100, 140, 200, 256][(seed % 6) as usize];
            let lists = board(side, density, seed);
            let start = (seed as usize * 7) % lists.len();
            visited_from(&mut a, lists.len(), start, |i| Ok(lists[i])).unwrap();
            assert_eq!(a.visited, reference(start, &lists), "seed {seed}");
            // the bound is never under the true count, and the shortcut is taken only under the cap
            let bound = dequeues_at_most(&mut a);
            let actual = dequeues(start, &lists, DEQUEUE_CAP + 1);
            assert!(
                bound >= actual.min(DEQUEUE_CAP + 1),
                "seed {seed}: {bound} < {actual}"
            );
            if bound <= DEQUEUE_CAP {
                shortcut += 1;
            } else {
                replayed += 1;
                capped += u32::from(actual > DEQUEUE_CAP);
            }
        }
        // the three cases all occur: the shortcut, a replay, and a replay the cap cuts short
        assert!(
            shortcut > 50 && replayed > 50 && capped > 20,
            "{shortcut} {replayed} {capped}"
        );
    }
}
