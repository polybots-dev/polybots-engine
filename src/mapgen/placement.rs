//! Weighted map-site placement (pure
//! index and float arithmetic on the candidate's random stream).
use super::{MapGenerationContext, MapGenerationError};
use crate::state::Xy as Coordinates;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementError {
    InvalidDimensions,
    InvalidCount,
    DomainTooSmall,
    InvalidCapitalCount,
    ProbabilityIndexOutOfBounds,
    ProbabilityValueOutOfRange,
    ArrayLengthOverflow,
    AllocationFailed,
    Random(MapGenerationError),
}
impl From<MapGenerationError> for PlacementError {
    fn from(e: MapGenerationError) -> Self {
        Self::Random(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbabilityRange {
    pub start_x: i32,
    pub end_x: i32,
    pub start_y: i32,
    pub end_y: i32,
}
fn from_index(index: i32, width: i32) -> Coordinates {
    Coordinates {
        x: index % width,
        y: index / width,
    }
}
fn index(c: Coordinates, width: i32) -> i32 {
    c.y.wrapping_mul(width).wrapping_add(c.x)
}
fn chebyshev(a: Coordinates, b: Coordinates) -> i32 {
    a.x.wrapping_sub(b.x)
        .wrapping_abs()
        .max(a.y.wrapping_sub(b.y).wrapping_abs())
}

/// Adding a distance to the probability table merges cubic peaks with MAX, not addition.
pub fn add_distance_probability(
    table: &mut [i32],
    width: i32,
    center: Coordinates,
    domain_size: i32,
) -> Result<(), PlacementError> {
    if width <= 0 {
        return Err(PlacementError::InvalidDimensions);
    }
    for (i, p) in table.iter_mut().enumerate() {
        let peak = 1.max(domain_size / 2 - chebyshev(from_index(i as i32, width), center));
        let cube = peak.wrapping_mul(peak).wrapping_mul(peak);
        *p = if *p == 0 { cube } else { (*p).max(cube) };
    }
    Ok(())
}

/// The probability in a range: half-open bounds, Y outer/X inner,
/// Max(1,storedProbability), unchecked Int32 accumulation.
pub fn probability_in_range(
    table: &[i32],
    width: i32,
    range: ProbabilityRange,
) -> Result<i32, PlacementError> {
    if width <= 0 {
        return Err(PlacementError::InvalidDimensions);
    }
    let mut sum = 0_i32;
    for y in range.start_y..range.end_y {
        for x in range.start_x..range.end_x {
            let i = index(Coordinates { x, y }, width);
            let p = table
                .get(i as usize)
                .ok_or(PlacementError::ProbabilityIndexOutOfBounds)?;
            sum = sum.wrapping_add(1.max(*p));
        }
    }
    Ok(sum)
}

/// The index for a probability value selects the first cumulative sum > value.
pub fn index_for_probability(
    table: &[i32],
    width: i32,
    value: i32,
    range: ProbabilityRange,
) -> Result<i32, PlacementError> {
    if width <= 0 {
        return Err(PlacementError::InvalidDimensions);
    }
    let mut sum = 0_i32;
    for y in range.start_y..range.end_y {
        for x in range.start_x..range.end_x {
            let i = index(Coordinates { x, y }, width);
            let p = table
                .get(i as usize)
                .ok_or(PlacementError::ProbabilityIndexOutOfBounds)?;
            sum = sum.wrapping_add(1.max(*p));
            if value < sum {
                return Ok(i);
            }
        }
    }
    Err(PlacementError::ProbabilityValueOutOfRange)
}

impl MapGenerationContext {
    /// Capital positions. count may reach 7 for internal
    /// continent seed selection, independently of the2–4 actual player scope.
    /// No placement-setting reassignment is performed by this
    pub fn capital_positions(&mut self, width: u16, count: u8) -> Result<Vec<i32>, PlacementError> {
        if width == 0 {
            return Err(PlacementError::InvalidDimensions);
        }
        if !(1..=7).contains(&count) {
            return Err(PlacementError::InvalidCount);
        }
        let width = i32::from(width);
        let side = f64::from(count).sqrt().ceil() as i32;
        let domain = width / side;
        let remainder = width % side;
        if domain <= 2 {
            return Err(PlacementError::DomainTooSmall);
        }
        let length = width
            .checked_mul(width)
            .ok_or(PlacementError::ArrayLengthOverflow)?;
        let mut table = Vec::new();
        table
            .try_reserve_exact(length as usize)
            .map_err(|_| PlacementError::AllocationFailed)?;
        table.resize(length as usize, 0_i32);
        let offset = |v: i32| remainder.min(1.max(remainder.min(v) - 1));
        for y in 0..side {
            for x in 0..side {
                add_distance_probability(
                    &mut table,
                    width,
                    Coordinates {
                        x: domain * x + domain / 2 + offset(x),
                        y: domain * y + domain / 2 + offset(y),
                    },
                    domain,
                )?;
            }
        }
        let mut domains: Vec<i32> = (0..side * side).collect();
        let mut positions = Vec::with_capacity(count as usize);
        let inset = if domain < 5 { 1 } else { 2 };
        for _ in 0..count {
            let selected = self.next_range(0, domains.len() as i32)?;
            let d = domains.remove(selected as usize);
            let cell = from_index(d, side);
            let start_x = domain * cell.x + offset(cell.x);
            let start_y = domain * cell.y + offset(cell.y);
            let range = ProbabilityRange {
                start_x: 2.max(start_x + inset),
                end_x: (width - 2).min(start_x + domain - inset),
                start_y: 2.max(start_y + inset),
                end_y: (width - 2).min(start_y + domain - inset),
            };
            let total = probability_in_range(&table, width, range)?;
            let value = self.next_range(0, total)?;
            positions.push(index_for_probability(&table, width, value, range)?);
        }
        Ok(positions)
    }
}

/// Removing the indices near an index intentionally flattens out-of-bounds coordinates
/// without clamping. Removing by value preserves order and removes one copy.
pub fn remove_indices_near_index(
    indices: &mut Vec<i32>,
    site: i32,
    width: i32,
    range: i32,
) -> Result<(), PlacementError> {
    if width <= 0 {
        return Err(PlacementError::InvalidDimensions);
    }
    let center = from_index(site, width);
    for y in center.y.wrapping_sub(range)..=center.y.wrapping_add(range) {
        for x in center.x.wrapping_sub(range)..=center.x.wrapping_add(range) {
            let target = index(Coordinates { x, y }, width);
            if let Some(i) = indices.iter().position(|&v| v == target) {
                indices.remove(i);
            }
        }
    }
    Ok(())
}

impl MapGenerationContext {
    /// The cities placed before terrain are appended to the caller's existing city list
    /// and returns early when stable interior candidates have been exhausted.
    pub fn preterrain_cities(
        &mut self,
        width: u16,
        height: u16,
        cities: &mut Vec<i32>,
        count: i32,
    ) -> Result<(), PlacementError> {
        if width == 0 {
            return Err(PlacementError::InvalidDimensions);
        }
        let w = i32::from(width);
        let h = i32::from(height);
        let mut candidates = Vec::new();
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                candidates.push(index(Coordinates { x, y }, w));
            }
        }
        for &city in cities.iter() {
            remove_indices_near_index(&mut candidates, city, w, 2)?;
        }
        for _ in 0..count {
            if candidates.is_empty() {
                return Ok(());
            }
            let chosen = self.next_range(0, candidates.len() as i32)?;
            let site = candidates[chosen as usize];
            cities.push(site);
            remove_indices_near_index(&mut candidates, site, w, 2)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorridorResult {
    pub iterations: u32,
    pub reached: bool,
}
impl MapGenerationContext {
    /// Land between tiles: at most 1000 attempts, no final forced
    /// connection, and the target is not appended when the walk becomes adjacent.
    pub fn land_corridor(
        &mut self,
        from: Coordinates,
        to_index: i32,
        land: &mut Vec<i32>,
        width: u16,
    ) -> Result<CorridorResult, PlacementError> {
        if width == 0 {
            return Err(PlacementError::InvalidDimensions);
        }
        let width = i32::from(width);
        let target = from_index(to_index, width);
        let mut current = from;
        for iteration in 0..1000 {
            if current == target {
                return Ok(CorridorResult {
                    iterations: iteration,
                    reached: true,
                });
            }
            let i = index(current, width);
            if !land.contains(&i) {
                land.push(i);
            }
            let dx = target.x.wrapping_sub(current.x);
            let dy = target.y.wrapping_sub(current.y);
            if dx.wrapping_abs().wrapping_add(dy.wrapping_abs()) == 1 {
                return Ok(CorridorResult {
                    iterations: iteration + 1,
                    reached: true,
                });
            }
            let mask = self.next_range(1, 4)?;
            if mask & 1 != 0 {
                let mut sign = dx.signum();
                if sign == 0 {
                    sign = if self.next_float() > 0.5 { 1 } else { -1 };
                }
                current.x = (current.x + sign).max(0).min(width - 1);
            }
            if mask & 2 != 0 {
                let mut sign = dy.signum();
                if sign == 0 {
                    sign = if self.next_float() > 0.5 { 1 } else { -1 };
                }
                current.y = (current.y + sign).max(0).min(width - 1);
            }
        }
        Ok(CorridorResult {
            iterations: 1000,
            reached: false,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuburbPlacement {
    /// Newly selected sites only, in selection order. Borrowed repair sites are
    /// retained in per_player but are not appended again to this
    pub sites: Vec<i32>,
    pub per_player: Vec<Vec<i32>>,
}
fn within(site: Coordinates, indices: &[i32], width: i32, range: i32) -> bool {
    indices
        .iter()
        .any(|&v| chebyshev(site, from_index(v, width)) <= range)
}
impl MapGenerationContext {
    /// Suburbs. Source player count is the non-Nature roster
    /// count; the corresponding capital list must already have that ordering.
    pub fn suburbs(
        &mut self,
        width: u16,
        height: u16,
        capitals: &[i32],
        count: i32,
        land: &mut Vec<i32>,
    ) -> Result<SuburbPlacement, PlacementError> {
        if width == 0 || height == 0 {
            return Err(PlacementError::InvalidDimensions);
        }
        if !(2..=4).contains(&capitals.len()) {
            return Err(PlacementError::InvalidCapitalCount);
        }
        if count < 0 {
            return Err(PlacementError::InvalidCount);
        }
        let w = i32::from(width);
        let h = i32::from(height);
        let mut result = SuburbPlacement {
            sites: vec![],
            per_player: vec![vec![]; capitals.len()],
        };
        if count == 0 {
            return Ok(result);
        }
        for _ in 0..count {
            for (player, &capital) in capitals.iter().enumerate() {
                let center = from_index(capital, w);
                let mut radius = 3_i32;
                while (radius as f32) < (width as f32) * f32::from_bits(0x3f19999a) {
                    let mut candidates = Vec::new();
                    for y in center.y - radius..=center.y + radius {
                        if y >= h - 1 {
                            break;
                        }
                        if y <= 0 {
                            continue;
                        }
                        for x in center.x - radius..=center.x + radius {
                            if x >= w - 1 {
                                break;
                            }
                            if x <= 0 {
                                continue;
                            }
                            if x != center.x - radius
                                && x != center.x + radius
                                && y != center.y - radius
                                && y != center.y + radius
                            {
                                continue;
                            }
                            let c = Coordinates { x, y };
                            if !within(c, capitals, w, 2)
                                && !within(c, &result.sites, w, 2)
                                && !within(c, &result.per_player[player], w, 2)
                            {
                                candidates.push(index(c, w));
                            }
                        }
                    }
                    if !candidates.is_empty() {
                        let chosen = self.next_range(0, candidates.len() as i32)?;
                        let site = candidates[chosen as usize];
                        result.per_player[player].push(site);
                        result.sites.push(site);
                        // The candidate list changes after selection; it is
                        // rebuilt for the next player/radius before reuse.
                        remove_indices_near_index(&mut candidates, site, w, 2)?;
                        break;
                    }
                    radius += 1;
                }
            }
        }
        for (player, &capital) in capitals.iter().enumerate() {
            let center = from_index(capital, w);
            if result.per_player[player].len() != count as usize {
                // The game's repair runs requested-count iterations, not shortfall.
                for _ in 0..count {
                    let mut nearest = None;
                    let mut distance = w;
                    for &candidate in result.sites.iter().chain(capitals.iter()) {
                        if candidate == capital || result.per_player[player].contains(&candidate) {
                            continue;
                        }
                        let d = chebyshev(center, from_index(candidate, w));
                        if d < distance {
                            distance = d;
                            nearest = Some(candidate);
                        }
                    }
                    if let Some(site) = nearest {
                        result.per_player[player].push(site);
                    } else {
                        break;
                    }
                }
            }
            let mut centroid = center;
            for &site in &result.per_player[player] {
                let c = from_index(site, w);
                centroid.x = centroid.x.wrapping_add(c.x);
                centroid.y = centroid.y.wrapping_add(c.y);
            }
            let divisor = result.per_player[player].len() as i32 + 1;
            centroid.x /= divisor;
            centroid.y /= divisor;
            self.land_corridor(centroid, capital, land, width)?;
            for &site in &result.per_player[player] {
                self.land_corridor(centroid, site, land, width)?;
            }
        }
        Ok(result)
    }
}

/// The capitals' neighbours: exact insertion order, no bounds checks or
/// deduplication. Callers append this list to land indices after city selection.
pub fn capital_neighbor_indices(capitals: &[i32], width: i32) -> Vec<i32> {
    let mut out = Vec::with_capacity(capitals.len() * 4);
    for &i in capitals {
        out.extend([
            i.wrapping_add(1),
            i.wrapping_sub(1),
            i.wrapping_add(width),
            i.wrapping_sub(width),
        ]);
    }
    out
}

/// Capital reassignment based on placement settings, after the game's
/// nullable-account lookup has resolved the preferred player's roster index.
/// None means no requested/resolved player. Ties retain the first max(y-x).
pub fn reassign_noise_capitals(
    capitals: &mut [i32],
    width: u16,
    resolved_player: Option<usize>,
) -> Result<(), PlacementError> {
    let Some(player) = resolved_player else {
        return Ok(());
    };
    if width == 0 {
        return Err(PlacementError::InvalidDimensions);
    }
    if player >= capitals.len() {
        return Err(PlacementError::InvalidCapitalCount);
    }
    let mut best = None;
    let mut best_value = i32::MIN;
    for (i, &site) in capitals.iter().enumerate() {
        let c = from_index(site, i32::from(width));
        let value = c.y.wrapping_sub(c.x);
        if best_value < value {
            best_value = value;
            best = Some(i);
        }
    }
    let best = best.ok_or(PlacementError::InvalidCapitalCount)?;
    capitals.swap(player, best);
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoisePlacementSites {
    pub capitals: Vec<i32>,
    pub suburbs: SuburbPlacement,
    pub cities: Vec<i32>,
    /// Original list used for smoothing protection and final forced terrain.
    pub land_indices: Vec<i32>,
    /// Original list followed by capital neighbors, for the noise map only.
    pub noise_seed_indices: Vec<i32>,
    pub preterrain_city_budget: i32,
    pub postterrain_city_budget: i32,
}
impl MapGenerationContext {
    /// The placement prefix of map generation. Call on the same
    /// candidate RNG used by landforming; the game's fallback clears the land list
    /// before this entry but does not reset the already-advanced random stream.
    /// Preferred-player lookup is an explicit caller input, never guessed.
    pub fn noise_placement_sites(
        &mut self,
        width: u16,
        height: u16,
        players: u8,
        settings: &super::MapGeneratorSettings,
        resolved_preferred_player: Option<usize>,
    ) -> Result<NoisePlacementSites, PlacementError> {
        if !(2..=4).contains(&players) {
            return Err(PlacementError::InvalidCapitalCount);
        }
        let mut capitals = self.capital_positions(width, players)?;
        reassign_noise_capitals(&mut capitals, width, resolved_preferred_player)?;
        let axis = (f32::from(width) * f32::from_bits(0x3ea8f5c3)) as i32;
        let available = axis.wrapping_mul(axis).wrapping_sub(capitals.len() as i32);
        let max_cities = ((available as f32) * settings.post_terrain_city_density) as i32;
        let suburb_count = settings.min_suburb_count.max(
            settings
                .max_suburb_count
                .min(max_cities / i32::from(players)),
        );
        let mut land_indices = Vec::new();
        let suburbs = self.suburbs(width, height, &capitals, suburb_count, &mut land_indices)?;
        let mut cities = capitals.clone();
        cities.extend_from_slice(&suburbs.sites);
        let preterrain_city_budget = ((available.wrapping_sub(suburbs.sites.len() as i32) as f32)
            * settings.pre_terrain_city_density) as i32;
        self.preterrain_cities(width, height, &mut cities, preterrain_city_budget)?;
        land_indices.extend_from_slice(&cities);
        let mut noise_seed_indices = land_indices.clone();
        noise_seed_indices.extend(capital_neighbor_indices(&capitals, i32::from(width)));
        Ok(NoisePlacementSites {
            capitals,
            suburbs,
            cities,
            land_indices,
            noise_seed_indices,
            preterrain_city_budget,
            postterrain_city_budget: max_cities.wrapping_sub(preterrain_city_budget),
        })
    }
}
