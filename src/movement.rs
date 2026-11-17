//! Move candidates: the game move-option search,
//! each target then kept only if the validator's path search finds a
//! path within the unit's movement, plus the embark detail.
//!
//! Exact-or-refuse: what is not ported yet (linked units, mangrove terrain) returns Err instead of
//! being modelled.

use crate::action::Path;
use crate::menu::{effects as tile_effects, relation, unit_effects};
use crate::state::{Game, Tile, Unit, Xy};
use crate::state::{COOL_DOWN, PEACE};
use crate::tables::{terrain_bit, Tables};

pub const MOVE: u64 = 6;

const WATER: i32 = 1;
const OCEAN: i32 = 2;
const FIELD: i32 = 3;
const MOUNTAIN: i32 = 4;
const FOREST: i32 = 5;
const ICE: i32 = 6;
const WETLAND: i32 = 7;
const CITY: i32 = 1;
const BRIDGE: i32 = 48;
const FROZEN: i64 = 0;
const POISONED: i64 = 1;
const BOOSTED: i64 = 2;
const INVISIBLE: i64 = 3;
const BUBBLE: i64 = 4;
const SWIFT: i64 = 6;
/// what step costs saturate to when a tile ends movement (embark, zone of control, rough terrain)
const STOP: i32 = 1000;

fn wetland(t: &Tile) -> bool {
    crate::aquarion::wetland(t)
}

fn is_water(t: &Tile) -> bool {
    t.terrain == WATER || t.terrain == OCEAN
}

/// The per-unit search profile.
struct Profile<'a> {
    g: &'a Game,
    t: &'a Tables,
    unit: &'a Unit,
    owner: i32,
    /// enemy units next to a tile do not end this unit's movement there (sneak, hide, invisible)
    slips_past: bool,
    hide: bool,
    creep: bool,
    aquatic: bool,
    /// Fly: every terrain, no embarking, no port needed, a flat step cost after zone of control
    fly: bool,
    /// Skate: a step costs 10 onto ice and ends movement anywhere else
    skate: bool,
    /// a swimmer that carries or has its own terrain list: a flooded
    /// tile ends its movement
    water_bound: bool,
    carry: bool,
    infiltrate: bool,
    /// the terrain kinds the unit may enter, one bit per terrain id
    allowed: u32,
    required_port: bool,
    /// the battle-advance profile lets the path end on (and pass) occupied tiles
    allow_occupied: bool,
    /// what the owner's units share, with the searches' per-tile arrays
    mover: &'a Mover,
}

/// A box of tiles (inclusive coordinates; empty while `x0 > x1`): what a unit's legal rows
/// read, so that the kept list (`menu_cache`) redoes them only when a tile in it changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reads {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Reads {
    pub const NONE: Reads = Reads {
        x0: i32::MAX,
        y0: i32::MAX,
        x1: i32::MIN,
        y1: i32::MIN,
    };
    /// add the tiles within `r` of `c`
    pub fn add(&mut self, c: Xy, r: i32) {
        self.x0 = self.x0.min(c.x - r);
        self.y0 = self.y0.min(c.y - r);
        self.x1 = self.x1.max(c.x + r);
        self.y1 = self.y1.max(c.y + r);
    }
}

/// What a player's units share in a search (its techs' movement and embark unlocks).
pub(crate) struct Mover {
    /// terrain kinds unlocked by tech, one bit per terrain id
    movements: u32,
    can_embark: bool,
    glide: bool,
    /// the path search's arrays: cost and predecessor per tile, and the open list
    scratch: std::cell::RefCell<Scratch>,
    /// per tile, whether an enemy unit beside it ends movement there (the same for every unit
    /// of the player): 0 not asked yet, 1 no, 2 yes
    guarded: std::cell::RefCell<Vec<u8>>,
}

type Scratch = (Vec<i32>, Vec<Xy>, Vec<(Xy, f32)>);

thread_local! {
    /// The arrays of the last search context dropped on this thread, for the next one: a
    /// search allocates nothing once they have grown to the board.
    static SPARE: std::cell::Cell<Option<(Scratch, Vec<u8>)>> = const { std::cell::Cell::new(None) };
}

impl Drop for Mover {
    fn drop(&mut self) {
        let arrays = (
            std::mem::take(self.scratch.get_mut()),
            std::mem::take(self.guarded.get_mut()),
        );
        SPARE.set(Some(arrays));
    }
}

impl Profile<'_> {
    fn rel(&self, p: i32, q: i32) -> i32 {
        relation(self.g, p, q)
    }
    fn allows(&self, terrain: i32) -> bool {
        self.allowed & terrain_bit(terrain) != 0
    }
    fn improvement_has(&self, t: &Tile, a: &str) -> bool {
        t.improvement
            .as_ref()
            .and_then(|m| self.t.improvements.get(m.kind as usize))
            .is_some_and(|d| d.improvement_abilities.has(a))
    }
    fn occupant(&self, t: &Tile) -> Option<&Unit> {
        t.unit.and_then(|n| self.g.units.get(n as usize))
    }
    /// The unit's visible-unit lookup for its owner
    fn visible(&self, t: &Tile) -> Option<&Unit> {
        if !t.explorers.contains(&self.owner) {
            return None;
        }
        let u = self.occupant(t)?;
        let hidden = unit_effects(self.g, u).contains(&INVISIBLE)
            && u.owner != self.owner
            && self.rel(self.owner, u.owner) != PEACE;
        (!hidden).then_some(u)
    }
    fn tile(&self, xy: Xy) -> Option<&Tile> {
        self.g.tile(xy)
    }
    fn neighbours(&self, c: Xy) -> impl Iterator<Item = &Tile> {
        (-1..=1)
            .flat_map(move |dx| (-1..=1).map(move |dy| (dx, dy)))
            .filter(|&d| d != (0, 0))
            .filter_map(move |(dx, dy)| {
                self.tile(Xy {
                    x: c.x + dx,
                    y: c.y + dy,
                })
            })
    }

    /// Disembarking: a vehicle onto a city or dry land
    fn disembarks(&self, target: &Tile) -> bool {
        self.carry
            && (target.improvement.as_ref().is_some_and(|m| m.kind == CITY)
                || [FIELD, MOUNTAIN, FOREST, ICE].contains(&target.terrain) && !wetland(target))
    }

    /// Embarking: a land unit onto an own or peaceful port
    fn embarks(&self, target: &Tile) -> bool {
        if self.aquatic
            || self.fly
            || self.unit.leader != 0
            || self.unit.follower != 0
            || !(is_water(target) || wetland(target))
            || !self.improvement_has(target, "embark")
            || !self.mover.can_embark
        {
            return false;
        }
        target.owner == self.owner
            || target.owner != 0 && self.rel(self.owner, target.owner) == PEACE
    }

    fn can_finish(&self, t: &Tile) -> bool {
        if self.visible(t).is_some() && !self.allow_occupied {
            return false;
        }
        if t.improvement.as_ref().is_some_and(|m| m.kind == CITY) {
            if self.infiltrate && t.owner != 0 && t.owner != self.owner {
                return false;
            }
            let r = self.rel(self.owner, t.owner);
            if t.owner != 0 && (r == PEACE || r == COOL_DOWN) {
                return false;
            }
        }
        true
    }

    /// Tile accessibility (unit profile, not allowing occupied tiles)
    fn accessible(&self, origin: &Tile, target: &Tile) -> bool {
        let owned_or_peace = target.owner == self.owner
            || target.owner != 0 && self.rel(self.owner, target.owner) == PEACE;
        if !target.explorers.contains(&self.owner) {
            return false;
        }
        if let Some(o) = self.visible(target) {
            if !self.allow_occupied
                && o.owner != self.owner
                && !(o.owner != 0 && self.rel(self.owner, o.owner) == PEACE)
                && !self.hide
            {
                return false;
            }
        }
        if self.fly {
            return true;
        }
        if self.improvement_has(target, "bridge") {
            if target
                .improvement
                .as_ref()
                .is_some_and(|m| m.kind == BRIDGE)
                && !self.allows(target.terrain)
            {
                let c = target.coordinates;
                let anchor = |dx: i32, dy: i32| {
                    self.tile(Xy {
                        x: c.x + dx,
                        y: c.y + dy,
                    })
                    .is_some_and(|n| !is_water(n))
                };
                let (horizontal, vertical) = if anchor(0, -1) && anchor(0, 1) {
                    (true, false)
                } else if anchor(-1, 0) && anchor(1, 0) {
                    (false, true)
                } else {
                    (false, false)
                };
                let (dx, dy) = (origin.coordinates.x - c.x, origin.coordinates.y - c.y);
                if horizontal && dy == 0 && dx != 0 || vertical && dx == 0 && dy != 0 {
                    return false;
                }
            }
            return true;
        }
        let wet = wetland(target) && self.allows(WETLAND);
        if is_water(target) {
            if wet {
                return true;
            }
            if self.required_port
                && (!self.improvement_has(target, "embark")
                    || !owned_or_peace
                    || !self.mover.can_embark
                    || self.unit.leader != 0
                    || self.unit.follower != 0)
            {
                return false;
            }
        }
        target.improvement.as_ref().is_some_and(|m| m.kind == CITY)
            || self.allows(target.terrain)
            || wet
    }

    /// Whether a visible enemy unit that is not at peace stands beside `target` (the same answer
    /// from every origin, so it is worked out once per tile and search profile).
    fn guarded(&self, target: &Tile) -> bool {
        let w = self.g.width;
        let i = (target.coordinates.y * w + target.coordinates.x) as usize;
        let known = self.mover.guarded.borrow()[i];
        if known != 0 {
            return known == 2;
        }
        let found = self.neighbours(target.coordinates).any(|n| {
            self.occupant(n).is_some_and(|e| {
                e.owner != self.owner
                    && self.rel(self.owner, e.owner) != PEACE
                    && !unit_effects(self.g, e).contains(&INVISIBLE)
                    && e.leader == 0
            })
        });
        self.mover.guarded.borrow_mut()[i] = 1 + u8::from(found);
        found
    }

    /// Movement cost (rules 115+ unit branch), in tenths of a move
    fn step_cost(&self, target: &Tile, from: &Tile) -> i32 {
        if self.embarks(target) || self.disembarks(target) {
            return STOP;
        }
        if !self.slips_past && self.guarded(target) {
            return STOP;
        }
        if self.fly {
            return 10;
        }
        if self.skate {
            return if target.terrain == ICE { 10 } else { STOP };
        }
        if target.terrain == ICE && self.mover.glide {
            return 9;
        }
        let creep = self.creep;
        if is_water(target) {
            if creep {
                return 10;
            }
        } else {
            if self.aquatic && !crate::aquarion::flooded(target) {
                return STOP;
            }
            if target.terrain != MOUNTAIN && creep {
                return 10;
            }
        }
        let road = |t: &Tile| {
            t.has_road
                || t.improvement
                    .as_ref()
                    .is_some_and(|m| m.kind == CITY || m.kind == BRIDGE)
        };
        if road(from) && road(target) && target.terrain != ICE && !creep && !self.aquatic {
            let foreign_ok = |t: &Tile| {
                t.owner == 0
                    || t.owner == self.owner
                    || self.g.player(t.owner).is_some() && self.rel(t.owner, self.owner) == PEACE
            };
            // the road check queries both tiles (permitted &=), so neither lookup is skipped
            let (a, b) = (foreign_ok(from), foreign_ok(target));
            if a && b {
                return 5;
            }
        }
        if self.improvement_has(target, "slow") {
            return STOP;
        }
        if target.terrain == MOUNTAIN || target.terrain == FOREST {
            return STOP;
        }
        if self.water_bound && crate::aquarion::flooded(target) {
            return STOP;
        }
        if !self.aquatic && tile_effects(target).contains(&crate::aquarion::ALGAE) {
            return STOP;
        }
        10
    }

    /// `accessible` between tile indices
    fn edge_accessible(&self, from: usize, to: usize) -> bool {
        self.accessible(&self.g.tiles[from], &self.g.tiles[to])
    }
    /// `step_cost` onto `to` from `from`
    fn edge_cost(&self, to: usize, from: usize) -> i32 {
        self.step_cost(&self.g.tiles[to], &self.g.tiles[from])
    }

    fn index(&self, c: Xy) -> Option<usize> {
        let w = self.g.width;
        let h = self.g.tiles.len() as i32 / w;
        (c.x >= 0 && c.y >= 0 && c.x < w && c.y < h).then(|| (c.y * w + c.x) as usize)
    }
    /// the neighbours of tile `from` (at `c`) the unit can step to, in the game's order
    fn open_neighbours(&self, from: usize, c: Xy) -> ([usize; 8], usize) {
        let (mut out, mut n) = ([0; 8], 0);
        if self.allowed == 0 {
            return (out, 0);
        }
        for dx in -1..=1 {
            for dy in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let next = self.index(Xy {
                    x: c.x + dx,
                    y: c.y + dy,
                });
                if let Some(next) = next.filter(|&next| self.edge_accessible(from, next)) {
                    out[n] = next;
                    n += 1;
                }
            }
        }
        (out, n)
    }

    /// Move options: FIFO relaxation; `found` gets each finishable tile on first discovery.
    /// `reads` gets every tile the search looks at: an expanded tile's neighbours are checked
    /// for access and cost, which read their own neighbours (bridge anchors, zone of control),
    /// so 2 around each expanded tile.
    fn options(&self, range: i32, reads: &mut Reads, mut found: impl FnMut(&Tile)) {
        let tiles = &self.g.tiles;
        let first = self.index(self.unit.coordinates).unwrap();
        let mut scratch = self.mover.scratch.borrow_mut();
        // cost so far per tile (-1: not reached), and the queue of tiles to expand
        let (costs, _, _) = &mut *scratch;
        costs.clear();
        costs.resize(tiles.len(), -1);
        costs[first] = 0;
        let mut queue = [0u16; 1000];
        let (mut head, mut tail) = (0, 1);
        queue[0] = first as u16;
        let limit = range * 10;
        // the game stops after 1 000 expansions
        while head < tail {
            let cur = usize::from(queue[head]);
            head += 1;
            reads.add(tiles[cur].coordinates, 2);
            let cost = costs[cur];
            let (valid, n) = self.open_neighbours(cur, tiles[cur].coordinates);
            for &next in &valid[..n] {
                let candidate = cost + self.edge_cost(next, cur);
                let old = costs[next];
                if old >= 0 && old <= candidate {
                    continue;
                }
                costs[next] = candidate;
                if old < 0 && self.can_finish(&tiles[next]) {
                    found(&tiles[next]);
                }
                if candidate < limit && tail < queue.len() {
                    queue[tail] = next as u16;
                    tail += 1;
                }
            }
        }
    }

    /// The path from the unit to `destination`, destination first and the unit's tile
    /// last, or None
    fn path(&self, destination: Xy, max_cost: i32) -> Option<Path> {
        let tiles = &self.g.tiles;
        let start = self.unit.coordinates;
        let dest = self.index(destination)?;
        if start == destination {
            return Some(Path::from_iter([destination]));
        }
        if !self.can_finish(&tiles[dest]) {
            return None;
        }
        let manhattan = (destination.x - start.x).abs() + (destination.y - start.y).abs();
        let mut scratch = self.mover.scratch.borrow_mut();
        let (cost, came_from, open) = &mut *scratch;
        open.clear();
        open.push((start, manhattan as f32));
        cost.clear();
        cost.resize(tiles.len(), 0);
        came_from.clear();
        came_from.resize(tiles.len(), Xy { x: 0, y: 0 });
        let limit = (max_cost * 10) as f32;
        for _ in 0..1000 {
            if open.is_empty() {
                return None;
            }
            let cur = open.remove(0).0;
            if cur == destination {
                let mut path = Path::from_iter([destination]);
                let mut c = destination;
                for _ in 0..=tiles.len() {
                    c = came_from[self.index(c)?];
                    path.push(c);
                    if c == start {
                        return Some(path);
                    }
                }
                return None;
            }
            let ci = self.index(cur).unwrap();
            let cc = cost[ci];
            if (cc as f32) < limit {
                let (valid, n) = self.open_neighbours(ci, cur);
                for &next in &valid[..n] {
                    let nc = cc + self.edge_cost(next, ci);
                    if cost[next] < 1 || nc < cost[next] {
                        cost[next] = nc;
                        came_from[next] = cur;
                        let c = tiles[next].coordinates;
                        let (dx, dy) = ((destination.x - c.x) as f64, (destination.y - c.y) as f64);
                        let priority = (dx * dx + dy * dy).sqrt() as f32 + nc as f32;
                        let at = open
                            .iter()
                            .position(|&(_, p)| p > priority)
                            .unwrap_or(open.len());
                        open.insert(at, (c, priority));
                    }
                }
            }
        }
        None
    }
}

/// The move candidates of the mover's unit on tile `i` alone (see `moves_at`).
pub fn moves_on(
    g: &Game,
    t: &Tables,
    mover: i32,
    i: usize,
) -> Result<Vec<crate::menu::Cand>, String> {
    let mut out = Vec::new();
    let mut reads = Reads::NONE;
    moves_at(g, t, mover, i, &mut None, &mut reads, &mut out)?;
    Ok(out)
}

/// Appends the move candidates for the mover's unit on tile `i` to `out`, in the game's target order,
/// each with its embark flag. `shared` carries the mover's search context from one tile to the next.
/// `mover` must be the current player with no pending trigger (as `menu` guarantees): the validator's
/// current-player and trigger checks are not repeated here. `reads` gets the tiles the search
/// read beyond the unit's own tile.
pub(crate) fn moves_at(
    g: &Game,
    t: &Tables,
    mover: i32,
    i: usize,
    shared: &mut Option<Mover>,
    reads: &mut Reads,
    out: &mut Vec<crate::menu::Cand>,
) -> Result<(), String> {
    let tile = &g.tiles[i];
    let Some(node) = tile.unit else {
        return Ok(());
    };
    let unit = g.units.get(node as usize).ok_or("unit node out of range")?;
    if unit.owner != mover || unit.moved || unit_effects(g, unit).contains(&FROZEN) {
        return Ok(());
    }
    let mut _t = engine_fast_frame::timers::scope("menu.moves: profile");
    if shared.is_none() {
        *shared = Some(mover_context(g, t, mover)?);
    }
    let (profile, mut range) = profile_with(g, t, unit, shared.as_ref().unwrap())?;
    if range == 0 {
        return Ok(());
    }
    // a chain moves at its follower's pace, and only its head moves
    if unit.follower != 0 {
        if let Some(f) = crate::linked::find(g, unit.follower) {
            let follower = &g.units[f as usize];
            range = range.min(profile_with(g, t, follower, shared.as_ref().unwrap())?.1);
        }
    }
    if unit.leader != 0 || range == 0 {
        return Ok(());
    }
    _t = engine_fast_frame::timers::scope("menu.moves: options");
    // The game's command validator searches a path to each of these destinations again.
    // That search and this flood fill use the same edges, costs and budget, so they reach
    // the same tiles unless a search hits its cap of 1 000 steps; over 1 000 games it
    // rejected none of 9.5 million destinations and peaked at 75 steps.
    profile.options(range, reads, |target| {
        let embark = is_water(target) && profile.embarks(target);
        let mut c = crate::menu::cand(MOVE)
            .source(tile.coordinates)
            .target(target.coordinates);
        c.details.embark = Some(embark);
        out.push(c);
    });
    Ok(())
}

/// The game unit path for a Move command or action, destination first, or None.
pub(crate) fn unit_path(g: &Game, t: &Tables, unit: &Unit, to: Xy) -> Result<Option<Path>, String> {
    let mover = mover_context(g, t, unit.owner)?;
    let (profile, range) = profile_with(g, t, unit, &mover)?;
    if range == 0 {
        return Ok(None);
    }
    Ok(profile.path(to, range))
}

/// A unit path query with an explicit movement budget (the push step)
pub(crate) fn unit_path_budget(
    g: &Game,
    t: &Tables,
    unit: &Unit,
    to: Xy,
    budget: i32,
) -> Result<Option<Path>, String> {
    let mover = mover_context(g, t, unit.owner)?;
    let (profile, _) = profile_with(g, t, unit, &mover)?;
    Ok(profile.path(to, budget))
}

/// Placement accessibility: tile accessibility with the candidate as both origin and target
pub(crate) fn placement_accessible(
    g: &Game,
    t: &Tables,
    unit: &Unit,
    c: Xy,
) -> Result<bool, String> {
    let mover = mover_context(g, t, unit.owner)?;
    let (profile, _) = profile_with(g, t, unit, &mover)?;
    let tile = g.tile(c).ok_or("placement tile")?;
    Ok(profile.accessible(tile, tile))
}

/// Whether a path within the unit's movement reaches `to` when occupied
/// tiles are allowed
pub(crate) fn can_advance(g: &Game, t: &Tables, unit: &Unit, to: Xy) -> Result<bool, String> {
    let mover = mover_context(g, t, unit.owner)?;
    let (mut profile, range) = profile_with(g, t, unit, &mover)?;
    if range == 0 {
        return Ok(false);
    }
    profile.allow_occupied = true;
    Ok(profile.path(to, range).is_some())
}

/// The part of a search profile that is the same for every unit of `owner`. A board with
/// mangrove, which the search does not model, never gets here (`Game::import` refuses it).
pub(crate) fn mover_context(g: &Game, t: &Tables, owner: i32) -> Result<Mover, String> {
    let p = g.player(owner).ok_or("missing unit owner")?;
    // player movements: tech movementUnlocks, Water added when only Ocean is unlocked
    let movements = t.movements(&p.available_tech)?;
    let techs = || {
        p.available_tech
            .iter()
            .filter_map(|&k| t.techs.get(k as usize))
    };
    let mut can_embark = false;
    let tree = t.tribe_tree(p.tribe)?;
    for &k in &p.available_tech {
        can_embark |= tree.improvements_of(k)?.iter().any(|&id| {
            let d = t.improvements.get(id).unwrap();
            !d.hidden && d.improvement_abilities.has("embark")
        });
    }
    let (scratch, mut guarded) = SPARE.take().unwrap_or_default();
    guarded.clear();
    guarded.resize(g.tiles.len(), 0);
    Ok(Mover {
        movements,
        can_embark,
        glide: techs().any(|x| x.ability_unlocks.has("glide")),
        scratch: scratch.into(),
        guarded: guarded.into(),
    })
}

/// The unit's movement with its effects (Boosted, Swift or Bubble add one,
/// Poisoned takes one above 1). A search spends at least 5 tenths a step (a road), so it reaches
/// no tile more than twice this from the unit.
pub(crate) fn movement_range(g: &Game, t: &Tables, unit: &Unit) -> Result<i32, String> {
    let fx = unit_effects(g, unit);
    let data = t.units.get(unit.kind as usize).ok_or("unknown unit kind")?;
    let mut range = data.movement + i32::from(fx.contains(&BOOSTED));
    if fx.contains(&SWIFT) || fx.contains(&BUBBLE) {
        range += 1;
    }
    if fx.contains(&POISONED) && range > 1 {
        range -= 1;
    }
    Ok(range)
}

/// The movement for `unit` and its path-search profile (the profile does
/// not depend on the movement)
fn profile_with<'a>(
    g: &'a Game,
    t: &'a Tables,
    unit: &'a Unit,
    mover: &'a Mover,
) -> Result<(Profile<'a>, i32), String> {
    let fx = unit_effects(g, unit);
    let data = t.units.get(unit.kind as usize).ok_or("unknown unit kind")?;
    let has = |a: &str| data.unit_abilities.has(a);
    let range = movement_range(g, t, unit)?;
    // the unit's own terrain list, limited to what the player has unlocked unless it navigates
    let mut own_terrain = 0;
    for n in &data.movement_terrain {
        own_terrain |= terrain_bit(t.terrain.id(n)? as i32);
    }
    let mut allowed = if data.movement_terrain.is_empty() {
        mover.movements
    } else if has("navigate") {
        own_terrain
    } else {
        own_terrain & mover.movements
    };
    let (aquatic, fly) = (has("swim"), has("fly"));
    if fly {
        // allowed terrain: every catalog terrain but None
        allowed = (1..t.terrain.len() as i32)
            .filter(|&k| t.terrain.get(k as usize).is_some())
            .fold(0, |m, k| m | terrain_bit(k));
    } else if aquatic {
        allowed |= terrain_bit(WATER);
    }
    let hide = has("hide");
    let profile = Profile {
        g,
        t,
        unit,
        owner: unit.owner,
        slips_past: has("sneak") || hide || fx.contains(&INVISIBLE),
        hide,
        creep: has("creep"),
        aquatic,
        fly,
        skate: has("skate"),
        water_bound: aquatic && (has("carry") || !data.movement_terrain.is_empty()),
        carry: has("carry"),
        infiltrate: has("infiltrate"),
        allowed,
        required_port: !aquatic && !fly,
        allow_occupied: false,
        mover,
    };
    Ok((profile, range))
}
