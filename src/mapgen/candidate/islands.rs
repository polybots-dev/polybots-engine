//! The island path of a map candidate for Continents and Pangea.
//! Continents grow
//! from seeds over a smoothed noise map, villages and then capitals are placed on them; when
//! no capitals fit, `draw` falls back to the noise map on the same stream. Each step consumes
//! the candidate's random stream in the game's order, and its float expressions are the
//! game's.
use super::*;
use crate::mapgen::{min_capital_distance, preset, smooth_noise};

fn error(e: impl std::fmt::Debug) -> String {
    format!("island map generation: {e:?}")
}

/// The map area of radius 1 without the diagonals, x outer and y inner
fn orthogonal(g: &Game, c: Xy) -> Around {
    [(-1, 0), (0, -1), (0, 1), (1, 0)]
        .into_iter()
        .filter_map(|(dx, dy)| {
            g.index(Xy {
                x: c.x + dx,
                y: c.y + dy,
            })
        })
        .collect()
}

fn is_city(tile: &Tile) -> bool {
    tile.improvement.as_ref().is_some_and(|m| m.kind == CITY)
}

/// Populated continent count: lowered while players per continent is strictly between
/// 1 and 2
fn populated_continent_count(mut count: usize, players: usize) -> usize {
    while count > 1 {
        let ratio = ((players as f32) / (count as f32)).max(1.0_f32);
        if ratio >= 2.0_f32 || ratio == 1.0_f32 {
            break;
        }
        count -= 1;
    }
    count
}

/// Island capital eligibility: None excludes the tile from both passes, Some(true)
/// marks a strict site
fn capital_eligibility(g: &Game, i: usize, radius: i32) -> Option<bool> {
    let tile = &g.tiles[i];
    let c = tile.coordinates;
    let dx = c.x.min(g.width - 1 - c.x);
    let dy = c.y.min(height(g) - 1 - c.y);
    if dx.min(dy) < 2 || tile.improvement.is_some() {
        return None;
    }
    for j in area(g, c, radius) {
        let other = &g.tiles[j];
        let distance = (other.coordinates.x - c.x)
            .abs()
            .max((other.coordinates.y - c.y).abs());
        if other
            .improvement
            .as_ref()
            .is_some_and(|m| distance <= 2 || (other.owner != 0 && m.kind == CITY))
        {
            return None;
        }
    }
    Some(dx != 3 && dy != 3 && !is_water(tile.terrain))
}

impl Candidate<'_> {
    fn players(&self) -> Vec<i32> {
        self.g
            .players
            .iter()
            .map(|p| p.action.id)
            .filter(|&id| id != 255)
            .collect()
    }

    fn side(&self) -> u16 {
        self.g.width as u16
    }

    /// The island map: continents, villages, capitals (on the islands, else
    /// the tight placement and more villages), open-water villages and climates. Ok(false)
    /// when no capitals fit: the caller clears the board and draws the noise map instead.
    pub(super) fn island_map(
        &mut self,
        settings: &MapGeneratorSettings,
        preferred: Option<usize>,
        actions: &mut Vec<Action>,
    ) -> Result<bool, String> {
        let populated = self.island_villages(settings)?;
        if !self.island_capitals(populated, preferred, actions)? {
            if !self.tight_capitals(preferred, actions)? {
                return Ok(false);
            }
            self.fill_island_villages()?;
        }
        self.open_water_villages(settings.wetness)?;
        self.climates(settings)?;
        Ok(true)
    }

    /// Island villages: the smoothed noise, the continent seeds (the centre
    /// for Pangea; for Continents a drawn count of weighted capital positions), the
    /// continents grown from them and their villages. Returns how many continents hold
    /// capitals.
    fn island_villages(&mut self, settings: &MapGeneratorSettings) -> Result<usize, String> {
        let players = self.players().len();
        if !(2..=4).contains(&players) {
            return Err("island map generation supports 2 to 4 players".into());
        }
        let (width, h) = (self.side(), height(self.g) as u16);
        let mut noise = self
            .rng
            .generate_noise(width, h, &[], settings.wetness)
            .map_err(error)?;
        smooth_noise(
            width,
            h,
            &mut noise,
            &[],
            settings.smooth_iterations,
            settings.surrounding_space_value,
        )
        .map_err(error)?;
        let seeds = match settings.map_type {
            preset::PANGEA => vec![i32::from(h / 2) * i32::from(width) + i32::from(width / 2)],
            preset::CONTINENTS => {
                let count = self.continent_seed_count(settings.wetness)?;
                self.rng
                    .capital_positions(width, count as u8)
                    .map_err(error)?
            }
            _ => return Err("island map generation for a preset without islands".into()),
        };
        let populated = populated_continent_count(seeds.len(), players);
        self.add_continents(settings, &seeds, &noise, populated)?;
        self.fill_island_villages()?;
        Ok(populated)
    }

    /// Continent seed count: a count from 2 to 7 whose land per continent is
    /// 30 to 200 tiles (integer division before the float)
    fn continent_seed_count(&mut self, wetness: f32) -> Result<i32, String> {
        let tiles = self.g.tiles.len() as i32;
        let counts: Vec<i32> = (2..=7)
            .filter(|&count| {
                let size = (1.0_f32 - wetness) * ((tiles / count) as f32);
                (30.0_f32..=200.0_f32).contains(&size)
            })
            .collect();
        let choice = self.rng.next_range(0, counts.len() as i32).map_err(error)?;
        counts
            .get(choice as usize)
            .copied()
            .ok_or_else(|| "no continent seed count".into())
    }

    /// Add continents: one continent per seed, grown to its share of the land
    fn add_continents(
        &mut self,
        settings: &MapGeneratorSettings,
        seeds: &[i32],
        noise: &[f32],
        populated: usize,
    ) -> Result<(), String> {
        let tile_count = self.g.tiles.len();
        let players = self.g.players.len() - 1;
        if self.g.tiles.iter().any(|t| t.continent.is_some()) {
            return Err("island continents on an uncleared map".into());
        }
        if !seeds.is_empty() && (populated == 0 || populated > seeds.len()) {
            return Err("invalid populated continent count".into());
        }
        if seeds.iter().any(|&i| i < 0 || i as usize >= tile_count) {
            return Err("continent seed outside the map".into());
        }
        self.continents = seeds.iter().map(|_| fresh_continent()).collect();
        for (id, &i) in seeds.iter().enumerate() {
            let tile = self.g.tile_mut(i as usize);
            tile.continent = Some(id as i32);
            tile.altitude = 1;
            tile.terrain = FIELD;
            self.continents[id].tiles.push(tile.coordinates);
        }
        let count = seeds.len();
        let mut target_land = (tile_count as f32) * (1.0_f32 - settings.wetness);
        if count > 1 {
            target_land *= 0.5_f32 / (count as f32) + 1.0_f32;
        }
        let capital_share = (0.5_f32 * target_land) / (players as f32);
        let base_share = (target_land - 0.5_f32 * target_land) / (count as f32);
        for n in 0..players {
            if count > 0 {
                self.continents[n % populated].number_of_capitals += 1;
            }
        }
        let mut blocked = vec![false; self.g.tiles.len()];
        for id in 0..count {
            let mut attempts = 0;
            while (self.continents[id].number_of_capitals as f32) * capital_share + base_share
                > self.continents[id].tiles.len() as f32
                && attempts < 1000
            {
                attempts += 1;
                self.expand_continent(id, noise, settings.map_type, &mut blocked)?;
            }
        }
        Ok(())
    }

    /// Expand a continent: from a shuffled origin, the best-scoring free neighbour joins
    /// the continent; an origin with none is blocked for good
    fn expand_continent(
        &mut self,
        id: usize,
        noise: &[f32],
        map_type: i32,
        blocked: &mut [bool],
    ) -> Result<bool, String> {
        let (w, h) = (self.g.width, height(self.g));
        let mut origins = self.continents[id].tiles.clone();
        self.rng.shuffle(&mut origins).map_err(error)?;
        for origin in origins {
            let origin_index = self.g.index(origin).ok_or("continent tile off the map")?;
            if blocked[origin_index] {
                continue;
            }
            let mut candidates = orthogonal(self.g, origin);
            self.rng.shuffle(&mut candidates).map_err(error)?;
            let mut best_score = 0.0_f32;
            let mut best = None;
            for &candidate in &candidates {
                if self.g.tiles[candidate].continent.is_some() {
                    continue;
                }
                let c = self.g.tiles[candidate].coordinates;
                let mut score = noise[candidate] + 1.0_f32;
                let edge = c.x.min(w - 1 - c.x).min(c.y.min(h - 1 - c.y));
                match map_type {
                    preset::CONTINENTS => score += (edge as f32) / (-5.0_f32),
                    preset::PANGEA => score += (-1.0_f32) / (edge as f32),
                    _ => {}
                }
                for neighbour in area(self.g, c, 1) {
                    let Some(other) = self.g.tiles[neighbour].continent else {
                        continue;
                    };
                    if other as usize == id {
                        continue;
                    }
                    if self.g.tiles[neighbour].altitude >= 0 {
                        score -= self.rng.next_float_range(0.5, 3.0);
                        let n = self.g.tiles[neighbour].coordinates;
                        if n.x == c.x || n.y == c.y {
                            score += -10.0_f32;
                        }
                    } else {
                        score -= self.rng.next_float_range(0.0, 0.5);
                    }
                }
                if score > best_score {
                    best_score = score;
                    best = Some(candidate);
                }
            }
            if let Some(chosen) = best {
                let tile = self.g.tile_mut(chosen);
                tile.continent = Some(id as i32);
                tile.altitude = 1;
                tile.terrain = FIELD;
                tile.resource = None;
                self.continents[id].tiles.push(tile.coordinates);
                return Ok(true);
            }
            blocked[origin_index] = true;
        }
        Ok(false)
    }

    /// Fill island villages: continents in turn until each fails once in a row
    fn fill_island_villages(&mut self) -> Result<(), String> {
        let (mut misses, mut continent) = (0, 0);
        while misses < self.continents.len() {
            misses = if self.add_island_village(continent)? {
                0
            } else {
                misses + 1
            };
            continent = (continent + 1) % self.continents.len();
        }
        Ok(())
    }

    /// Add a village to a continent: a village on a shuffled continent tile; a forced
    /// second pass may place one but still reports false
    fn add_island_village(&mut self, continent: usize) -> Result<bool, String> {
        let mut sites = self.continents[continent].tiles.clone();
        if sites.len() < 2 {
            return Ok(false);
        }
        self.rng.shuffle(&mut sites).map_err(error)?;
        let (w, h) = (self.g.width, height(self.g));
        for forced in [false, true] {
            for &c in &sites {
                let i = self.g.index(c).ok_or("continent tile off the map")?;
                let tile = &self.g.tiles[i];
                let edge_x = c.x.min(w - 1 - c.x);
                let edge_y = c.y.min(h - 1 - c.y);
                if edge_x.min(edge_y) < 1
                    || tile.improvement.is_some()
                    || area(self.g, c, 2).any(|j| self.g.tiles[j].improvement.is_some())
                {
                    continue;
                }
                if forced {
                    let (mut same, mut foreign) = (false, false);
                    for j in area(self.g, c, 1) {
                        let neighbour = &self.g.tiles[j];
                        if neighbour.altitude >= 0 {
                            if neighbour.continent == tile.continent {
                                same = true;
                            } else {
                                foreign = true;
                            }
                        }
                    }
                    if !same || foreign {
                        continue;
                    }
                } else if is_water(tile.terrain)
                    || edge_x.min(edge_y) < 2
                    || edge_x == 3
                    || edge_y == 3
                {
                    continue;
                }
                set_tile_as_city(self.g.tile_mut(i));
                return Ok(!forced);
            }
        }
        Ok(false)
    }

    /// Emergency city position: a random strict site, else the first possible one in
    /// board order
    fn emergency_city_position(&mut self, players: u8) -> Result<Option<Xy>, String> {
        let radius = min_capital_distance(self.side(), players).map_err(error)? - 1;
        let mut strict = Vec::new();
        let mut first_possible = None;
        for i in 0..self.g.tiles.len() {
            if let Some(eligible) = capital_eligibility(self.g, i, radius) {
                first_possible.get_or_insert(self.g.tiles[i].coordinates);
                if eligible {
                    strict.push(self.g.tiles[i].coordinates);
                }
            }
        }
        if strict.is_empty() {
            return Ok(first_possible);
        }
        let k = self.rng.next_range(0, strict.len() as i32).map_err(error)?;
        Ok(Some(strict[k as usize]))
    }

    /// Reserve an island capital: the continent's
    /// unowned village farthest from the capitals so far (a little water nearby counts), else
    /// an emergency site; it becomes a city owned by `player` for now
    fn reserve_island_capital(
        &mut self,
        continent: usize,
        player: i32,
        players: u8,
        capitals: &mut Vec<Xy>,
    ) -> Result<bool, String> {
        let minimum = min_capital_distance(self.side(), players).map_err(error)?;
        let (w, h) = (self.g.width, height(self.g));
        let sites = &self
            .continents
            .get(continent)
            .ok_or("missing capital continent")?
            .tiles;
        let (mut best, mut best_score) = (None, 0);
        for &c in sites {
            let tile = self
                .g
                .tile(c)
                .ok_or("island capital candidate off the map")?;
            if tile.owner != 0 || !is_city(tile) {
                continue;
            }
            let distance = capitals.iter().fold(20, |nearest, other| {
                nearest.min((c.x - other.x).abs().max((c.y - other.y).abs()))
            });
            if distance < minimum {
                continue;
            }
            let edge = c.x.min(c.y).min(w - 1 - c.x).min(h - 1 - c.y);
            if edge <= 1 {
                continue;
            }
            let water = area(self.g, c, 1)
                .filter(|&i| is_water(self.g.tiles[i].terrain))
                .count();
            let score = distance + i32::from((2..=6).contains(&water));
            if score > best_score {
                best = Some(c);
                best_score = score;
            }
        }
        let site = match best {
            Some(c) => Some(c),
            None => self.emergency_city_position(players)?,
        };
        let Some(site) = site else {
            return Ok(false);
        };
        let i = self.g.index(site).ok_or("island capital off the map")?;
        set_tile_as_city(self.g.tile_mut(i));
        self.g.tile_mut(i).owner = player;
        capitals.push(site);
        Ok(true)
    }

    /// Assign island capitals: a provisional
    /// capital per player on its continent in turn, each then placed again on the continent
    /// its first site lies on. Ok(false) leaves the provisional cities for the tight pass.
    fn island_capitals(
        &mut self,
        populated: usize,
        preferred: Option<usize>,
        actions: &mut Vec<Action>,
    ) -> Result<bool, String> {
        if populated == 0 || populated > self.continents.len() {
            return Err("invalid populated continent count".into());
        }
        let ids = self.players();
        let players = ids.len() as u8;
        let mut sites = Vec::with_capacity(ids.len());
        for (seat, &id) in ids.iter().enumerate() {
            if !self.reserve_island_capital(seat % populated, id, players, &mut sites)? {
                return Ok(false);
            }
        }
        // a snapshot is visited while the live list loses and gains sites
        for original in sites.clone() {
            let i = self
                .g
                .index(original)
                .ok_or("provisional site off the map")?;
            if let Some(continent) = self.g.tiles[i].continent {
                let id = self.g.tiles[i].owner;
                self.g.tile_mut(i).owner = 0;
                let at = sites
                    .iter()
                    .position(|&c| c == original)
                    .ok_or("provisional site missing from the list")?;
                sites.remove(at);
                if !self.reserve_island_capital(continent as usize, id, players, &mut sites)? {
                    return Ok(false);
                }
            }
        }
        self.finalize_island_capitals(&ids, sites, preferred, actions)?;
        Ok(true)
    }

    /// Finalize island capitals: the preferred player moves to the site farthest
    /// south-west (largest y - x), then every site becomes its owner's capital
    fn finalize_island_capitals(
        &mut self,
        ids: &[i32],
        sites: Vec<Xy>,
        preferred: Option<usize>,
        actions: &mut Vec<Action>,
    ) -> Result<(), String> {
        if let Some(seat) = preferred {
            let id = *ids.get(seat).ok_or("missing preferred island player")?;
            let (mut best, mut best_delta, mut current) = (None, i32::MIN, None);
            for &c in &sites {
                let i = self.g.index(c).ok_or("preferred island site off the map")?;
                if c.y - c.x > best_delta {
                    best_delta = c.y - c.x;
                    best = Some(i);
                }
                if current.is_none() && self.g.tiles[i].owner == id {
                    current = Some(i);
                }
            }
            let from = current.ok_or("missing preferred island owner")?;
            let to = best.ok_or("empty island capital list")?;
            self.g.tile_mut(from).owner = self.g.tiles[to].owner;
            self.g.tile_mut(to).owner = id;
        }
        for site in sites {
            let i = self.g.index(site).ok_or("island capital off the map")?;
            let id = self.g.tiles[i].owner;
            self.g.tile_mut(i).owner = 0;
            set_tile_as_capital(self.g, self.t, id, site, actions)?;
        }
        Ok(())
    }

    /// Tight island capitals: with every
    /// improvement and owner cleared, each player takes the next strict site in board order.
    /// Ok(false) when the board runs out.
    fn tight_capitals(
        &mut self,
        preferred: Option<usize>,
        actions: &mut Vec<Action>,
    ) -> Result<bool, String> {
        let ids = self.players();
        let radius = min_capital_distance(self.side(), ids.len() as u8).map_err(error)? - 1;
        for tile in self.g.tiles_mut() {
            tile.improvement = None;
            tile.owner = 0;
        }
        let mut index = 0;
        let mut sites = Vec::with_capacity(ids.len());
        for &id in &ids {
            while index < self.g.tiles.len()
                && capital_eligibility(self.g, index, radius) != Some(true)
            {
                index += 1;
            }
            if index >= self.g.tiles.len() {
                return Ok(false);
            }
            let tile = self.g.tile_mut(index);
            set_tile_as_city(tile);
            tile.owner = id;
            sites.push(tile.coordinates);
        }
        self.finalize_island_capitals(&ids, sites, preferred, actions)?;
        Ok(true)
    }

    /// Place open-water villages: villages on open water away from land and cities,
    /// wetness x tiles / 50 of them (one at least: the count is checked after a placement)
    fn open_water_villages(&mut self, wetness: f32) -> Result<(), String> {
        let count = self.g.tiles.len() as i32;
        let target = (wetness * count as f32 / 50.0) as i32;
        let (w, h) = (self.g.width, height(self.g));
        let mut candidates: Vec<usize> = (0..self.g.tiles.len()).collect();
        self.rng.shuffle(&mut candidates).map_err(error)?;
        let mut placed = 0;
        for i in candidates {
            let tile = &self.g.tiles[i];
            let c = tile.coordinates;
            if tile.improvement.is_some()
                || !is_water(tile.terrain)
                || c.x <= 0
                || c.y <= 0
                || c.x >= w - 1
                || c.y >= h - 1
            {
                continue;
            }
            if area(self.g, c, 1).any(|j| !is_water(self.g.tiles[j].terrain)) {
                continue;
            }
            if area(self.g, c, 2).any(|j| is_city(&self.g.tiles[j])) {
                continue;
            }
            set_tile_as_city(self.g.tile_mut(i));
            placed += 1;
            if placed >= target {
                break;
            }
        }
        Ok(())
    }
}
