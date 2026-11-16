//! A game's rule state as the fixed-size block the kernel sources work on
//! (`kernel/state.h`), and its conversion from and to the Rust engine's `Game`.
//!
//! The structs here mirror `kernel/state.h` field for field; `tests/state_roundtrip.rs`
//! compares their sizes and offsets with the C compiler's. The block leaves out what no rule
//! reads (`Side`, `Memory`, the checkpoint's constant parts), so `from_device` takes those
//! from a game given as the template.
//!
//! Unit and effect-list numbers are handles: the block numbers them in the order the tiles
//! reach them and drops the ones nothing reaches. `canonical` puts a `Game` in that same
//! numbering, which is how two states are compared.

use engine_fast::inline::{Counts, Inline};
use engine_fast::mapgen::candidate::Continent;
use engine_fast::state::{
    Game, HashSeed, Improvement, Message, Player, PlayerAction, Relation, Seeds, Task, TaskFlags,
    TaskKind, Tile, Trigger, Unit, Xy,
};
use std::collections::HashMap;

pub const MAX_SIDE: usize = 30;
pub const MAX_TILES: usize = MAX_SIDE * MAX_SIDE;
pub const MAX_UNITS: usize = 2 * MAX_TILES + 32;
/// The most units a block holds by default, whatever its board: well above the most live units
/// measured (465, 30 x 30 with 4 seats over 6 000 decisions);
/// a game that would pass it stops with the device's unit-table capacity, never wraps.
pub const DEFAULT_UNIT_CAP: usize = 1024;
pub const MAX_PLAYERS: usize = 5;
pub const MAX_TECH: usize = 56;
pub const MAX_TASKS: usize = 10;
pub const MAX_TRIGGERS: usize = 8;
pub const NAMING_SEEDS: usize = 1024;
pub const MAX_ACTIONS: usize = 128;
pub const NONE: i32 = -1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EfXy {
    pub x: i32,
    pub y: i32,
}

pub const MAX_CONTINENTS: usize = 24;
pub const CONTINENT_WEIGHTS: usize = 14;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EfContinent {
    pub weights: [u32; CONTINENT_WEIGHTS],
    pub climate: i32,
    pub skin: i32,
    pub alien: i32,
    pub detached: i32,
    pub has_start: i32,
    pub pad: i32,
    pub start: EfXy,
}

/// A continent as the device holds it (kernel/action.h EfContinent)
pub fn continent_to(c: &Continent) -> Result<EfContinent, String> {
    let w = [
        c.crop, c.fish, c.fruit, c.game, c.metal, c.whale, c.spores, c.aquacrop, c.water, c.ocean,
        c.field, c.mountain, c.forest, c.ice,
    ];
    let start = c.tiles.first();
    Ok(EfContinent {
        weights: w.map(f32::to_bits),
        climate: c.climate,
        skin: c.skin,
        alien: i32::from(c.has_alien_climate),
        detached: i32::from(c.detached),
        has_start: i32::from(start.is_some()),
        pad: 0,
        start: start.map_or(EfXy::default(), |&s| xy(s)),
    })
}

/// A continent the device made (a lost city's): the fresh continent with the device's fields
fn continent_from(e: &EfContinent) -> Continent {
    let mut c = engine_fast::mapgen::candidate::fresh_continent();
    let w = e.weights.map(f32::from_bits);
    [
        c.crop, c.fish, c.fruit, c.game, c.metal, c.whale, c.spores, c.aquacrop, c.water, c.ocean,
        c.field, c.mountain, c.forest, c.ice,
    ] = w;
    c.climate = e.climate;
    c.skin = e.skin;
    c.has_alien_climate = e.alien != 0;
    c.detached = e.detached != 0;
    if e.has_start != 0 {
        c.tiles.push(Xy {
            x: e.start.x,
            y: e.start.y,
        });
    }
    c
}

fn continents_of(g: &Game) -> Option<Vec<Continent>> {
    serde_json::from_value(g.continents().clone()).ok()
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfImprovement {
    pub production_n: u16,
    pub base_score_n: u16,
    pub upgrade_n: u16,
    pub level_n: u16,
    pub founded_n: i16,
    pub population_n: i16,
    pub xp_n: i16,
    pub owner_n: u8,
    pub founder_n: u8,
    pub connected_to_capital_of_player_n: u8,
    pub kind_n: i8,
    pub border_size_n: i8,
    pub named_n: i8,
    pub effects_len_n: i8,
    pub effects_n: [i8; 2],
    pub rewards_n: [u8; 14],
    pub pad: i8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfTile {
    pub coordinates: EfXy,
    pub ruling_city: EfXy,
    pub improvement: EfImprovement,
    pub unit_n: i16,
    pub continent_n: i16,
    pub last_population_check_n: i16,
    pub available_population_n: i16,
    pub owner_n: u8,
    pub capital_of_n: u8,
    pub explorers_n: [u8; 5],
    pub terrain_n: i8,
    pub climate_n: i8,
    pub altitude_n: i8,
    pub explorers_len_n: i8,
    pub has_improvement_n: i8,
    pub has_resource_n: i8,
    pub resource_n: i8,
    pub has_road_n: i8,
    pub has_route_n: i8,
    pub has_continent_n: i8,
    pub had_route_n: i8,
    pub skin_n: i8,
    pub effects_len_n: i8,
    pub effects_n: [i8; 5],
    pub pad: i8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfUnit {
    pub id: u32,
    pub leader: u32,
    pub follower: u32,
    pub previous_turn_end_coordinates: EfXy,
    pub coordinates: EfXy,
    pub home: EfXy,
    pub passenger_n: i16,
    pub effects_n: i16,
    pub health_n: u16,
    pub xp_n: i16,
    pub created_turn_n: i16,
    pub owner_n: u8,
    pub used_n: i8,
    pub kind_n: i8,
    pub promotion_level_n: i8,
    pub moved_n: i8,
    pub attacked_n: i8,
    pub direction_n: i8,
    pub flipped_n: i8,
    pub is_temporary_explorer_n: i8,
    pub birth_climate_n: i8,
    pub birth_climate_skin_n: i8,
    pub pad: [i8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfEffectList {
    pub used_n: i8,
    pub len_n: i8,
    pub items_n: [i8; 9],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfTask {
    pub kind: i32,
    pub started: i32,
    pub completed: i32,
    pub has_peaceful_turns: i32,
    pub has_kills: i32,
    pub has_conversions: i32,
    pub has_explored_tiles: i32,
    pub pad: i32,
    pub peaceful_turns: i64,
    pub kills: i64,
    pub conversions: i64,
    pub explored_tiles: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfRelation {
    pub state: i32,
    pub last_attack_turn: i32,
    pub embassy_level: i32,
    pub last_peace_broken_turn: i32,
    pub first_meet: i32,
    pub embassy_build_turn: i32,
    pub previous_attack_turn: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfPlayer {
    pub id: i32,
    pub currency: i32,
    pub tasks_len: i32,
    pub opinions_dirty: i32,
    pub tasks: [EfTask; MAX_TASKS],
    pub tribe: i32,
    pub available_tech_len: i32,
    pub available_tech: [i32; MAX_TECH],
    pub known_players_len: i32,
    pub known_players: [i32; MAX_PLAYERS],
    pub cities: i32,
    pub kills: i32,
    pub casualties: i32,
    pub relations_len: i32,
    pub relation_ids: [i32; MAX_PLAYERS],
    pub relations: [EfRelation; MAX_PLAYERS],
    pub has_tech_cache: i32,
    pub tech_cache_len: i32,
    pub tech_cache: [i32; MAX_TECH],
    pub start_tile: EfXy,
    pub handicap: i32,
    pub auto_play: i32,
    pub messages_len: i32,
    pub message_senders: [i32; MAX_PLAYERS],
    pub built_unique_len: i32,
    pub built_unique: [i32; 8],
    pub block_train_units: i32,
    pub skin: i32,
    pub climate: i32,
    pub has_chosen_tribe: i32,
    pub pad: i32,
    pub message_kinds: [i64; MAX_PLAYERS],
    pub score: i64,
    pub resigned_at_command: i64,
    pub wiped_at_command: i64,
    pub end_score: i64,
    pub color: i64,
    pub tribe_mix: i64,
    pub resigned_turn: i64,
    pub wipe_outs: i64,
    pub killer_id: i64,
    pub killed_turn: i64,
    pub capital_count_cache: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfTrigger {
    pub player: i32,
    pub opponent: i32,
    pub kind: i64,
    pub coordinates: EfXy,
}

#[repr(C)]
pub struct EfGame {
    pub width: i32,
    pub tile_count: i32,
    pub player_count: i32,
    pub seats: i32,
    pub win_by_capital: i32,
    pub win_by_extermination: i32,
    pub achievements_active: i32,
    pub naming_case: i32,
    pub version: i32,
    pub game_skin: i32,
    pub rules_mode: i64,
    pub game_type: i64,
    pub turn_limit: i64,
    pub score_limit: i64,
    pub death_condition: i64,
    pub action_limit: i64,
    pub current_turn: i32,
    pub current_player_index: i32,
    pub current_unit_id: u32,
    pub needs_routes: i32,
    pub current_state: i64,
    pub current_command: u64,
    pub decisions: u64,
    pub commands: u64,
    pub gameplay_seed: i64,
    pub village_name_seed: i64,
    pub random_hash: i64,
    pub village_name_hash: i64,
    pub has_random_hash: i32,
    pub has_village_name_hash: i32,
    pub naming_next: i32,
    pub naming_count: i32,
    pub triggers_len: i32,
    pub actions_len: i32,
    pub tile_cap: i32,
    pub unit_cap: i32,
    pub triggers: [EfTrigger; MAX_TRIGGERS],
    pub actions: [crate::action::EfAction; MAX_ACTIONS],
    pub players: [EfPlayer; MAX_PLAYERS],
    pub naming_seeds: [i32; NAMING_SEEDS],
    pub continent_count: i32,
    pub continent_pad: i32,
    pub continents: [EfContinent; MAX_CONTINENTS],
}

/// The default capacities for a board of `tiles` tiles: its tiles, and units as the C's
/// `2 * tiles + 32` up to DEFAULT_UNIT_CAP.
pub fn default_caps(tiles: usize) -> (i32, i32) {
    (tiles as i32, (2 * tiles + 32).min(DEFAULT_UNIT_CAP) as i32)
}

/// kernel/state.h's layout: the header, then units, tiles and effect lists, padded to 8 bytes.
pub fn units_offset() -> usize {
    std::mem::size_of::<EfGame>()
}
pub fn tiles_offset(unit_cap: i32) -> usize {
    units_offset() + unit_cap as usize * std::mem::size_of::<EfUnit>()
}
pub fn lists_offset(tile_cap: i32, unit_cap: i32) -> usize {
    tiles_offset(unit_cap) + tile_cap as usize * std::mem::size_of::<EfTile>()
}
pub fn block_size(tile_cap: i32, unit_cap: i32) -> usize {
    let end = lists_offset(tile_cap, unit_cap)
        + (unit_cap as usize + 1) * std::mem::size_of::<EfEffectList>();
    end.div_ceil(8) * 8
}

/// Blocks of the same capacities end to end, for one call into the C side, and their
/// capacities (tiles, units; zero for no blocks).
pub fn pack(blocks: &[Block]) -> (Vec<u64>, (i32, i32)) {
    let caps = blocks.first().map_or((0, 0), |b| (b.tile_cap, b.unit_cap));
    let words = blocks.first().map_or(0, |b| b.words.len());
    let mut flat = Vec::with_capacity(blocks.len() * words);
    for b in blocks {
        b.assert_own_caps();
        assert_eq!(
            (b.tile_cap, b.unit_cap),
            caps,
            "blocks of one call have the same capacities"
        );
        flat.extend_from_slice(&b.words);
    }
    (flat, caps)
}

/// Copy `pack`ed blocks back into the blocks they came from.
pub fn unpack(flat: &[u64], blocks: &mut [Block]) {
    let words = blocks.first().map_or(0, |b| b.words.len());
    assert_eq!(flat.len(), blocks.len() * words);
    for (b, w) in blocks.iter_mut().zip(flat.chunks_exact(words.max(1))) {
        b.words.copy_from_slice(w);
    }
}

/// One game's device block: the header `EfGame` (reached by Deref) and its regions, laid out
/// for its own capacities.
pub struct Block {
    words: Box<[u64]>,
}

impl Block {
    /// An all-zero block with room for `tile_cap` tiles and `unit_cap` units.
    pub fn new(tile_cap: i32, unit_cap: i32) -> Block {
        assert!(
            (1..=MAX_TILES as i32).contains(&tile_cap)
                && (1..=MAX_UNITS as i32).contains(&unit_cap)
        );
        let mut b = Block {
            words: vec![0u64; block_size(tile_cap, unit_cap) / 8].into_boxed_slice(),
        };
        b.tile_cap = tile_cap;
        b.unit_cap = unit_cap;
        b
    }

    /// The block's bytes, padding included (zero, as `new` leaves them).
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: the words are plain integers, viewed as bytes.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast(), self.words.len() * 8) }
    }

    /// The block's bytes, to overwrite with another block of the same capacities.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as for `bytes`; every byte pattern is a valid value of the block's fields.
        unsafe {
            std::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast(), self.words.len() * 8)
        }
    }

    fn region<T>(&self, offset: usize, len: usize) -> &[T] {
        assert!(offset + len * std::mem::size_of::<T>() <= self.words.len() * 8);
        // SAFETY: in bounds (checked), aligned (every region starts at a multiple of 4 and the
        // structs hold no field wider than 4 bytes), plain integers.
        unsafe { std::slice::from_raw_parts(self.bytes().as_ptr().add(offset).cast(), len) }
    }
    fn region_mut<T>(&mut self, offset: usize, len: usize) -> &mut [T] {
        assert!(offset + len * std::mem::size_of::<T>() <= self.words.len() * 8);
        // SAFETY: as for `region`.
        unsafe {
            std::slice::from_raw_parts_mut(self.bytes_mut().as_mut_ptr().add(offset).cast(), len)
        }
    }
    pub fn units(&self) -> &[EfUnit] {
        self.region(units_offset(), self.unit_cap as usize)
    }
    pub fn units_mut(&mut self) -> &mut [EfUnit] {
        let n = self.unit_cap as usize;
        self.region_mut(units_offset(), n)
    }
    pub fn tiles(&self) -> &[EfTile] {
        self.region(tiles_offset(self.unit_cap), self.tile_cap as usize)
    }
    pub fn tiles_mut(&mut self) -> &mut [EfTile] {
        let (o, n) = (tiles_offset(self.unit_cap), self.tile_cap as usize);
        self.region_mut(o, n)
    }
    pub fn lists(&self) -> &[EfEffectList] {
        self.region(
            lists_offset(self.tile_cap, self.unit_cap),
            self.unit_cap as usize + 1,
        )
    }
    pub fn lists_mut(&mut self) -> &mut [EfEffectList] {
        let (o, n) = (
            lists_offset(self.tile_cap, self.unit_cap),
            self.unit_cap as usize + 1,
        );
        self.region_mut(o, n)
    }
}

impl Block {
    /// The whole block for the C side, which reads its regions past the header (a pointer from
    /// `Deref` would cover the header alone).
    /// The host build sizes the block by its capacities (and some of its entry points, the
    /// digest and the probe, run no check), so a block whose capacities were changed is
    /// refused here; the device checks them against the room it has.
    pub fn as_ptr(&self) -> *const EfGame {
        self.assert_own_caps();
        self.words.as_ptr().cast()
    }
    pub fn as_mut_ptr(&mut self) -> *mut EfGame {
        self.assert_own_caps();
        self.words.as_mut_ptr().cast()
    }
    fn assert_own_caps(&self) {
        let in_range = (1..=MAX_TILES as i32).contains(&self.tile_cap)
            && (1..=MAX_UNITS as i32).contains(&self.unit_cap);
        assert!(
            in_range && block_size(self.tile_cap, self.unit_cap) == self.words.len() * 8,
            "a block's capacities describe its own buffer"
        );
        // the probe and the dump walk tile_count tiles without the full check
        assert!(
            (0..=self.tile_cap).contains(&self.tile_count),
            "a block's tiles fit its capacity"
        );
    }
}

impl Clone for Block {
    fn clone(&self) -> Block {
        Block {
            words: self.words.clone(),
        }
    }
}

impl std::ops::Deref for Block {
    type Target = EfGame;
    fn deref(&self) -> &EfGame {
        // SAFETY: the block starts with the header, aligned to 8 bytes.
        unsafe { &*self.words.as_ptr().cast::<EfGame>() }
    }
}

impl std::ops::DerefMut for Block {
    fn deref_mut(&mut self) -> &mut EfGame {
        // SAFETY: as for `deref`.
        unsafe { &mut *self.words.as_mut_ptr().cast::<EfGame>() }
    }
}

fn xy(c: Xy) -> EfXy {
    EfXy { x: c.x, y: c.y }
}

fn back(c: EfXy) -> Xy {
    Xy { x: c.x, y: c.y }
}

/// Copy a short list into a fixed array and give its length.
fn list<T: Copy, U: Copy, const N: usize>(
    from: &[T],
    to: &mut [U; N],
    what: &str,
    f: impl Fn(T) -> U,
) -> Result<i32, String> {
    if from.len() > N {
        return Err(format!("{what}: {} entries, room for {N}", from.len()));
    }
    for (slot, &v) in to.iter_mut().zip(from) {
        *slot = f(v);
    }
    Ok(from.len() as i32)
}

fn short<T: Copy + Default, U: Copy, const N: usize, const M: usize>(
    len: i32,
    items: &[U; N],
    what: &str,
    f: impl Fn(U) -> T,
) -> Result<Inline<T, M>, String> {
    let len = usize::try_from(len).ok().filter(|&n| n <= N && n <= M);
    let len = len.ok_or_else(|| format!("{what}: length out of range"))?;
    let mut out = Inline::new();
    for &v in &items[..len] {
        out.push(f(v));
    }
    Ok(out)
}

const NAMING_CASES: [&str; 2] = ["Common", "Turkic"];

/// The numbering of the units and effect lists the tiles reach, in tile order: for each, the
/// old index of the entry that gets each new number.
struct Reached {
    units: Vec<u32>,
    unit_of: HashMap<u32, i32>,
    lists: Vec<u32>,
    list_of: HashMap<u32, i32>,
}

/// `Reached::lists`' entry for device slot 0, the empty list no engine list maps to
const EMPTY_SLOT: u32 = u32::MAX;

/// The units and lists the tiles reach, numbered in reach order. Number 0 is the device's
/// reserved empty list, which no unit owns (the kernels' placement scratch reads it, as the
/// engine's reads no list); every engine list a unit reaches, list 0 included, gets a number
/// from 1, so a list that empties later is still the list its unit owns.
fn reached(g: &Game) -> Result<Reached, String> {
    let mut r = Reached {
        units: Vec::new(),
        unit_of: HashMap::new(),
        lists: vec![EMPTY_SLOT],
        list_of: HashMap::new(),
    };
    for tile in &g.tiles {
        let mut node = tile.unit;
        while let Some(n) = node {
            let u = g.units.get(n as usize).ok_or("unit node out of range")?;
            if r.unit_of.insert(n, r.units.len() as i32).is_some() {
                return Err(format!("unit node {n} is reached twice"));
            }
            r.units.push(n);
            if !r.list_of.contains_key(&u.effects) {
                if u.effects as usize >= g.effect_lists.len() {
                    return Err("effect list out of range".into());
                }
                r.list_of.insert(u.effects, r.lists.len() as i32);
                r.lists.push(u.effects);
            }
            node = u.passenger;
        }
    }
    Ok(r)
}

/// The game as a device block with its board's default capacities (`default_caps`).
pub fn to_device(g: &Game) -> Result<Block, String> {
    let (tiles, units) = default_caps(g.tiles.len().max(1));
    to_device_with(g, tiles, units)
}

/// The game as a device block with room for `tile_cap` tiles and `unit_cap` units (a batch's
/// common layout). Fails when the game does not fit.
pub fn to_device_with(g: &Game, tile_cap: i32, unit_cap: i32) -> Result<Block, String> {
    let r = reached(g)?;
    if g.tiles.len() > MAX_TILES || g.width as usize > MAX_SIDE {
        return Err(format!(
            "board of {} tiles is larger than the device block",
            g.tiles.len()
        ));
    }
    if g.tiles.len() > tile_cap as usize {
        return Err(format!(
            "board of {} tiles is larger than the batch's {tile_cap}",
            g.tiles.len()
        ));
    }
    if r.units.len() > unit_cap as usize || g.players.len() > MAX_PLAYERS {
        return Err("more units or players than the device block holds".into());
    }
    let mut d = Block::new(tile_cap, unit_cap);
    d.width = g.width;
    d.tile_count = g.tiles.len() as i32;
    d.player_count = g.players.len() as i32;
    let f = &g.fixed;
    d.seats = f.seats as i32;
    d.win_by_capital = i32::from(f.win_by_capital);
    d.win_by_extermination = i32::from(f.win_by_extermination);
    d.achievements_active = i32::from(f.achievements_active);
    d.naming_case = NAMING_CASES
        .iter()
        .position(|c| *c == f.naming_case)
        .unwrap_or(2) as i32;
    d.version = f.version;
    d.game_skin = g.game_skin();
    d.rules_mode = f.rules_mode;
    d.game_type = f.game_type;
    d.turn_limit = f.turn_limit;
    d.score_limit = f.score_limit;
    d.death_condition = f.death_condition;
    d.action_limit = f.action_limit as i64;
    d.current_turn = g.current_turn;
    d.current_player_index = g.current_player_index;
    d.current_unit_id = g.current_unit_id;
    d.needs_routes = i32::from(g.needs_routes);
    d.current_state = g.current_state;
    d.current_command = g.current_command;
    d.decisions = g.decisions;
    d.commands = g.commands;
    d.gameplay_seed = g.seeds.gameplay_seed;
    d.village_name_seed = g.seeds.village_name_seed;
    d.has_random_hash = i32::from(g.seeds.random_hash.is_some());
    d.random_hash = g.seeds.random_hash.map_or(0, |h| h.seed);
    d.has_village_name_hash = i32::from(g.seeds.village_name_hash.is_some());
    d.village_name_hash = g.seeds.village_name_hash.map_or(0, |h| h.seed);
    if g.naming_seeds.len() > NAMING_SEEDS {
        return Err("more naming seeds than the device block holds".into());
    }
    d.naming_count = g.naming_seeds.len() as i32;
    for (slot, &seed) in d.naming_seeds.iter_mut().zip(&g.naming_seeds) {
        *slot = i32::try_from(seed).map_err(|_| "a naming seed is outside 32 bits")?;
    }
    match continents_of(g) {
        Some(list) => {
            if list.len() > MAX_CONTINENTS {
                return Err("more continents than the device block holds".into());
            }
            d.continent_count = list.len() as i32;
            for (slot, c) in d.continents.iter_mut().zip(&list) {
                *slot = continent_to(c)?;
            }
        }
        None => d.continent_count = -1,
    }
    if g.triggers.len() > MAX_TRIGGERS {
        return Err("more pending choices than the device block holds".into());
    }
    d.triggers_len = g.triggers.len() as i32;
    for (slot, t) in d.triggers.iter_mut().zip(&g.triggers) {
        *slot = EfTrigger {
            player: t.player,
            opponent: t.opponent,
            kind: t.kind,
            coordinates: xy(t.coordinates),
        };
    }
    if g.actions.len() > MAX_ACTIONS {
        return Err("more queued actions than the device block holds".into());
    }
    d.actions_len = g.actions.len() as i32;
    for (slot, a) in d.actions.iter_mut().zip(&g.actions) {
        *slot = crate::action::to_device(a)?;
    }
    for (slot, p) in d.players.iter_mut().zip(&g.players) {
        player_to(p, slot)?;
    }
    for (new, &old) in r.lists.iter().enumerate() {
        let from: &[i64] = if old == EMPTY_SLOT {
            &[]
        } else {
            &g.effect_lists[old as usize]
        };
        let to = &mut d.lists_mut()[new];
        to.used_n = 1;
        if from.len() > to.items_n.len() {
            return Err(format!("unit effects: {} entries, room for 9", from.len()));
        }
        for (slot, &v) in to.items_n.iter_mut().zip(from.iter()) {
            *slot = i8::try_from(v)
                .map_err(|_| format!("unit effect {v} is outside the device's range"))?;
        }
        to.len_n = from.len() as i8;
    }
    for (new, &old) in r.units.iter().enumerate() {
        let u = &g.units[old as usize];
        d.units_mut()[new] = EfUnit {
            used_n: 1,
            id: u.id,
            leader: u.leader,
            follower: u.follower,
            owner_n: fit(u.owner, "unit owner")?,
            kind_n: fit(u.kind, "unit kind")?,
            previous_turn_end_coordinates: xy(u.previous_turn_end_coordinates),
            coordinates: xy(u.coordinates),
            home: xy(u.home),
            passenger_n: fit(
                u.passenger.map_or(NONE, |n| r.unit_of[&n]),
                "passenger slot",
            )?,
            health_n: fit(u.health, "health")?,
            promotion_level_n: fit(u.promotion_level, "promotion level")?,
            xp_n: fit(u.xp, "unit xp")?,
            moved_n: i8::from(u.moved),
            attacked_n: i8::from(u.attacked),
            direction_n: fit(u.direction, "direction")?,
            flipped_n: i8::from(u.flipped),
            created_turn_n: fit(u.created_turn, "creation turn")?,
            effects_n: fit(r.list_of[&u.effects], "effect list slot")?,
            is_temporary_explorer_n: i8::from(u.is_temporary_explorer),
            birth_climate_n: fit(u.birth_climate, "birth climate")?,
            birth_climate_skin_n: fit(u.birth_climate_skin, "birth climate skin")?,
            pad: [0; 3],
        };
    }
    for unit in &mut d.units_mut()[r.units.len()..] {
        unit.passenger_n = -1;
    }
    for (slot, t) in d.tiles_mut().iter_mut().zip(&g.tiles) {
        tile_to(t, slot, &r)?;
    }
    for tile in &mut d.tiles_mut()[g.tiles.len()..] {
        tile.unit_n = -1;
    }
    Ok(d)
}

/// `v` in the narrower type of a device field, or the refusal naming the field (a value that
/// does not fit is refused, never truncated).
fn fit<T: TryFrom<i64>>(v: impl Into<i64>, what: &str) -> Result<T, String> {
    let v = v.into();
    T::try_from(v).map_err(|_| format!("{what} {v} is outside the device's range"))
}

/// A list into a narrowed device array: its entries through `fit`, its length as an int8.
fn fit_list<T: Copy + Into<i64>, U: TryFrom<i64>, const N: usize>(
    from: &[T],
    to: &mut [U; N],
    what: &str,
) -> Result<i8, String> {
    if from.len() > N {
        return Err(format!("{what}: {} entries, room for {N}", from.len()));
    }
    for (slot, &v) in to.iter_mut().zip(from) {
        *slot = fit(v, what)?;
    }
    Ok(from.len() as i8)
}

fn tile_to(t: &Tile, d: &mut EfTile, r: &Reached) -> Result<(), String> {
    d.coordinates = xy(t.coordinates);
    d.terrain_n = fit(t.terrain, "terrain")?;
    d.climate_n = fit(t.climate, "climate")?;
    d.altitude_n = fit(t.altitude, "altitude")?;
    d.owner_n = fit(t.owner, "tile owner")?;
    d.capital_of_n = fit(t.capital_of, "capital of")?;
    d.explorers_len_n = fit_list(&t.explorers, &mut d.explorers_n, "explorer")?;
    d.ruling_city = xy(t.ruling_city);
    d.has_improvement_n = i8::from(t.improvement.is_some());
    if let Some(m) = &t.improvement {
        let i = &mut d.improvement;
        i.kind_n = fit(m.kind, "improvement kind")?;
        i.owner_n = fit(m.owner, "improvement owner")?;
        i.founder_n = fit(m.founder, "founder")?;
        i.level_n = fit(m.level, "level")?;
        i.founded_n = fit(m.founded, "founding turn")?;
        i.xp_n = fit(m.xp, "improvement xp")?;
        i.population_n = fit(m.population, "population")?;
        i.production_n = fit(m.production, "production")?;
        i.base_score_n = fit(m.base_score, "base score")?;
        i.border_size_n = fit(m.border_size, "border size")?;
        i.upgrade_n = fit(m.upgrade, "upgrade")?;
        i.connected_to_capital_of_player_n = fit(m.connected_to_capital_of_player, "connection")?;
        i.named_n = i8::from(m.named);
        for (kind, slot) in i.rewards_n.iter_mut().enumerate() {
            *slot = fit(m.rewards.count(kind as i32) as i64, "reward count")?;
        }
        i.effects_len_n = fit_list(&m.effects, &mut i.effects_n, "improvement effect")?;
    }
    d.has_resource_n = i8::from(t.resource.is_some());
    d.resource_n = fit(t.resource.unwrap_or(0), "resource")?;
    d.unit_n = fit(t.unit.map_or(NONE, |n| r.unit_of[&n]), "unit slot")?;
    d.has_road_n = i8::from(t.has_road);
    d.has_route_n = i8::from(t.has_route);
    d.has_continent_n = i8::from(t.continent.is_some());
    d.continent_n = fit(t.continent.unwrap_or(0), "continent")?;
    d.had_route_n = i8::from(t.had_route);
    d.last_population_check_n = fit(t.last_population_check, "population check")?;
    d.available_population_n = fit(t.available_population, "available population")?;
    d.skin_n = fit(t.skin, "tile skin")?;
    d.effects_len_n = fit_list(&t.effects, &mut d.effects_n, "tile effect")?;
    Ok(())
}

fn player_to(p: &Player, d: &mut EfPlayer) -> Result<(), String> {
    d.id = p.action.id;
    d.currency = p.action.currency;
    d.opinions_dirty = i32::from(p.action.opinions_dirty);
    d.tasks_len = list(&p.action.tasks, &mut d.tasks, "tasks", |t: Task| EfTask {
        kind: i32::from(t.kind.0),
        started: i32::from(t.flags.started),
        completed: i32::from(t.flags.completed),
        has_peaceful_turns: i32::from(t.peaceful_turns.is_some()),
        has_kills: i32::from(t.kills.is_some()),
        has_conversions: i32::from(t.conversions.is_some()),
        has_explored_tiles: i32::from(t.explored_tiles.is_some()),
        pad: 0,
        peaceful_turns: t.peaceful_turns.unwrap_or(0),
        kills: t.kills.unwrap_or(0),
        conversions: t.conversions.unwrap_or(0),
        explored_tiles: t.explored_tiles.unwrap_or(0),
    })?;
    d.tribe = p.tribe;
    d.available_tech_len = list(&p.available_tech, &mut d.available_tech, "techs", |v| v)?;
    d.known_players_len = list(
        &p.known_players,
        &mut d.known_players,
        "known players",
        |v| v,
    )?;
    d.cities = p.cities;
    d.kills = p.kills;
    d.casualties = p.casualties;
    d.relations_len = list(&p.relations, &mut d.relation_ids, "relations", |(id, _)| id)?;
    list(
        &p.relations,
        &mut d.relations,
        "relations",
        |(_, r): (i32, Relation)| EfRelation {
            state: r.state,
            last_attack_turn: r.last_attack_turn,
            embassy_level: r.embassy_level,
            last_peace_broken_turn: r.last_peace_broken_turn,
            first_meet: r.first_meet,
            embassy_build_turn: r.embassy_build_turn,
            previous_attack_turn: r.previous_attack_turn,
        },
    )?;
    d.has_tech_cache = i32::from(p.unlocked_tech_cache.is_some());
    if let Some(cache) = &p.unlocked_tech_cache {
        d.tech_cache_len = list(cache, &mut d.tech_cache, "tech cache", |v| v)?;
    }
    d.start_tile = xy(p.start_tile);
    d.handicap = p.handicap;
    d.auto_play = i32::from(p.auto_play);
    d.messages_len = list(
        &p.messages,
        &mut d.message_senders,
        "messages",
        |m: Message| m.sender,
    )?;
    list(
        &p.messages,
        &mut d.message_kinds,
        "messages",
        |m: Message| m.kind,
    )?;
    d.built_unique_len = list(
        &p.built_unique_improvements,
        &mut d.built_unique,
        "monuments",
        |v| v,
    )?;
    d.block_train_units = i32::from(p.block_train_units);
    d.skin = p.skin;
    d.climate = p.climate;
    d.has_chosen_tribe = i32::from(p.has_chosen_tribe);
    d.score = p.score;
    d.resigned_at_command = p.resigned_at_command;
    d.wiped_at_command = p.wiped_at_command;
    d.end_score = p.end_score;
    d.color = p.color;
    d.tribe_mix = p.tribe_mix;
    d.resigned_turn = p.resigned_turn;
    d.wipe_outs = p.wipe_outs;
    d.killer_id = p.killer_id;
    d.killed_turn = p.killed_turn;
    d.capital_count_cache = p.capital_count_cache;
    Ok(())
}

fn count(n: i32, max: usize, what: &str) -> Result<usize, String> {
    usize::try_from(n)
        .ok()
        .filter(|&n| n <= max)
        .ok_or_else(|| format!("{what} out of range"))
}

/// The block as a game. What the block does not hold (`Side`, `Memory`, the checkpoint's
/// constant parts) comes from `template`, whose settings must be the block's. Units and
/// effect lists come out in slot order with the free slots left out.
pub fn from_device(d: &Block, template: &Game) -> Result<Game, String> {
    let settings = to_device_settings(template);
    let ours = [
        d.seats as i64,
        d.win_by_capital as i64,
        d.win_by_extermination as i64,
        d.achievements_active as i64,
        d.naming_case as i64,
        i64::from(d.version),
        d.rules_mode,
        d.game_type,
        d.turn_limit,
        d.score_limit,
        d.death_condition,
        d.action_limit,
    ];
    if settings != ours {
        return Err("the block's settings are not the template's".into());
    }
    let tiles_len = count(d.tile_count, d.tile_cap.max(0) as usize, "tile count")?;
    let players_len = count(d.player_count, MAX_PLAYERS, "player count")?;
    // free slots are left out, so the used ones are renumbered
    let mut unit_of = HashMap::new();
    let mut list_of = HashMap::new();
    let mut effect_lists = Vec::new();
    for (slot, l) in d.lists().iter().enumerate() {
        if l.used_n != 0 {
            list_of.insert(slot as i32, effect_lists.len() as u32);
            effect_lists.push(short(
                i32::from(l.len_n),
                &l.items_n,
                "unit effects",
                i64::from,
            )?);
        }
    }
    if list_of.get(&0) != Some(&0) {
        return Err("effect list 0 is not in use".into());
    }
    let used: Vec<usize> = (0..d.unit_cap as usize)
        .filter(|&s| d.units()[s].used_n != 0)
        .collect();
    for (new, &slot) in used.iter().enumerate() {
        unit_of.insert(slot as i32, new as u32);
    }
    let node = |slot: i32| -> Result<Option<u32>, String> {
        if slot == NONE {
            return Ok(None);
        }
        unit_of
            .get(&slot)
            .copied()
            .map(Some)
            .ok_or_else(|| format!("unit slot {slot} is free or out of range"))
    };
    let mut units = Vec::with_capacity(used.len());
    for &slot in &used {
        let u = &d.units()[slot];
        units.push(Unit {
            id: u.id,
            leader: u.leader,
            follower: u.follower,
            owner: u.owner_n.into(),
            kind: u.kind_n.into(),
            previous_turn_end_coordinates: back(u.previous_turn_end_coordinates),
            coordinates: back(u.coordinates),
            home: back(u.home),
            passenger: node(u.passenger_n.into())?,
            health: u.health_n.into(),
            promotion_level: u.promotion_level_n.into(),
            xp: u.xp_n.into(),
            moved: u.moved_n != 0,
            attacked: u.attacked_n != 0,
            direction: u.direction_n.into(),
            flipped: u.flipped_n != 0,
            created_turn: u.created_turn_n.into(),
            effects: *list_of
                .get(&i32::from(u.effects_n))
                .ok_or("a unit's effect list is free or out of range")?,
            is_temporary_explorer: u.is_temporary_explorer_n != 0,
            birth_climate: u.birth_climate_n.into(),
            birth_climate_skin: u.birth_climate_skin_n.into(),
        });
    }
    let mut tiles = Vec::with_capacity(tiles_len);
    for t in &d.tiles()[..tiles_len] {
        tiles.push(tile_from(t, node(t.unit_n.into())?)?);
    }
    let mut players = Vec::with_capacity(players_len);
    for p in &d.players[..players_len] {
        players.push(player_from(p)?);
    }
    let triggers = d.triggers[..count(d.triggers_len, MAX_TRIGGERS, "pending choices")?]
        .iter()
        .map(|t| Trigger {
            player: t.player,
            opponent: t.opponent,
            kind: t.kind,
            coordinates: back(t.coordinates),
        })
        .collect();
    let naming_count = count(d.naming_count, NAMING_SEEDS, "naming seed count")?;
    let naming_next = count(d.naming_next, naming_count, "naming seed position")?;
    let mut game = Game {
        width: d.width,
        tiles,
        units,
        players,
        current_turn: d.current_turn,
        current_player_index: d.current_player_index,
        current_unit_id: d.current_unit_id,
        current_state: d.current_state,
        current_command: d.current_command,
        needs_routes: d.needs_routes != 0,
        decisions: d.decisions,
        commands: d.commands,
        actions: d.actions[..count(d.actions_len, MAX_ACTIONS, "queued actions")?]
            .iter()
            .map(crate::action::from_device)
            .collect::<Result<_, _>>()?,
        triggers,
        effect_lists,
        memory: template.memory.clone(),
        seeds: Seeds {
            gameplay_seed: d.gameplay_seed,
            village_name_seed: d.village_name_seed,
            random_hash: (d.has_random_hash != 0).then_some(HashSeed {
                seed: d.random_hash,
            }),
            village_name_hash: (d.has_village_name_hash != 0).then_some(HashSeed {
                seed: d.village_name_hash,
            }),
        },
        naming_seeds: d.naming_seeds[naming_next..naming_count]
            .iter()
            .map(|&s| i64::from(s))
            .collect(),
        fixed: template.fixed.clone(),
        side: template.side.clone(),
    };
    // the continents a lost city made on the device join the template's list
    if let Some(mut list) = continents_of(template) {
        let count = usize::try_from(d.continent_count)
            .unwrap_or(0)
            .min(MAX_CONTINENTS);
        if count > list.len() {
            list.extend(d.continents[list.len()..count].iter().map(continent_from));
            game.set_continents(serde_json::to_value(&list).map_err(|e| e.to_string())?);
        }
    }
    Ok(game)
}

fn to_device_settings(g: &Game) -> [i64; 12] {
    let f = &g.fixed;
    [
        f.seats as i64,
        i64::from(f.win_by_capital),
        i64::from(f.win_by_extermination),
        i64::from(f.achievements_active),
        NAMING_CASES
            .iter()
            .position(|c| *c == f.naming_case)
            .unwrap_or(2) as i64,
        i64::from(f.version),
        f.rules_mode,
        f.game_type,
        f.turn_limit,
        f.score_limit,
        f.death_condition,
        f.action_limit as i64,
    ]
}

fn tile_from(t: &EfTile, unit: Option<u32>) -> Result<Tile, String> {
    let improvement = if t.has_improvement_n != 0 {
        let m = &t.improvement;
        let mut rewards = Counts::<14>::default();
        for (kind, &n) in m.rewards_n.iter().enumerate() {
            for _ in 0..n {
                rewards.try_push(kind as i32)?;
            }
        }
        Some(Improvement {
            kind: m.kind_n.into(),
            owner: m.owner_n.into(),
            founder: m.founder_n.into(),
            level: m.level_n.into(),
            founded: m.founded_n.into(),
            xp: m.xp_n.into(),
            population: m.population_n.into(),
            production: m.production_n.into(),
            base_score: m.base_score_n.into(),
            border_size: m.border_size_n.into(),
            upgrade: m.upgrade_n.into(),
            connected_to_capital_of_player: m.connected_to_capital_of_player_n.into(),
            named: m.named_n != 0,
            rewards,
            effects: short(
                m.effects_len_n.into(),
                &m.effects_n,
                "improvement effects",
                i32::from,
            )?,
        })
    } else {
        None
    };
    Ok(Tile {
        coordinates: back(t.coordinates),
        terrain: t.terrain_n.into(),
        climate: t.climate_n.into(),
        altitude: t.altitude_n.into(),
        owner: t.owner_n.into(),
        capital_of: t.capital_of_n.into(),
        explorers: short(
            t.explorers_len_n.into(),
            &t.explorers_n,
            "explorers",
            i32::from,
        )?,
        ruling_city: back(t.ruling_city),
        improvement,
        resource: (t.has_resource_n != 0).then_some(t.resource_n.into()),
        unit,
        has_road: t.has_road_n != 0,
        has_route: t.has_route_n != 0,
        continent: (t.has_continent_n != 0).then_some(t.continent_n.into()),
        had_route: t.had_route_n != 0,
        last_population_check: t.last_population_check_n.into(),
        available_population: t.available_population_n.into(),
        skin: t.skin_n.into(),
        effects: short(
            t.effects_len_n.into(),
            &t.effects_n,
            "tile effects",
            i32::from,
        )?,
    })
}

fn player_from(p: &EfPlayer) -> Result<Player, String> {
    let tasks = short(p.tasks_len, &p.tasks, "tasks", |t: EfTask| Task {
        kind: TaskKind(t.kind as u8),
        flags: TaskFlags {
            started: t.started != 0,
            completed: t.completed != 0,
        },
        peaceful_turns: (t.has_peaceful_turns != 0).then_some(t.peaceful_turns),
        kills: (t.has_kills != 0).then_some(t.kills),
        conversions: (t.has_conversions != 0).then_some(t.conversions),
        explored_tiles: (t.has_explored_tiles != 0).then_some(t.explored_tiles),
    })?;
    let relation_ids: Inline<i32, 5> = short(p.relations_len, &p.relation_ids, "relations", |v| v)?;
    let mut relations = Inline::new();
    for (i, &id) in relation_ids.iter().enumerate() {
        let r = &p.relations[i];
        relations.push((
            id,
            Relation {
                state: r.state,
                last_attack_turn: r.last_attack_turn,
                embassy_level: r.embassy_level,
                last_peace_broken_turn: r.last_peace_broken_turn,
                first_meet: r.first_meet,
                embassy_build_turn: r.embassy_build_turn,
                previous_attack_turn: r.previous_attack_turn,
            },
        ));
    }
    let senders: Inline<i32, 5> = short(p.messages_len, &p.message_senders, "messages", |v| v)?;
    let mut messages = Inline::new();
    for (i, &sender) in senders.iter().enumerate() {
        messages.push(Message {
            kind: p.message_kinds[i],
            sender,
        });
    }
    Ok(Player {
        action: PlayerAction {
            id: p.id,
            currency: p.currency,
            tasks,
            opinions_dirty: p.opinions_dirty != 0,
        },
        tribe: p.tribe,
        available_tech: short(p.available_tech_len, &p.available_tech, "techs", |v| v)?,
        known_players: short(
            p.known_players_len,
            &p.known_players,
            "known players",
            |v| v,
        )?,
        score: p.score,
        cities: p.cities,
        kills: p.kills,
        casualties: p.casualties,
        relations,
        unlocked_tech_cache: if p.has_tech_cache != 0 {
            Some(short(p.tech_cache_len, &p.tech_cache, "tech cache", |v| v)?)
        } else {
            None
        },
        start_tile: back(p.start_tile),
        handicap: p.handicap,
        auto_play: p.auto_play != 0,
        resigned_at_command: p.resigned_at_command,
        wiped_at_command: p.wiped_at_command,
        messages,
        built_unique_improvements: short(p.built_unique_len, &p.built_unique, "monuments", |v| v)?,
        block_train_units: p.block_train_units != 0,
        skin: p.skin,
        end_score: p.end_score,
        color: p.color,
        tribe_mix: p.tribe_mix,
        climate: p.climate,
        has_chosen_tribe: p.has_chosen_tribe != 0,
        resigned_turn: p.resigned_turn,
        wipe_outs: p.wipe_outs,
        killer_id: p.killer_id,
        killed_turn: p.killed_turn,
        capital_count_cache: p.capital_count_cache,
    })
}

/// The game with its units and effect lists numbered in the order the tiles reach them and
/// the ones nothing reaches left out: the form in which two states are compared.
pub fn canonical(g: &Game) -> Result<Game, String> {
    let r = reached(g)?;
    let mut out = g.clone();
    out.units = r
        .units
        .iter()
        .map(|&old| {
            let mut u = g.units[old as usize];
            u.passenger = u.passenger.map(|n| r.unit_of[&n] as u32);
            u.effects = r.list_of[&u.effects] as u32;
            u
        })
        .collect();
    out.effect_lists = r
        .lists
        .iter()
        .map(|&old| {
            if old == EMPTY_SLOT {
                Default::default()
            } else {
                g.effect_lists[old as usize]
            }
        })
        .collect();
    for tile in &mut out.tiles {
        tile.unit = tile.unit.map(|n| r.unit_of[&n] as u32);
    }
    Ok(out)
}

extern "C" {
    fn ef_twin_layout(out: *mut i64, capacity: i32) -> i32;
    fn ef_twin_game_digest(game: *const EfGame) -> u64;
    fn ef_twin_game_probe(game: *const EfGame, out: *mut i64);
}

#[cfg(feature = "cuda")]
extern "C" {
    fn ef_cuda_games_roundtrip(
        games: *const EfGame,
        count: i32,
        tile_cap: i32,
        unit_cap: i32,
        back: *mut EfGame,
        digests: *mut u64,
        probes: *mut i64,
    ) -> i32;
}

/// The C compiler's sizes and offsets for the block's structs, in the order
/// `tests/state_roundtrip.rs` lists them.
pub fn twin_layout() -> Vec<i64> {
    let mut out = vec![0; 64];
    // SAFETY: the C side writes at most `capacity` values and returns how many.
    let n = unsafe { ef_twin_layout(out.as_mut_ptr(), out.len() as i32) };
    out.truncate(n as usize);
    out
}

/// The host build's digest of the block.
pub fn twin_digest(d: &Block) -> u64 {
    // SAFETY: a valid block is passed by reference and only read.
    unsafe { ef_twin_game_digest(d.as_ptr()) }
}

/// The host build's values read through the block's fields: units on the board, the sum of
/// their health, the players' stars, the sum of tile terrain.
pub fn twin_probe(d: &Block) -> [i64; 4] {
    let mut out = [0; 4];
    // SAFETY: the C side reads the block and writes four values.
    unsafe { ef_twin_game_probe(d.as_ptr(), out.as_mut_ptr()) };
    out
}

/// Upload the blocks to the GPU, compute each one's digest and probe there, and download the
/// blocks again: (the blocks as they came back, digests, probes).
#[cfg(feature = "cuda")]
#[allow(clippy::type_complexity)] // the three results the doc comment names
pub fn device_roundtrip(games: &[Block]) -> Result<(Vec<Block>, Vec<u64>, Vec<[i64; 4]>), String> {
    let (flat, (tile_cap, unit_cap)) = pack(games);
    let mut back = vec![0u64; flat.len()];
    let mut digests = vec![0u64; games.len()];
    let mut probes = vec![[0i64; 4]; games.len()];
    // SAFETY: `flat` and `back` hold `count` blocks of these capacities each; `digests`
    // one value and `probes` four per block.
    let code = unsafe {
        ef_cuda_games_roundtrip(
            flat.as_ptr().cast(),
            games.len() as i32,
            tile_cap,
            unit_cap,
            back.as_mut_ptr().cast(),
            digests.as_mut_ptr(),
            probes.as_mut_ptr().cast(),
        )
    };
    if code != 0 {
        return Err(format!("CUDA error {code}"));
    }
    let mut blocks = games.to_vec();
    unpack(&back, &mut blocks);
    Ok((blocks, digests, probes))
}
