//! One generated map candidate on the game state (map generation, noise path): placement sites,
//! noise path: placement sites, terrain from noise with capitals and
//! villages, climates, terrain kinds, resources, ruins, starfish and shorelines. The
//! candidate's random stream is consumed in the game's order.
use super::placement::remove_indices_near_index;
use super::{capital_neighbor_indices, MapGenerationContext, MapGeneratorSettings};
use crate::action::Action;
use crate::random::HashFunction;
use crate::state::{Game, Improvement, Shoreline, Shorelines, Tile, Xy};
use crate::step::player_index;
use crate::tables::Tables;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

mod islands;

const NULL: Xy = Xy { x: -1, y: -1 };
const WATER: i32 = 1;
const OCEAN: i32 = 2;
const FIELD: i32 = 3;
const MOUNTAIN: i32 = 4;
const FOREST: i32 = 5;
const ICE: i32 = 6;
const CITY: i32 = 1;
const RUIN: i32 = 2;
const FUNGI: i32 = 37;
const LIGHTHOUSE: i32 = 47;
const WHALE: i32 = 4;
const STARFISH: i32 = 8;
const SWAMPED: i32 = 2;
const SWAMP_SKIN: i32 = 17;

/// A continent of the board (`board.continents`)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Continent {
    #[serde(default)]
    pub detached: bool,
    pub tiles: Vec<Xy>,
    pub climate: i32,
    pub skin: i32,
    pub crop: f32,
    pub fish: f32,
    pub fruit: f32,
    pub game: f32,
    pub metal: f32,
    pub whale: f32,
    pub spores: f32,
    pub aquacrop: f32,
    pub water: f32,
    pub ocean: f32,
    pub field: f32,
    pub mountain: f32,
    pub forest: f32,
    pub ice: f32,
    pub has_alien_climate: bool,
    pub land_tile_count: i32,
    pub number_of_capitals: i32,
    pub max_size: i32,
}

pub fn fresh_continent() -> Continent {
    Continent {
        detached: false,
        tiles: vec![],
        climate: 0,
        skin: 0,
        crop: 0.5,
        fish: 0.5,
        fruit: 0.5,
        game: 0.5,
        metal: f32::from_bits(0x3f4ccccd),
        whale: 0.0,
        spores: 0.0,
        aquacrop: 0.0,
        water: 0.0,
        ocean: 0.0,
        field: 0.5,
        mountain: f32::from_bits(0x3e19999a),
        forest: f32::from_bits(0x3ecccccd),
        ice: 0.5,
        has_alien_climate: false,
        land_tile_count: 0,
        number_of_capitals: 0,
        max_size: 0,
    }
}

/// A cleared tile: water, no owner, empty shorelines
pub fn cleared_tile(coordinates: Xy) -> Tile {
    Tile {
        coordinates,
        terrain: WATER,
        climate: 0,
        altitude: -1,
        owner: 0,
        capital_of: 0,
        explorers: Default::default(),
        ruling_city: NULL,
        improvement: None,
        resource: None,
        unit: None,
        has_road: false,
        has_route: false,
        continent: None,
        had_route: false,
        last_population_check: 0,
        available_population: 0,
        skin: 0,
        effects: Default::default(),
    }
}

/// A cleared tile's shorelines: none drawn.
pub fn cleared_shorelines() -> Shorelines {
    let edge = || Shoreline {
        visible: false,
        sprite_extension: None,
    };
    Shorelines {
        any: false,
        north: edge(),
        south: edge(),
        east: edge(),
        west: edge(),
    }
}

/// A neutral level 1 city
fn village() -> Improvement {
    Improvement {
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
    }
}

fn set_tile_as_city(tile: &mut Tile) {
    tile.owner = 0;
    tile.terrain = FIELD;
    tile.resource = None;
    tile.improvement = Some(village());
}

fn is_water(terrain: i32) -> bool {
    terrain == WATER || terrain == OCEAN
}

fn height(g: &Game) -> i32 {
    g.tiles.len() as i32 / g.width
}

/// A radius-1 area's tiles held in place.
type Around = crate::inline::Inline<usize, 8>;

/// the tiles of the square of `radius` around `center` without it, x outer and y inner (as
/// the game does), as `Game::index` finds them. It holds the board's shape, not the board, so
/// caller may change tiles while it runs.
fn area(g: &Game, center: Xy, radius: i32) -> impl Iterator<Item = usize> + Clone {
    let (width, len) = (g.width, g.tiles.len());
    (-radius..=radius)
        .flat_map(move |dx| (-radius..=radius).map(move |dy| (dx, dy)))
        .filter(|&d| d != (0, 0))
        .filter_map(move |(dx, dy)| {
            let (x, y) = (center.x + dx, center.y + dy);
            let i = (y * width + x) as usize;
            (x >= 0 && y >= 0 && x < width && i < len).then_some(i)
        })
}

fn corners(g: &Game) -> [Xy; 4] {
    let (x, y) = (g.width - 1, height(g) - 1);
    [
        Xy { x, y },
        Xy { x: 0, y: 0 },
        Xy { x: 0, y },
        Xy { x, y: 0 },
    ]
}

fn start_tile(g: &Game, i: usize) -> Result<Xy, String> {
    Ok(g.players[i].start_tile)
}

fn tribe_has(t: &Tables, tribe: i32, ability: &str) -> bool {
    t.tribes
        .get(tribe as usize)
        .is_some_and(|d| d.tribe_abilities.has(ability))
}

/// The climate: a player without a climate takes its tribe's
fn player_climate(g: &mut Game, i: usize) -> Result<i32, String> {
    if g.players[i].climate == 0 {
        g.players[i].climate = g.players[i].tribe;
    }
    Ok(g.players[i].climate)
}

/// Ruling the area, immediate form: the city claims every unruled tile in its border
fn rule_area(
    g: &mut Game,
    t: &Tables,
    player: i32,
    city: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let ci = g.index(city).ok_or("border rule: city tile")?;
    let radius = g.tiles[ci]
        .improvement
        .as_ref()
        .ok_or("border rule: missing city improvement")?
        .border_size;
    let pi = player_index(g, player)?;
    for x in (city.x - radius).max(0)..=(city.x + radius).min(g.width - 1) {
        for y in (city.y - radius).max(0)..=(city.y + radius).min(height(g) - 1) {
            let c = Xy { x, y };
            let Some(i) = g.index(c) else { continue };
            if g.tiles[i].ruling_city == NULL || g.tiles[i].ruling_city == city {
                let old = g.tiles[i].owner;
                let is_city = g.tiles[i]
                    .improvement
                    .as_ref()
                    .is_some_and(|m| m.kind == CITY);
                if g.player(old).is_some() {
                    return Err(
                        "claiming a tile from another player at map generation not implemented"
                            .into(),
                    );
                }
                g.tiles[i].owner = player;
                g.tiles[i].ruling_city = city;
                if is_city {
                    g.players[pi].cities = g.players[pi].cities.wrapping_add(1);
                }
                if old != player {
                    let amount = 20i32.wrapping_add(crate::economy::improvement_score(t, g, i));
                    crate::step::add_score(g, player, i64::from(amount), out)?;
                }
            }
            if !crate::explore::explored(g, i, player) {
                crate::step::add_score(g, player, 5, out)?;
                g.tiles[i].explorers.push(player);
            }
            crate::economy::surrounding(g, t, player, c, out)?;
        }
    }
    Ok(())
}

/// Exploring from a tile, immediate form: score and mark every unexplored tile in sight, far
/// to near, then queue the contacts those tiles show
fn explore_from_tile(
    g: &mut Game,
    player: i32,
    center: Xy,
    sight: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let pi = player_index(g, player)?;
    let ci = g.index(center).ok_or("exploration origin")?;
    let radius = if g.tiles[ci].terrain == MOUNTAIN {
        2
    } else {
        sight
    };
    let area = crate::explore::area_sorted(g, center, radius)?;
    for &i in area.iter().rev() {
        if crate::explore::explored(g, i, player) {
            continue;
        }
        crate::step::add_score(g, player, 5, out)?;
        if !g.tiles[i].explorers.contains(&player) {
            g.tiles[i].explorers.push(player);
        }
    }
    if player != 255 && (g.players[pi].known_players.len() as i32) < g.players.len() as i32 - 1 {
        for &i in area.iter() {
            if !crate::explore::explored(g, i, player) {
                continue;
            }
            let c = g.tiles[i].coordinates;
            let unit_owner = crate::explore::visible_unit_owner(g, player, i)?;
            let unit_contact = match unit_owner {
                Some(o) if o != player => !crate::explore::knows(g, player, o)?,
                _ => false,
            };
            let city_owner = g.tiles[i]
                .improvement
                .as_ref()
                .filter(|m| m.kind == CITY)
                .map(|_| g.tiles[i].owner);
            let city_contact = match city_owner {
                Some(o) if o != 0 && o != player => !crate::explore::knows(g, player, o)?,
                _ => false,
            };
            if unit_contact {
                out.push(crate::explore::meet(player, unit_owner.unwrap(), c));
            }
            if city_contact && (!unit_contact || unit_owner != city_owner) {
                out.push(crate::explore::meet(player, city_owner.unwrap(), c));
            }
        }
    }
    Ok(())
}

/// Setting a tile as capital: the player's named capital, its territory and first sight
fn set_tile_as_capital(
    g: &mut Game,
    t: &Tables,
    player: i32,
    target: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let Some(ti) = g.index(target) else {
        return Ok(());
    };
    let pi = player_index(g, player).map_err(|_| "SetTileAsCapital null player")?;
    let tribe = g.players[pi].tribe;
    if tribe == 0 {
        return Ok(());
    }
    let Some(bonus) = t.tribes.get(tribe as usize).map(|d| d.bonus) else {
        return Ok(());
    };
    g.players[pi].start_tile = target;
    g.players[pi].cities = 0;
    let climate = player_climate(g, pi)?;
    let skin = g.players[pi].skin;
    let tile = &mut g.tiles[ti];
    tile.terrain = FIELD;
    tile.climate = climate;
    tile.skin = skin;
    tile.altitude = 1;
    tile.resource = None;
    tile.unit = None;
    let mut city = village();
    let name = crate::capture::city_name(g, t, tribe, i64::from(skin), target)?;
    if bonus == 1 {
        city.level = 3;
    }
    g.tiles[ti].improvement = Some(city);
    g.name_city(ti, name);
    g.tiles[ti].capital_of = player;
    if bonus == 2 {
        if let Some(i) = area(g, target, 1).find(|&i| g.tiles[i].terrain == FIELD) {
            let mut fungi = village();
            fungi.kind = FUNGI;
            fungi.level = 0;
            fungi.production = 0;
            fungi.border_size = 0;
            fungi.founded = g.current_turn;
            fungi.founder = g.tiles[i].owner;
            g.tiles[i].improvement = Some(fungi);
        }
    }
    rule_area(g, t, player, target, out)?;
    explore_from_tile(g, player, target, 2, out)?;
    Ok(())
}

/// The terrain thresholds of the noise: nearest-even quantiles of the sorted noise
fn thresholds(noise: &[f32], wetness: f32, shallows: f32) -> Result<(f32, f32), String> {
    if noise.is_empty() || noise.iter().any(|x| !x.is_finite()) {
        return Err("noise shape".into());
    }
    let mut sorted = noise.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let position = ((noise.len() - 1) as f32) * wetness;
    let index = f64::from(position).round_ties_even() as i64;
    if index < 0 || index as usize >= noise.len() {
        return Err("noise quantile".into());
    }
    let lower = f64::from((index as f32) * (1.0_f32 - shallows)).round_ties_even() as i64;
    if lower < 0 || lower as usize >= noise.len() {
        return Err("noise quantile".into());
    }
    Ok((sorted[index as usize], sorted[lower as usize]))
}

fn modify(default: f32, modifier: f32) -> f32 {
    if default > 0.0 {
        default * modifier
    } else {
        modifier
    }
}

fn number(v: &Value, what: &str) -> Result<f32, String> {
    v.as_f64()
        .map(|x| x as f32)
        .ok_or_else(|| format!("{what} contains a non-number"))
}

/// Assigning a player to a continent: it takes the player's climate and tribe modifiers
pub(crate) fn set_continent_player(
    g: &mut Game,
    t: &Tables,
    c: &mut Continent,
    pi: usize,
    fallback: bool,
) -> Result<(), String> {
    let tribe = g.players[pi].tribe;
    if tribe == 0 {
        return Ok(());
    }
    let Some(data) = t.tribes.get(tribe as usize) else {
        return Ok(());
    };
    apply_tribe(t, data, c)?;
    if c.has_alien_climate && fallback {
        c.climate = 7; // Imperius
        c.skin = 0;
    } else {
        c.climate = player_climate(g, pi)?;
        c.skin = g.players[pi].skin;
    }
    c.tiles.push(start_tile(g, pi)?);
    Ok(())
}

/// A fresh continent with `tribe`'s part of the player assignment applied (what a lost
/// city's continent takes from its tribe), or None for no tribe or one the catalog lacks
pub fn tribe_continent(t: &Tables, tribe: i32) -> Result<Option<Continent>, String> {
    let Some(data) = t.tribes.get(tribe as usize).filter(|_| tribe != 0) else {
        return Ok(None);
    };
    let mut c = fresh_continent();
    apply_tribe(t, data, &mut c)?;
    Ok(Some(c))
}

/// The tribe's part of the player assignment: the alien-climate flag, Aquacrop's base and
/// the tribe's terrain and resource modifiers
fn apply_tribe(
    t: &Tables,
    data: &crate::tables::TribeData,
    c: &mut Continent,
) -> Result<(), String> {
    c.has_alien_climate = data.tribe_abilities.has("alienclimate");
    c.aquacrop = f32::from_bits(0x3e99999a);
    for (name, value) in &data.terrain_modifier {
        let slot = match t.terrain.id(name)? {
            1 => &mut c.water,
            2 => &mut c.ocean,
            3 => &mut c.field,
            4 => &mut c.mountain,
            5 => &mut c.forest,
            6 => &mut c.ice,
            _ => continue,
        };
        *slot = modify(*slot, number(value, "terrainModifier")?);
    }
    for (name, value) in &data.resource_modifier {
        let slot = match t.resources.id(name)? {
            1 => &mut c.game,
            2 => &mut c.crop,
            3 => &mut c.fish,
            4 => &mut c.whale,
            5 => &mut c.metal,
            6 => &mut c.fruit,
            7 => &mut c.spores,
            9 => &mut c.aquacrop,
            _ => continue,
        };
        *slot = modify(*slot, number(value, "resourceModifier")?);
    }
    Ok(())
}

fn resource_probability(c: &Continent, kind: i32) -> f32 {
    match kind {
        1 => c.game,
        2 => c.crop,
        3 => c.fish,
        4 => c.whale,
        5 => c.metal,
        6 => c.fruit,
        7 => c.spores,
        9 => c.aquacrop,
        _ => 0.0,
    }
}

struct Candidate<'a> {
    g: &'a mut Game,
    t: &'a Tables,
    rng: MapGenerationContext,
    continents: Vec<Continent>,
}

impl Candidate<'_> {
    fn postterrain_cities(&mut self, budget: i32) -> Result<(), String> {
        let (width, h) = (self.g.width, height(self.g));
        let g = &mut *self.g;
        let mut candidates = Vec::new();
        for (i, tile) in g.tiles.iter().enumerate() {
            let c = tile.coordinates;
            if is_water(tile.terrain)
                || c.x < 1
                || c.y < 1
                || c.x >= width - 1
                || c.y >= h - 1
                || tile.improvement.is_some()
            {
                continue;
            }
            if area(g, c, 2).all(|j| g.tiles[j].improvement.is_none()) {
                candidates.push(i as i32);
            }
        }
        for _ in 0..budget {
            if candidates.is_empty() {
                break;
            }
            let position = self
                .rng
                .next_range(0, candidates.len() as i32)
                .map_err(|e| format!("{e:?}"))? as usize;
            let site = candidates[position];
            set_tile_as_city(&mut g.tiles[site as usize]);
            remove_indices_near_index(&mut candidates, site, width, 2)
                .map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }

    /// Adds the climates for a fresh noise map
    fn climates(&mut self, settings: &MapGeneratorSettings) -> Result<(), String> {
        let count = self.g.players.len() - 1;
        if !(2..=4).contains(&count) {
            return Err("map setup supports 2 to 4 players".into());
        }
        // island continents the board still refers to are kept, after the climate ones
        let mut referenced = vec![false; self.continents.len()];
        for id in self.g.tiles.iter().filter_map(|x| x.continent) {
            *referenced
                .get_mut(id as usize)
                .ok_or("invalid island continent reference")? = true;
        }
        let mut fallback = true;
        for p in self.g.players.iter().filter(|p| p.action.id != 255) {
            if self.t.tribes.get(p.tribe as usize).is_some()
                && !tribe_has(self.t, p.tribe, "alienclimate")
            {
                fallback = false;
                break;
            }
        }
        let mut continents = Vec::with_capacity(count);
        for pi in 0..count {
            let mut c = fresh_continent();
            set_continent_player(self.g, self.t, &mut c, pi, fallback)?;
            if settings.wetness == 0.0 {
                c.water = 0.0;
                c.ocean = 0.0;
                c.fish = 0.0;
                c.whale = 0.0;
            }
            continents.push(c);
        }
        let mut remap = vec![0; referenced.len()];
        for (id, mut old) in std::mem::take(&mut self.continents).into_iter().enumerate() {
            if referenced[id] {
                remap[id] = continents.len() as i32;
                old.detached = true;
                continents.push(old);
            }
        }
        for tile in &mut self.g.tiles {
            if let Some(id) = tile.continent {
                tile.continent = Some(remap[id as usize]);
            }
        }
        self.continents = continents;
        let g = &mut *self.g;
        let total = g.tiles.len() as i32;
        let mut assigned = count as i32;
        let mut budget = total.wrapping_mul(10);
        let mut id = 0;
        while assigned < total && budget > 0 {
            if self.continents[id].tiles.is_empty()
                || (self.continents[id].has_alien_climate && !fallback)
            {
                budget -= 1;
            } else {
                let source = self
                    .rng
                    .next_range(0, self.continents[id].tiles.len() as i32)
                    .map_err(|e| format!("{e:?}"))? as usize;
                let origin = self.continents[id].tiles[source];
                g.index(origin).ok_or("climate origin tile missing")?;
                let around = area(g, origin, 1);
                let mut candidates: Around = around
                    .clone()
                    .filter(|&i| g.tiles[i].climate == 0 && g.tiles[i].altitude >= 0)
                    .collect();
                if candidates.is_empty() {
                    candidates = around.filter(|&i| g.tiles[i].climate == 0).collect();
                }
                if candidates.is_empty() {
                    // the game removes the origin's first entry. A tile joins a continent
                    // here only while it has no climate, so once; only the continent's start
                    // tile, entered first, can be listed twice.
                    let tiles = &mut self.continents[id].tiles;
                    let first = if origin == tiles[0] { 0 } else { source };
                    tiles.remove(first);
                    budget -= 1;
                    continue;
                }
                let chosen = candidates[self
                    .rng
                    .next_range(0, candidates.len() as i32)
                    .map_err(|e| format!("{e:?}"))?
                    as usize];
                let tile = &mut g.tiles[chosen];
                tile.climate = self.continents[id].climate;
                tile.skin = self.continents[id].skin;
                tile.continent = Some(id as i32);
                if tile.altitude >= 0 {
                    tile.altitude = 1;
                    tile.terrain = FIELD;
                    tile.resource = None;
                }
                self.continents[id].tiles.push(tile.coordinates);
                assigned += 1;
            }
            id += 1;
            if id == count {
                id = 0;
            }
        }
        Ok(())
    }

    /// Adds the terrain: field, mountain and forest by each continent's weights
    fn terrain(&mut self) -> Result<(), String> {
        let g = &mut *self.g;
        for c in self.continents.iter_mut().filter(|c| !c.detached) {
            c.tiles.clear();
            c.land_tile_count = 0;
        }
        for tile in &g.tiles {
            let Some(id) = tile.continent else { continue };
            let c = self
                .continents
                .get_mut(id as usize)
                .ok_or("invalid continent reference")?;
            c.tiles.push(tile.coordinates);
            if !is_water(tile.terrain) && tile.improvement.is_none() {
                c.land_tile_count = c.land_tile_count.wrapping_add(1);
            }
        }
        for c in self.continents.iter().filter(|c| !c.detached) {
            let total = (c.forest + c.field) + c.mountain;
            let mut counts = [0_i32; 3];
            for (count, weight) in counts.iter_mut().zip([c.field, c.mountain, c.forest]) {
                let quota = (weight / total) * c.land_tile_count as f32;
                if quota > 0.0 {
                    *count = quota.ceil() as i32;
                }
            }
            for &coord in &c.tiles {
                let i = g.index(coord).ok_or("missing continent tile")?;
                let tile = &mut g.tiles[i];
                if is_water(tile.terrain) || tile.improvement.is_some() {
                    continue;
                }
                tile.resource = None;
                let remaining: i32 = counts.iter().sum();
                if remaining == 0 {
                    break;
                }
                let pick = self
                    .rng
                    .next_range(0, remaining)
                    .map_err(|e| format!("{e:?}"))?;
                let group = if pick < counts[0] {
                    0
                } else if pick < counts[0] + counts[1] {
                    1
                } else {
                    2
                };
                counts[group] -= 1;
                tile.terrain = [FIELD, MOUNTAIN, FOREST][group];
                tile.altitude = if tile.terrain == MOUNTAIN { 2 } else { 1 };
            }
        }
        Ok(())
    }

    /// Makes the ocean: water next to land is shallow, the rest ocean; a resource the new
    /// terrain does not allow is removed
    fn make_ocean(&mut self, convert_shallows: bool) -> Result<(), String> {
        let (g, t) = (&mut *self.g, self.t);
        let width = g.width as usize;
        let water = |i: usize| is_water(g.tiles[i].terrain);
        let mut changes = Vec::new();
        for (i, tile) in g.tiles.iter().enumerate() {
            if !water(i) {
                continue;
            }
            let near_land = (i + width < g.tiles.len() && !water(i + width))
                || (i > width && !water(i - width))
                || (i % width != 0 && !water(i - 1))
                || (i % width + 1 < width && !water(i + 1));
            let (terrain, altitude) = if near_land {
                (WATER, -1)
            } else if convert_shallows {
                (OCEAN, -2)
            } else {
                (tile.terrain, tile.altitude)
            };
            let mut resource = tile.resource;
            if let Some(kind) = resource {
                if kind == 0 {
                    return Err("null ocean resource data".into());
                }
                // a resource without a terrain list is unrestricted (no catalog entry has an empty one)
                let allowed = t.resource_terrains(kind)?;
                if !allowed.is_empty() && !allowed.contains(&terrain) {
                    resource = None;
                }
            }
            changes.push((i, terrain, altitude, resource));
        }
        for (i, terrain, altitude, resource) in changes {
            let tile = &mut g.tiles[i];
            tile.terrain = terrain;
            tile.altitude = altitude;
            tile.resource = resource;
        }
        Ok(())
    }

    fn lighthouses(&mut self) -> Result<(), String> {
        let g = &mut *self.g;
        for c in corners(g) {
            let i = g.index(c).ok_or("missing lighthouse corner")?;
            let tile = &mut g.tiles[i];
            let mut light = village();
            light.kind = LIGHTHOUSE;
            light.border_size = 0;
            light.production = 0;
            tile.improvement = Some(light);
            if !is_water(tile.terrain) {
                tile.terrain = FIELD;
            }
        }
        Ok(())
    }

    /// Adds one resource with the generator's own hash
    fn add_resource(&mut self, i: usize, resources: &[i32]) -> Result<(), String> {
        let hash = HashFunction((self.rng.next_float() * 1000.0_f32) as i32);
        place_resource(self.g, self.t, i, resources, &self.continents, hash)
    }

    /// Adds the resources: around every city, radius 1 at `richness`, then radius 2 at a third
    fn resources(&mut self, richness: f32) -> Result<(), String> {
        let resources: Vec<i32> = (0..self.t.resources.len())
            .filter(|&k| self.t.resources.get(k).is_some())
            .map(|k| k as i32)
            .collect();
        let cities: Vec<usize> = (0..self.g.tiles.len())
            .filter(|&i| {
                self.g.tiles[i]
                    .improvement
                    .as_ref()
                    .is_some_and(|m| m.kind == CITY)
            })
            .collect();
        let mut visited = vec![false; self.g.tiles.len()];
        for &city in &cities {
            for i in area(self.g, self.g.tiles[city].coordinates, 1) {
                let value = self.rng.next_float();
                if value < richness
                    && !visited[i]
                    && self.g.tiles[i].resource.is_none()
                    && self.g.tiles[i].improvement.is_none()
                {
                    self.add_resource(i, &resources)?;
                }
                visited[i] = true;
            }
        }
        let richness = richness / 3.0_f32;
        for &city in &cities {
            for i in area(self.g, self.g.tiles[city].coordinates, 2) {
                if self.g.tiles[i].improvement.is_none() {
                    let value = self.rng.next_float();
                    if value < richness && !visited[i] && self.g.tiles[i].resource.is_none() {
                        self.add_resource(i, &resources)?;
                    }
                }
                visited[i] = true;
            }
        }
        Ok(())
    }

    /// Places the declared starting resources around the player's capital
    fn starting_resources(&mut self, pi: usize) -> Result<(), String> {
        let (g, t) = (&mut *self.g, self.t);
        let tribe = t
            .tribes
            .get(g.players[pi].tribe as usize)
            .ok_or("unknown tribe")?;
        let mut requests = Vec::new();
        for name in &tribe.starting_resource {
            requests.push(t.resources.id(name)? as i32);
        }
        if requests.is_empty() {
            return Err("starting resources from technologies not implemented".into());
        }
        let mut candidates: Vec<usize> = area(g, start_tile(g, pi)?, 1).collect();
        self.rng
            .shuffle(&mut candidates)
            .map_err(|e| format!("{e:?}"))?;
        candidates.retain(|&i| {
            let matching = g.tiles[i]
                .resource
                .and_then(|existing| requests.iter().position(|&r| r == existing));
            if let Some(k) = matching {
                requests.remove(k);
                false
            } else {
                true
            }
        });
        let mut placements = Vec::new();
        for pass in 0..3 {
            let mut k = 0;
            while k < requests.len() {
                let kind = requests[k];
                let choices = t.resource_terrains(kind)?;
                let selected = self
                    .rng
                    .next_range(0, choices.len() as i32)
                    .map_err(|e| format!("{e:?}"))? as usize;
                let terrain = *choices
                    .get(selected)
                    .ok_or("starting resource has no chosen terrain")?;
                let candidate = candidates.iter().position(|&i| match pass {
                    0 => g.tiles[i].terrain == terrain,
                    1 => !is_water(g.tiles[i].terrain),
                    _ => true,
                });
                if let Some(position) = candidate {
                    placements.push((candidates.remove(position), terrain, requests.remove(k)));
                } else {
                    k += 1;
                }
            }
        }
        for (i, terrain, kind) in placements {
            g.tiles[i].terrain = terrain;
            g.tiles[i].resource = Some(kind);
        }
        Ok(())
    }

    fn ruins(&mut self, amount: i32) -> Result<(), String> {
        let g = &mut *self.g;
        let mut order: Vec<usize> = (0..g.tiles.len()).collect();
        self.rng.shuffle(&mut order).map_err(|e| format!("{e:?}"))?;
        let corners = corners(g);
        let (mut placed, mut water_count) = (0_i32, 0_i32);
        for i in order {
            let tile = &g.tiles[i];
            let c = tile.coordinates;
            let water = is_water(tile.terrain);
            if corners.contains(&c) || (water && water_count > amount) {
                continue;
            }
            if water
                && (tile.owner != 0
                    || tile.terrain != OCEAN
                    || c.x <= 0
                    || c.y <= 0
                    || c.x >= g.width - 1
                    || c.y >= height(g) - 1)
            {
                continue;
            }
            let blocked = area(g, c, 2).chain(std::iter::once(i)).any(|j| {
                let n = &g.tiles[j];
                match n.improvement.as_ref().map(|m| m.kind) {
                    Some(RUIN) => true,
                    Some(CITY) => {
                        water
                            || n.owner != 0
                            || (n.coordinates.x.abs_diff(c.x) <= 1
                                && n.coordinates.y.abs_diff(c.y) <= 1)
                    }
                    _ => false,
                }
            });
            if blocked {
                continue;
            }
            let mut ruin = village();
            ruin.kind = RUIN;
            ruin.border_size = 0;
            g.tiles[i].improvement = Some(ruin);
            placed += 1;
            water_count += i32::from(water);
            if placed > amount {
                break;
            }
        }
        Ok(())
    }

    fn starfish(&mut self, amount: f32) -> Result<(), String> {
        let g = &mut *self.g;
        let count = g.tiles.len() as i32;
        let mut placed = 0;
        if amount <= 0.0 {
            return Ok(());
        }
        for _ in 0..count {
            let i = self
                .rng
                .next_range(0, count)
                .map_err(|e| format!("{e:?}"))? as usize;
            let tile = &g.tiles[i];
            if !is_water(tile.terrain) || tile.improvement.is_some() {
                continue;
            }
            if area(g, tile.coordinates, 1).any(|j| {
                g.tiles[j].resource == Some(STARFISH)
                    || g.tiles[j]
                        .improvement
                        .as_ref()
                        .is_some_and(|m| m.kind == CITY)
            }) {
                continue;
            }
            g.tiles[i].resource = Some(STARFISH);
            placed += 1;
            if amount <= placed as f32 {
                break;
            }
        }
        Ok(())
    }

    fn shorelines(&mut self) {
        generate_shorelines(&mut *self.g);
    }
}

/// Adds one resource with hash `hash`: one hash per tile decides which resources pass and
/// which is kept; a tile without a continent turns to ice
pub(crate) fn place_resource(
    g: &mut Game,
    t: &Tables,
    i: usize,
    resources: &[i32],
    continents: &[Continent],
    hash: HashFunction,
) -> Result<(), String> {
    let tile = &g.tiles[i];
    if tile.improvement.is_some() {
        return Ok(());
    }
    let Some(continent) = tile.continent else {
        g.tiles[i].terrain = ICE;
        return Ok(());
    };
    let continent = continents
        .get(continent as usize)
        .ok_or("resource continent missing")?;
    let (c, terrain) = (tile.coordinates, tile.terrain);
    let mut candidates = Vec::new();
    for (counter, &kind) in resources.iter().enumerate() {
        let probability = resource_probability(continent, kind);
        if probability == 0.0 || !(hash.value(&[c.x, c.y, counter as i32]) <= probability) {
            continue;
        }
        if kind == WHALE
            && area(g, c, 1).any(|j| {
                g.tiles[j].resource == Some(WHALE)
                    || g.tiles[j]
                        .improvement
                        .as_ref()
                        .is_some_and(|m| m.kind == CITY)
            })
        {
            continue;
        }
        let allowed = t.resource_terrains(kind)?;
        if allowed.is_empty() || allowed.contains(&terrain) {
            candidates.push(kind);
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }
    let chosen = hash
        .range(
            0,
            candidates.len() as i32,
            &[c.x, c.y, resources.len() as i32],
        )
        .map_err(|_| "resource hash range")?;
    g.tiles[i].improvement = None;
    g.tiles[i].resource = Some(candidates[chosen as usize]);
    Ok(())
}

/// Generates the shorelines: each water tile's edge toward land is drawn (with the swamp look
/// beside a swamp tile); every other tile draws none. Map generation and the ice rules call it.
pub(crate) fn generate_shorelines(g: &mut Game) {
    let width = g.width as usize;
    for i in 0..g.tiles.len() {
        let edge = |j: Option<usize>| {
            let n = j.map(|j| &g.tiles[j]);
            Shoreline {
                visible: n.is_some_and(|x| [FIELD, MOUNTAIN, FOREST, ICE].contains(&x.terrain)),
                sprite_extension: Some(
                    if n.is_some_and(|x| x.skin == SWAMP_SKIN || x.effects.contains(&SWAMPED)) {
                        "_swamp"
                    } else {
                        ""
                    }
                    .into(),
                ),
            }
        };
        let (north, south, east, west) = if g.tiles[i].terrain == WATER {
            (
                edge((i + width < g.tiles.len()).then_some(i + width)),
                edge(i.checked_sub(width).filter(|&j| j != 0)),
                edge((i % width + 1 < width).then_some(i + 1)),
                edge(i.checked_sub(1).filter(|_| i % width != 0)),
            )
        } else {
            (edge(None), edge(None), edge(None), edge(None))
        };
        Arc::make_mut(&mut g.side).shorelines[i] = Some(Shorelines {
            any: north.visible || south.visible || east.visible || west.visible,
            north,
            south,
            east,
            west,
        });
    }
}

/// The noise path: replaces the board, the continents, the
/// players' start data and the queued actions with candidate `seed`. `preferred` is the roster
/// index of the player asked to start in the west, if any.
pub fn generate(
    g: &mut Game,
    t: &Tables,
    seed: i32,
    settings: &MapGeneratorSettings,
    preferred: Option<usize>,
) -> Result<(), String> {
    draw(g, t, seed, settings, preferred)?.record(g)
}

/// `generate` without recording the continents, which nothing reads until the game is
/// exported: the candidate search draws many maps and records only the one it keeps.
pub(crate) fn draw(
    g: &mut Game,
    t: &Tables,
    seed: i32,
    settings: &MapGeneratorSettings,
    preferred: Option<usize>,
) -> Result<Drawn, String> {
    use super::preset;
    if g.players.last().map(|p| p.action.id) != Some(255) {
        return Err("map generation requires a trailing Nature player".into());
    }
    for p in &mut g.players {
        p.action.tasks.clear();
    }
    for tile in &mut g.tiles {
        *tile = cleared_tile(tile.coordinates);
    }
    Arc::make_mut(&mut g.side).names.fill(None);
    let mut me = Candidate {
        g,
        t,
        rng: MapGenerationContext::new(seed),
        continents: Vec::new(),
    };
    let mut actions = Vec::new();
    let islands = [preset::CONTINENTS, preset::PANGEA].contains(&settings.map_type);
    // islands first for these presets; when no capitals fit,
    // the board is cleared and the noise map drawn on the same stream
    if !(islands && me.island_map(settings, preferred, &mut actions)?) {
        if islands {
            for tile in &mut me.g.tiles {
                *tile = cleared_tile(tile.coordinates);
            }
        }
        me.noise_map(settings, preferred, &mut actions)?;
    }
    me.populate(settings)?;
    me.shorelines();
    let continents = me.continents;
    g.actions = actions;
    Ok(Drawn(continents))
}

impl Candidate<'_> {
    /// Generates the noise map: placement sites, terrain from noise with the capitals and
    /// villages, the post-terrain cities and the climates
    fn noise_map(
        &mut self,
        settings: &MapGeneratorSettings,
        preferred: Option<usize>,
        actions: &mut Vec<Action>,
    ) -> Result<(), String> {
        let (width, h) = (self.g.width as u16, height(self.g) as u16);
        let count = self.g.players.len() - 1;
        let sites = self
            .rng
            .noise_placement_sites(width, h, count as u8, settings, preferred)
            .map_err(|e| format!("noise site placement: {e:?}"))?;
        // the landform: noise seeded with the land list and the capitals' neighbours
        let mut seeds = sites.land_indices.clone();
        seeds.extend(capital_neighbor_indices(&sites.capitals, i32::from(width)));
        let mut noise = self
            .rng
            .generate_noise(width, h, &seeds, settings.wetness)
            .map_err(|e| format!("{e:?}"))?;
        super::smooth_noise(
            width,
            h,
            &mut noise,
            &sites.land_indices,
            settings.smooth_iterations,
            settings.surrounding_space_value,
        )
        .map_err(|e| format!("{e:?}"))?;
        let (waterline, ocean) =
            thresholds(&noise, settings.wetness, settings.shallow_percent_of_water)?;
        for (i, &sample) in noise.iter().enumerate() {
            if sites.cities.contains(&(i as i32)) {
                if let Some(seat) = sites.capitals.iter().position(|&x| x == i as i32) {
                    let player = self.g.players[seat].action.id;
                    let target = self.g.tiles[i].coordinates;
                    set_tile_as_capital(self.g, self.t, player, target, actions)?;
                } else {
                    set_tile_as_city(&mut self.g.tiles[i]);
                }
            } else {
                let tile = &mut self.g.tiles[i];
                if sample < ocean {
                    tile.terrain = OCEAN;
                    tile.altitude = -2;
                } else if waterline <= sample {
                    tile.terrain = FIELD;
                    tile.altitude = 1;
                } else {
                    tile.terrain = WATER;
                    tile.altitude = -1;
                }
            }
        }
        for &i in &sites.land_indices {
            self.g.tiles[i as usize].terrain = FIELD;
            self.g.tiles[i as usize].altitude = 1;
        }
        self.postterrain_cities(sites.postterrain_city_budget)?;
        self.climates(settings)
    }

    /// Populates the generated landform and finishes the map, up to the shorelines
    fn populate(&mut self, settings: &MapGeneratorSettings) -> Result<(), String> {
        let convert = settings.shallow_percent_of_water == 0.0;
        self.terrain()?;
        self.make_ocean(convert)?;
        self.lighthouses()?;
        self.resources(settings.richness)?;
        for pi in 0..self.g.players.len() {
            if self.g.players[pi].action.id == 255 {
                continue;
            }
            self.starting_resources(pi)?;
            let (climate, skin) = (player_climate(self.g, pi)?, self.g.players[pi].skin);
            for i in area(self.g, start_tile(self.g, pi)?, 1) {
                self.g.tiles[i].climate = climate;
                self.g.tiles[i].skin = skin;
            }
        }
        self.make_ocean(convert)?;
        // a flooding tribe floods every field and forest tile of its capital's climate and skin
        for pi in 0..self.g.players.len() {
            let (id, tribe, start) = {
                let p = &self.g.players[pi];
                (p.action.id, p.tribe, p.start_tile)
            };
            if id == 0 || !tribe_has(self.t, tribe, "flooded") {
                continue;
            }
            let s = self.g.tile(start).ok_or("missing flood start tile")?;
            let (climate, skin) = (s.climate, s.skin);
            for i in 0..self.g.tiles.len() {
                let x = &self.g.tiles[i];
                if x.climate == climate && x.skin == skin && [FIELD, FOREST].contains(&x.terrain) {
                    let c = x.coordinates;
                    crate::aquarion::flood_tile(self.g, id, c)?;
                }
            }
        }
        let total = self.g.tiles.len() as i32;
        self.ruins(total / 40)?;
        self.starfish(total as f32 * settings.wetness / 25.0)?;
        for pi in 0..self.g.players.len() {
            let tribe = self.g.players[pi].tribe;
            if self.t.tribes.name(tribe as usize).ok() == Some("polaris")
                && tribe_has(self.t, tribe, "alienclimate")
            {
                let center = start_tile(self.g, pi)?;
                for i in area(self.g, center, 1).chain(self.g.index(center)) {
                    if is_water(self.g.tiles[i].terrain) {
                        self.g.tiles[i].terrain = ICE;
                    }
                }
            }
        }
        for c in corners(self.g) {
            let i = self.g.index(c).ok_or("missing map corner")?;
            self.g.tiles[i].explorers.clear();
        }
        Ok(())
    }
}

/// The continents a candidate drew, which the game records once the candidate is kept.
#[must_use]
pub(crate) struct Drawn(Vec<Continent>);

impl Drawn {
    pub(crate) fn record(&self, g: &mut Game) -> Result<(), String> {
        g.set_continents(serde_json::to_value(&self.0).map_err(|e| e.to_string())?);
        Ok(())
    }
}
