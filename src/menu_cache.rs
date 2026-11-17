//! The legal list kept between decisions.
//! `Cache::menu` returns exactly what `menu::menu` returns. It keeps each explored tile's rows
//! (the tile families, and the moves and attacks of the mover's unit on it) and redoes only the
//! rows whose inputs changed since its last call. Each call compares every tile with what the
//! last one saw: a tile's fields (the Tile without its unit) and its unit (the unit, its
//! passenger and their effect lists) are compared separately.
//!
//! What redoes what:
//! - what the families read outside the tiles (`as_read` of every player record; the mover,
//!   the turn, the board's size and fixed settings, the tables): any change rebuilds the whole
//!   list;
//! - the mover's stars: nothing; the rows are kept with every price, and the list keeps those
//!   up to the stars (every star test in the families is `price <= stars` on the row's own
//!   price);
//! - the tile families on tile i (train, build, improvement abilities, unit actions) are
//!   redone when tile i changed; when a neighbour's fields changed (neighbour terrain for
//!   training, adjacency, bridge anchors); when a neighbour's unit changed and a mover's unit
//!   stands on i (healing, boosting, freezing, breaking ice read the neighbours' units); when
//!   the fields within 2 of i's ruling city changed (a `limited` build looks within 2 of the
//!   city; the city's level sets the training support); and when the census of units on the
//!   board by owner, kind and home changed and i holds a mover's city or unit (training
//!   support, `unique` units);
//! - the moves and attacks of the mover's unit on tile i are redone when anything changed in
//!   the box they read when last made: the unit's tile, its attack square (its range), and the
//!   tiles the move search read (2 around each tile it expanded: `movement::Reads`). The search
//!   reaches no tile more than twice the unit's movement away, so the box is never wider than
//!   that plus 2. A linked unit (leader or follower) reads its partner and is redone every time;
//! - a choice menu or the final turn leaves the rows as they are: they still hold the state
//!   last seen, and the next comparison covers every step since.
//!
//! Research and diplomacy are recomputed every time.

use crate::menu::{self, Cand, Shared};
use crate::movement::Reads;
use crate::state::{Game, Player, Tile, Unit, Xy};
use crate::tables::Tables;

type Effects = crate::inline::Inline<i64, 9>;

/// A unit and its effect list.
type WithEffects = Option<(Unit, Effects)>;

/// What the rows of one tile read from it.
#[derive(Clone, Copy, PartialEq)]
struct Seen {
    tile: Tile,
    unit: WithEffects,
    passenger: WithEffects,
}

impl Seen {
    fn unit_part(&self) -> (WithEffects, WithEffects) {
        (self.unit, self.passenger)
    }
    fn of(g: &Game, i: usize) -> Seen {
        let tile = g.tiles[i];
        let with_effects = |n: u32| {
            let u = g.units[n as usize];
            let fx = g.effect_lists.get(u.effects as usize).copied();
            (u, fx.unwrap_or_else(Effects::new))
        };
        let unit = tile.unit.map(with_effects);
        let passenger = unit.and_then(|(u, _)| u.passenger).map(with_effects);
        Seen {
            tile,
            unit,
            passenger,
        }
    }
}

/// What the rows read from outside the tiles (the mover's stars masked).
#[derive(Clone, PartialEq, Default)]
struct Global {
    tables: usize,
    fixed: usize,
    width: i32,
    tiles: usize,
    mover: i32,
    turn: i32,
    players: Vec<Player>,
}

/// A player's record with what the tile and unit families never read blanked: what they read
/// is the id, tribe, techs, tasks' kinds and completion, the relation states, the unique
/// buildings, the train block, the skin and the climate (every read is in menu.rs, movement.rs,
/// combat.rs, polaris.rs, cymanti.rs, aquarion.rs and units.rs). A field added to `Player`
/// later is compared unless it is blanked here. The rest (stars, score, counters, cities,
/// messages, known players, embassies) is read only by research and diplomacy, which are
/// recomputed every time, or by no rule.
fn as_read(p: &Player) -> Player {
    let mut p = *p;
    p.action.currency = 0;
    p.action.opinions_dirty = false;
    for task in p.action.tasks.iter_mut() {
        *task = crate::state::Task {
            kind: task.kind,
            flags: crate::state::TaskFlags {
                started: false,
                completed: task.flags.completed,
            },
            ..Default::default()
        };
    }
    p.known_players.clear();
    p.messages.clear();
    for (_, r) in p.relations.iter_mut() {
        *r = crate::state::Relation {
            state: r.state,
            ..crate::state::Relation::DEFAULT
        };
    }
    p.score = 0;
    p.cities = 0;
    p.kills = 0;
    p.casualties = 0;
    p.unlocked_tech_cache = None;
    p.start_tile = Xy::default();
    p.handicap = 0;
    p.auto_play = false;
    p.resigned_at_command = 0;
    p.wiped_at_command = 0;
    p.end_score = 0;
    p.color = 0;
    p.tribe_mix = 0;
    p.has_chosen_tribe = false;
    p.resigned_turn = 0;
    p.wipe_outs = 0;
    p.killer_id = 0;
    p.killed_turn = 0;
    p.capital_count_cache = 0;
    p
}

impl Global {
    fn of(g: &Game, t: &Tables, mover: i32) -> Global {
        let players = g.players.iter().map(as_read).collect();
        Global {
            tables: t as *const Tables as usize,
            fixed: std::sync::Arc::as_ptr(&g.fixed) as usize,
            width: g.width,
            tiles: g.tiles.len(),
            mover,
            turn: g.current_turn,
            players,
        }
    }
}

/// How much of the list the calls redid, and why.
#[derive(Clone, Copy, Debug, Default)]
pub struct Redone {
    pub calls: u64,
    /// whole lists built, and why: nothing kept (first call, an error, another array), another
    /// mover, another turn, a player record changed (techs, tasks, relations...)
    pub full: u64,
    pub full_by: [u64; 4],
    /// calls answered by a whole list of their own (a choice, the final turn, queued actions)
    pub whole: u64,
    /// calls that redid nothing but the stars changed (the rows filtered again)
    pub stars_only: u64,
    /// explored tiles looked at, and those whose tile families were redone: touched
    /// themselves, a neighbour's fields, a neighbour's unit (beside a mover's unit), the
    /// ruling city's surroundings, the unit census, the stars risen above the rows' level
    pub tiles_seen: u64,
    pub tiles: u64,
    pub tiles_by: [u64; 6],
    /// the mover's units looked at, and those whose moves and attacks were redone
    pub units_seen: u64,
    pub units: u64,
    /// rows built, and rows put in a list
    pub rows_built: u64,
    pub rows_listed: u64,
}

/// The legal list of one game, kept between its decisions.
#[derive(Default)]
pub struct Cache {
    valid: bool,
    global: Global,
    seen: Vec<Seen>,
    /// (owner, kind, home) of every unit standing on the board, sorted
    census: Vec<(i32, i32, Xy)>,
    census_now: Vec<(i32, i32, Xy)>,
    shared: Option<Shared>,
    tile_rows: Vec<Vec<Cand>>,
    /// per tile, the stars its tile rows were made with: they hold every row priced up to that
    tile_stars: Vec<i32>,
    unit_rows: Vec<Vec<Cand>>,
    /// per tile, what its unit rows read when they were made
    unit_reads: Vec<Reads>,
    /// prefix sums, (width + 1) by (height + 1), of the tiles whose own fields changed, and of
    /// those whose unit (with its passenger and their effects) changed
    fields: Vec<u32>,
    units: Vec<u32>,
    /// the board's arrays and write clock at the last call (`board.rs`)
    ids: (u64, u64, u64),
    clock: u64,
    /// the shadow mode (`checked`): also compare every tile with what was last seen, and
    /// record a change the write stamps missed
    verify: bool,
    unmarked: Option<String>,
    last_stars: i32,
    pub redone: Redone,
}

impl Cache {
    pub fn new() -> Cache {
        Cache::default()
    }

    /// The legal list of `g`, as `menu::menu` gives it.
    pub fn menu(&mut self, g: &Game, t: &Tables) -> Result<Vec<Cand>, String> {
        let _all = engine_fast_frame::timers::scope("menu");
        self.redone.calls += 1;
        // a choice menu or the final turn keeps the rows: they still hold the state last
        // seen, and the next comparison covers every step since
        match menu::whole_menu(g) {
            Ok(Some(whole)) => {
                self.redone.whole += 1;
                return Ok(whole);
            }
            Ok(None) => {}
            Err(e) => {
                self.valid = false;
                return Err(e);
            }
        }
        match self.update(g, t) {
            Ok(out) => Ok(out),
            // an error names the first family that refuses, in the full list's order
            Err(_) => {
                self.valid = false;
                menu::menu(g, t)
            }
        }
    }

    /// The heap bytes the cache holds (its rows at their capacity, the per-tile arrays and the
    /// prefix sums; the shadow mode's copy of the board apart), and the board's write stamps.
    pub fn bytes(&self, g: &Game) -> (usize, usize) {
        use std::mem::size_of;
        let rows = |v: &Vec<Vec<Cand>>| {
            v.capacity() * size_of::<Vec<Cand>>()
                + v.iter()
                    .map(|r| r.capacity() * size_of::<Cand>())
                    .sum::<usize>()
        };
        let cache = rows(&self.tile_rows)
            + rows(&self.unit_rows)
            + self.tile_stars.capacity() * size_of::<i32>()
            + self.unit_reads.capacity() * size_of::<Reads>()
            + (self.fields.capacity() + self.units.capacity()) * size_of::<u32>()
            + (self.census.capacity() + self.census_now.capacity()) * size_of::<(i32, i32, Xy)>()
            + self.global.players.capacity() * size_of::<Player>();
        (cache, g.tiles.len() * 2 * size_of::<u64>())
    }

    /// `menu`, checked against `menu::menu` (the shadow mode): Err names the first difference,
    /// or a tile whose contents changed with no write stamp on it ("unmarked write").
    pub fn checked(&mut self, g: &Game, t: &Tables) -> Result<Vec<Cand>, String> {
        self.verify = true;
        let kept = self.menu(g, t);
        if let Some(e) = self.unmarked.take() {
            return Err(e);
        }
        let full = menu::menu(g, t);
        match (&kept, &full) {
            (Ok(a), Ok(b)) if a == b => kept,
            (Err(a), Err(b)) if a == b => kept,
            (Ok(a), Ok(b)) => Err(
                match (0..a.len().max(b.len())).find(|&k| a.get(k) != b.get(k)) {
                    Some(k) => format!(
                        "kept list differs at row {k} of {} (full {}): {:?} against {:?}",
                        a.len(),
                        b.len(),
                        a.get(k).map(|c| &c.action),
                        b.get(k).map(|c| &c.action)
                    ),
                    None => "kept list differs".into(),
                },
            ),
            _ => Err(format!("kept list {kept:?} against full {full:?}")),
        }
    }

    fn update(&mut self, g: &Game, t: &Tables) -> Result<Vec<Cand>, String> {
        let mover = crate::step::mover(g);
        let stars = g.player(mover).ok_or("missing mover")?.action.currency;
        let global = Global::of(g, t, mover);
        let n = g.tiles.len();
        let ids = (g.tiles.id(), g.units.id(), g.effect_lists.id());
        let full = !self.valid || self.global != global || self.ids != ids;
        if full {
            let by = if !self.valid || self.ids != ids {
                0
            } else if self.global.mover != global.mover {
                1
            } else if self.global.turn != global.turn {
                2
            } else {
                3
            };
            self.redone.full_by[by] += 1;
        }
        let (tiles_before, units_before) = (self.redone.tiles, self.redone.units);
        self.valid = false;
        self.ids = ids;
        if full {
            self.redone.full += 1;
            self.global = global;
            self.shared = None;
            self.seen.clear();
            self.tile_rows.resize_with(n, Vec::new);
            self.tile_stars.clear();
            self.tile_stars.resize(n, i32::MIN);
            self.unit_rows.resize_with(n, Vec::new);
            self.unit_reads.clear();
            self.unit_reads.resize(n, Reads::NONE);
        }
        let any_unit = self.mark(g, full);
        // the census changes only with a unit
        let census_changed = any_unit && {
            self.census_now.clear();
            for x in &g.tiles {
                if let Some(u) = x.unit.map(|n| &g.units[n as usize]) {
                    self.census_now.push((u.owner, u.kind, u.home));
                }
            }
            self.census_now
                .sort_unstable_by_key(|&(o, k, h)| (o, k, h.x, h.y));
            let changed = self.census != self.census_now;
            std::mem::swap(&mut self.census, &mut self.census_now);
            changed
        };
        let (w, h) = (g.width, n as i32 / g.width);
        let on_board = |c: Xy| c.x >= 0 && c.y >= 0 && c.x < w && c.y < h;
        let mut moving = None;
        // for this state only: what `limited` builds look for around each city
        let mut limited = menu::Limited::default();
        let mut out = Vec::with_capacity(128);
        for i in 0..n {
            let tile = &g.tiles[i];
            if !tile.explorers.contains(&mover) {
                continue;
            }
            self.redone.tiles_seen += 1;
            let c = tile.coordinates;
            let own_unit = tile
                .unit
                .map(|n| &g.units[n as usize])
                .filter(|u| u.owner == mover);
            let own_city =
                tile.owner == mover && tile.improvement.as_ref().is_some_and(|m| m.kind == 1);
            let rc = tile.ruling_city;
            // why the tile families here are redone, if they are (Redone::tiles_by)
            let by = if full
                || self.touched(&self.fields, w, h, c, 0)
                || self.touched(&self.units, w, h, c, 0)
            {
                Some(0)
            } else if self.touched(&self.fields, w, h, c, 1) {
                Some(1)
            } else if own_unit.is_some() && self.touched(&self.units, w, h, c, 1) {
                Some(2)
            } else if on_board(rc) && self.touched(&self.fields, w, h, rc, 2) {
                Some(3)
            } else if census_changed && (own_unit.is_some() || own_city) {
                Some(4)
            } else if stars > self.tile_stars[i] {
                Some(5)
            } else {
                None
            };
            if let Some(k) = by {
                self.redone.tiles += 1;
                self.redone.tiles_by[k] += 1;
                let rows = &mut self.tile_rows[i];
                rows.clear();
                menu::tile_rows(g, t, mover, i, &mut self.shared, &mut limited, stars, rows)?;
                self.tile_stars[i] = stars;
                self.redone.rows_built += rows.len() as u64;
            }
            if let Some(u) = own_unit {
                self.redone.units_seen += 1;
                let linked = u.leader != 0 || u.follower != 0;
                let r = self.unit_reads[i];
                let stale = full
                    || linked
                    || self.touched(&self.fields, w, h, c, 0)
                    || self.touched(&self.units, w, h, c, 0)
                    || self.touched_in(&self.fields, w, h, r)
                    || self.touched_in(&self.units, w, h, r);
                if stale {
                    self.redone.units += 1;
                    let rows = &mut self.unit_rows[i];
                    rows.clear();
                    // the unit's own tile and its attack square, then what the move search reads
                    let range = t
                        .units
                        .get(u.kind as usize)
                        .ok_or("unknown unit kind")?
                        .range;
                    let mut reads = Reads::NONE;
                    reads.add(c, range.max(0));
                    menu::unit_rows(g, t, mover, i, &mut moving, &mut reads, rows)?;
                    self.unit_reads[i] = reads;
                    self.redone.rows_built += rows.len() as u64;
                }
            } else {
                self.unit_rows[i].clear();
                self.unit_reads[i] = Reads::NONE;
            }
            let before = out.len();
            let affordable = |r: &&Cand| r.details.price.is_none_or(|p| p <= stars);
            out.extend(self.tile_rows[i].iter().filter(affordable).cloned());
            out.extend(self.unit_rows[i].iter().filter(affordable).cloned());
            self.redone.rows_listed += (out.len() - before) as u64;
        }
        menu::tail(g, t, mover, &mut out)?;
        if !full
            && self.redone.tiles == tiles_before
            && self.redone.units == units_before
            && stars != self.last_stars
        {
            self.redone.stars_only += 1;
        }
        self.last_stars = stars;
        self.valid = true;
        Ok(out)
    }

    /// Build the prefix sums of the tiles whose fields, and whose unit, were written since the
    /// last call (every tile when `all`), from the board's write stamps. In the shadow mode
    /// also compare every tile with what was last seen and record the first change without a
    /// stamp. Whether any tile's unit was written.
    fn mark(&mut self, g: &Game, all: bool) -> bool {
        let n = g.tiles.len();
        let (w, h) = (g.width as usize, n / g.width as usize);
        let seen = self.verify && self.seen.len() == n;
        if self.verify && !seen {
            self.seen.clear();
        }
        for sums in [&mut self.fields, &mut self.units] {
            sums.clear();
            sums.resize((w + 1) * (h + 1), 0);
        }
        let since = self.clock;
        let mut any_unit = false;
        for y in 0..h {
            let (mut fields_row, mut units_row) = (0, 0);
            for x in 0..w {
                let i = y * w + x;
                let fields = all || g.tiles.fields_stamp(i) > since;
                let unit = all || g.tiles.unit_stamp(i) > since;
                if self.verify {
                    if !seen {
                        self.seen.push(Seen::of(g, i));
                    } else {
                        let now = Seen::of(g, i);
                        let was = &self.seen[i];
                        let fields_differ = now.tile != was.tile && {
                            let (mut a, mut b) = (now.tile, was.tile);
                            (a.unit, b.unit) = (None, None);
                            a != b
                        };
                        let unit_differs =
                            now.tile.unit != was.tile.unit || now.unit_part() != was.unit_part();
                        if (fields_differ && !fields) || (unit_differs && !unit) {
                            self.unmarked.get_or_insert_with(|| {
                                let what = if fields_differ && !fields {
                                    "fields"
                                } else {
                                    "unit"
                                };
                                format!("unmarked write: tile {i} ({what}) changed with no stamp")
                            });
                        }
                        self.seen[i] = now;
                    }
                }
                any_unit |= unit;
                fields_row += u32::from(fields);
                units_row += u32::from(unit);
                let at = (y + 1) * (w + 1) + x + 1;
                self.fields[at] = self.fields[at - (w + 1)] + fields_row;
                self.units[at] = self.units[at - (w + 1)] + units_row;
            }
        }
        self.clock = g.tiles.clock();
        any_unit
    }

    /// Whether a tile counted in `sums` lies within `r` (Chebyshev) of `c`.
    fn touched(&self, sums: &[u32], w: i32, h: i32, c: Xy, r: i32) -> bool {
        let mut b = Reads::NONE;
        b.add(c, r);
        self.touched_in(sums, w, h, b)
    }

    /// Whether a tile counted in `sums` lies in the box `b`.
    fn touched_in(&self, sums: &[u32], w: i32, h: i32, b: Reads) -> bool {
        let (x0, y0) = (b.x0.max(0) as usize, b.y0.max(0) as usize);
        let (x1, y1) = (b.x1.min(w - 1) + 1, b.y1.min(h - 1) + 1);
        if x1 <= x0 as i32 || y1 <= y0 as i32 {
            return false;
        }
        let (x1, y1) = (x1 as usize, y1 as usize);
        let s = |x: usize, y: usize| sums[y * (w as usize + 1) + x];
        s(x1, y1) + s(x0, y0) > s(x0, y1) + s(x1, y0)
    }
}
