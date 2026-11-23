//! The game's tiles, units and effect lists, read like slices and written only through
//! `Game`'s accessors below, which stamp every tile a write can change. The kept legal list
//! (`menu_cache`) reads the stamps to learn which tiles changed since its last call without
//! comparing the board. The arrays'
//! contents are private to this module, so a write that does not stamp does not compile.
//!
//! Two stamps per tile: its fields (the Tile itself) and its unit (the unit standing there, its
//! passenger, and their effect lists). A stamp is the value of the board's write clock at the
//! tile's last write. Every array has an id, new whenever it is built or copied, so a reader
//! that sees another id knows nothing of the array's history.

use crate::state::{Game, Tile, Unit};
use serde::{Serialize, Serializer};
use std::sync::atomic::{AtomicU64, Ordering};

type Effects = crate::inline::Inline<i64, 9>;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn fresh_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// An array written only through `Game`.
pub struct Written<T> {
    items: Vec<T>,
    id: u64,
}

impl<T> Written<T> {
    /// new whenever the array is built or copied
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl<T> From<Vec<T>> for Written<T> {
    fn from(items: Vec<T>) -> Self {
        Written {
            items,
            id: fresh_id(),
        }
    }
}

impl<T> FromIterator<T> for Written<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        items.into_iter().collect::<Vec<T>>().into()
    }
}

impl<T: Clone> Clone for Written<T> {
    fn clone(&self) -> Self {
        self.items.clone().into()
    }
}

impl<T> Default for Written<T> {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl<T> std::ops::Deref for Written<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T: PartialEq> PartialEq for Written<T> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Written<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.items.fmt(f)
    }
}

impl<T: Serialize> Serialize for Written<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.items.serialize(s)
    }
}

impl<'a, T> IntoIterator for &'a Written<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

/// The tiles, with the two write stamps of each, and what the step's incremental passes keep
/// between calls (read against the stamps; not part of the state: equality ignores it).
pub struct Tiles {
    items: Written<Tile>,
    fields: Vec<u64>,
    units: Vec<u64>,
    clock: u64,
    /// shared between copies until one of them runs a pass, so a copy costs nothing more
    kept: std::sync::Arc<crate::routes::Kept>,
}

impl Tiles {
    /// new whenever the tiles are built or copied
    pub fn id(&self) -> u64 {
        self.items.id
    }
    /// the write clock: every stamp is at most this
    pub fn clock(&self) -> u64 {
        self.clock
    }
    /// the clock at the last write to tile `i`'s fields, and to its unit
    pub fn fields_stamp(&self, i: usize) -> u64 {
        self.fields[i]
    }
    pub fn unit_stamp(&self, i: usize) -> u64 {
        self.units[i]
    }
    fn stamp_fields(&mut self, i: usize) {
        self.clock += 1;
        self.fields[i] = self.clock;
    }
    fn stamp_unit(&mut self, i: usize) {
        self.clock += 1;
        self.units[i] = self.clock;
    }
    /// the passes' kept results, taken out while a pass reads the board (put them back)
    /// (in place: a new `Arc` per call would allocate twice per pass; a kept result shared
    /// with a copy of the game is copied first, as copy-on-write)
    pub(crate) fn take_kept(&mut self) -> crate::routes::Kept {
        std::mem::take(std::sync::Arc::make_mut(&mut self.kept))
    }
    pub(crate) fn put_kept(&mut self, kept: crate::routes::Kept) {
        *std::sync::Arc::make_mut(&mut self.kept) = kept;
    }
    /// what the passes kept, for counts
    pub fn kept(&self) -> &crate::routes::Kept {
        &self.kept
    }
    /// forget what the passes kept: the next call of each runs in full
    pub fn forget_kept(&mut self) {
        self.kept = Default::default();
    }
    fn stamp_all(&mut self) {
        self.clock += 1;
        let c = self.clock;
        self.fields.iter_mut().for_each(|s| *s = c);
        self.units.iter_mut().for_each(|s| *s = c);
    }
}

impl From<Vec<Tile>> for Tiles {
    fn from(items: Vec<Tile>) -> Self {
        let n = items.len();
        Tiles {
            items: items.into(),
            fields: vec![0; n],
            units: vec![0; n],
            clock: 0,
            kept: Default::default(),
        }
    }
}

impl Clone for Tiles {
    fn clone(&self) -> Self {
        Tiles {
            items: self.items.clone(),
            fields: self.fields.clone(),
            units: self.units.clone(),
            clock: self.clock,
            kept: self.kept.clone(),
        }
    }
}

impl Default for Tiles {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl std::ops::Deref for Tiles {
    type Target = [Tile];
    fn deref(&self) -> &[Tile] {
        &self.items.items
    }
}

impl PartialEq for Tiles {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl std::fmt::Debug for Tiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.items.fmt(f)
    }
}

impl Serialize for Tiles {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.items.serialize(s)
    }
}

impl<'a> IntoIterator for &'a Tiles {
    type Item = &'a Tile;
    type IntoIter = std::slice::Iter<'a, Tile>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.items.iter()
    }
}

/// The writes. Each stamps what it can change; when in doubt it stamps more.
impl Game {
    /// tile `i`, to change anything on it (its fields and its unit are stamped)
    pub fn tile_mut(&mut self, i: usize) -> &mut Tile {
        self.tiles.stamp_fields(i);
        self.tiles.stamp_unit(i);
        &mut self.tiles.items.items[i]
    }

    /// put `node` (or nothing) on tile `i`: only the tile's unit is stamped
    pub fn set_unit_at(&mut self, i: usize, node: Option<u32>) {
        self.tiles.stamp_unit(i);
        self.tiles.items.items[i].unit = node;
    }

    /// every tile (all stamped)
    pub fn tiles_mut(&mut self) -> &mut [Tile] {
        self.tiles.stamp_all();
        &mut self.tiles.items.items
    }

    /// unit `node`: the tile that holds it (standing there, or carrying it as a passenger) has
    /// its unit stamped. That is the tile at the unit's coordinates when the unit stands there;
    /// otherwise (a passenger keeps the coordinates where it embarked) the board is searched.
    /// A write that moves the unit also changes the tiles it leaves and enters, which
    /// `set_unit_at` stamps.
    pub fn unit_mut(&mut self, node: usize) -> &mut Unit {
        let c = self.units.items[node].coordinates;
        let here = self
            .index(c)
            .filter(|&i| self.tiles.items.items[i].unit == Some(node as u32));
        let holder = here.or_else(|| {
            let units = &self.units.items;
            self.tiles.items.items.iter().position(|x| {
                x.unit.is_some_and(|n| {
                    n as usize == node || units[n as usize].passenger == Some(node as u32)
                })
            })
        });
        if let Some(i) = holder {
            self.tiles.stamp_unit(i);
        }
        &mut self.units.items[node]
    }

    /// every unit (every tile's unit stamped)
    pub fn units_mut(&mut self) -> &mut [Unit] {
        self.tiles.clock += 1;
        let c = self.tiles.clock;
        self.tiles.units.iter_mut().for_each(|s| *s = c);
        &mut self.units.items
    }

    /// add a unit, standing nowhere yet; its node
    pub fn push_unit(&mut self, u: Unit) -> u32 {
        self.units.items.push(u);
        (self.units.items.len() - 1) as u32
    }

    /// effect list `list`: every tile whose unit or passenger holds it has its unit stamped
    /// (a list can be shared between units)
    pub fn effects_mut(&mut self, list: usize) -> Option<&mut Effects> {
        let holds = |u: &Unit| u.effects as usize == list;
        for i in 0..self.tiles.len() {
            let Some(n) = self.tiles.items.items[i].unit else {
                continue;
            };
            let u = &self.units.items[n as usize];
            let passenger = u.passenger.map(|p| &self.units.items[p as usize]);
            if holds(u) || passenger.is_some_and(holds) {
                self.tiles.stamp_unit(i);
            }
        }
        self.effect_lists.items.get_mut(list)
    }

    /// add an effect list, held by no unit yet; its index
    pub fn push_effects(&mut self, e: Effects) -> u32 {
        self.effect_lists.items.push(e);
        (self.effect_lists.items.len() - 1) as u32
    }

    /// remove the last effect list, stamping its holders like `effects_mut`
    pub fn pop_effects(&mut self) -> Option<Effects> {
        let last = self.effect_lists.items.len().checked_sub(1)?;
        self.effects_mut(last);
        self.effect_lists.items.pop()
    }
}

#[cfg(test)]
mod tests {
    use crate::menu_cache::Cache;
    use crate::reset::{reset, Config};
    use crate::tables::Tables;

    const CATALOG: &str = include_str!("../rules/rules-124.json");

    /// A write that skips the stamp, which only code in this module can make, is caught by the
    /// shadow mode; the same write through `tile_mut` is not.
    #[test]
    fn the_shadow_mode_catches_a_write_without_a_stamp() {
        let t = Tables::compile(CATALOG).unwrap();
        let config = Config {
            rules: 124,
            seed: 3,
            size: 11,
            map: 2,
            mode: 4,
            tribes: vec![7, 7],
            action_limit: 10,
        };
        let g = reset(&config, CATALOG, &t).unwrap();
        let mover = crate::step::mover(&g);
        let i = (0..g.tiles.len())
            .find(|&i| g.tiles[i].explorers.contains(&mover) && g.tiles[i].unit.is_none())
            .unwrap();
        let mut stamped = g.clone();
        let mut cache = Cache::new();
        cache.checked(&stamped, &t).unwrap();
        stamped.tile_mut(i).has_road = !stamped.tiles[i].has_road;
        assert!(cache.checked(&stamped, &t).is_ok());
        let mut unstamped = g.clone();
        let mut cache = Cache::new();
        cache.checked(&unstamped, &t).unwrap();
        unstamped.tiles.items.items[i].has_road = !unstamped.tiles[i].has_road;
        let e = cache.checked(&unstamped, &t).unwrap_err();
        assert!(e.starts_with("unmarked write"), "{e}");
    }

    /// A passenger keeps the coordinates where it embarked; a write to it stamps the tile of
    /// the carrier that holds it.
    #[test]
    fn a_write_to_a_passenger_stamps_its_carrier() {
        let t = Tables::compile(CATALOG).unwrap();
        let config = Config {
            rules: 124,
            seed: 3,
            size: 11,
            map: 2,
            mode: 4,
            tribes: vec![7, 7],
            action_limit: 10,
        };
        let mut g = reset(&config, CATALOG, &t).unwrap();
        let carrier = (0..g.tiles.len())
            .find(|&i| g.tiles[i].unit.is_some())
            .unwrap();
        let node = g.tiles[carrier].unit.unwrap();
        // a passenger whose coordinates are another tile's, as after its carrier moved
        let mut p = g.units[node as usize];
        let elsewhere = (carrier + 1) % g.tiles.len();
        p.coordinates = g.tiles[elsewhere].coordinates;
        let passenger = g.push_unit(p);
        g.unit_mut(node as usize).passenger = Some(passenger);
        let before = g.tiles.unit_stamp(carrier);
        g.unit_mut(passenger as usize).health -= 10;
        assert!(g.tiles.unit_stamp(carrier) > before);
    }
}
