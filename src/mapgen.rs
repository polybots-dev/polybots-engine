//! Map generation for a new game, so that reset needs nothing else. Each stage is checked
//! against the game per seed. Every preset: the noise path, and for Continents and Pangea
//! the island path first (candidate/islands.rs).
use crate::random::{RandomError, SystemRandom};

pub mod candidate;
pub mod placement;
pub mod select;

pub use placement::capital_neighbor_indices;

/// map type ids
pub mod preset {
    pub const NONE: i32 = 0;
    pub const DRYLAND: i32 = 1;
    pub const LAKES: i32 = 2;
    pub const CONTINENTS: i32 = 3;
    pub const ARCHIPELAGO: i32 = 4;
    pub const WATER_WORLD: i32 = 5;
    pub const PANGEA: i32 = 6;
}

/// The map generator's settings for a preset. Every constant
/// uses original f32 bits; the shallow-water share stays zero.
#[derive(Clone, Debug, PartialEq)]
pub struct MapGeneratorSettings {
    pub wetness: f32,
    pub richness: f32,
    pub smooth_iterations: i32,
    pub pre_terrain_city_density: f32,
    pub post_terrain_city_density: f32,
    pub min_suburb_count: i32,
    pub max_suburb_count: i32,
    pub surrounding_space_value: f32,
    pub shallow_percent_of_water: f32,
    pub equality_iterations: i32,
    pub equality_limit: f32,
    pub map_type: i32,
}
impl Default for MapGeneratorSettings {
    fn default() -> Self {
        Self {
            wetness: f32::from_bits(0x3ee66666),
            richness: 1.0,
            smooth_iterations: 2,
            pre_terrain_city_density: f32::from_bits(0x3e99999a),
            post_terrain_city_density: 1.0,
            min_suburb_count: 1,
            max_suburb_count: 2,
            surrounding_space_value: 0.5,
            shallow_percent_of_water: 0.0,
            equality_iterations: 50,
            equality_limit: f32::from_bits(0x3f8147ae),
            map_type: preset::NONE,
        }
    }
}
impl MapGeneratorSettings {
    pub fn from_preset(map_type: i32) -> Self {
        let mut s = Self {
            map_type,
            ..Self::default()
        };
        match map_type {
            preset::NONE | preset::CONTINENTS => {
                s.wetness = 0.5;
                s.surrounding_space_value = 1.0;
                s.smooth_iterations = 1;
            }
            preset::DRYLAND => {
                s.wetness = 0.0;
                s.smooth_iterations = 0;
                s.pre_terrain_city_density = 0.0;
                s.min_suburb_count = 0;
                s.max_suburb_count = 0;
            }
            preset::LAKES => {
                s.wetness = f32::from_bits(0x3e99999a);
                s.surrounding_space_value = 1.0;
            }
            preset::ARCHIPELAGO => {
                s.wetness = f32::from_bits(0x3f266666);
                s.smooth_iterations = 0;
            }
            preset::WATER_WORLD => {
                s.wetness = f32::from_bits(0x3f59999a);
                s.pre_terrain_city_density = f32::from_bits(0x3dcccccd);
                s.min_suburb_count = 0;
                s.max_suburb_count = 0;
            }
            preset::PANGEA => {
                s.wetness = 0.5;
                s.surrounding_space_value = 0.0;
                s.smooth_iterations = 1;
            }
            _ => {}
        }
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapGenerationError {
    UnsupportedPlayerCount,
    InvalidNoiseShape,
    InvalidLandIndex,
    NoContinentSeedCount,
    NonFiniteNoise,
    InvalidNoiseQuantile,
    CapitalPlacementUnsupported,
    MapNotCleared,
    InvalidRoster,
    InvalidPopulatedContinentCount,
    WrongMapPreset,
    TileArrayTooShort,
    ListTooLong,
    ArrayLengthOverflow,
    AllocationFailed,
    Random(RandomError),
}

/// Candidate-map stream. Its seed is independent of both of the game's seed fields.
/// Construction draws no samples; continuation retains this stream even when
/// the islands stage fails and the generator falls back to noise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapGenerationContext {
    seed: i32,
    random: SystemRandom,
    draws: u64,
}
impl MapGenerationContext {
    pub fn new(seed: i32) -> Self {
        Self {
            seed,
            random: SystemRandom::new(seed),
            draws: 0,
        }
    }
    pub const fn seed(&self) -> i32 {
        self.seed
    }
    pub const fn draws(&self) -> u64 {
        self.draws
    }
    pub fn next_raw(&mut self) -> i32 {
        self.draws += 1;
        self.random.next()
    }
    pub fn next_float(&mut self) -> f32 {
        self.draws += 1;
        self.random.next_float()
    }
    /// A float range in [min, max), with the f32 width
    pub fn next_float_range(&mut self, min: f32, max: f32) -> f32 {
        self.draws += 1;
        self.random.next_float_range(min, max)
    }
    pub fn next_range(&mut self, min: i32, max: i32) -> Result<i32, MapGenerationError> {
        let value = self
            .random
            .next_range(min, max)
            .map_err(MapGenerationError::Random)?;
        self.draws += if i64::from(max) - i64::from(min) > i64::from(i32::MAX) {
            2
        } else {
            1
        };
        Ok(value)
    }
    /// Both shuffles swap forward and consume one
    /// draw even for a self-swap. Lists shorter than two consume no draws.
    pub fn shuffle<T>(&mut self, items: &mut [T]) -> Result<(), MapGenerationError> {
        let count = i32::try_from(items.len()).map_err(|_| MapGenerationError::ListTooLong)?;
        for i in 0..count - 1 {
            let j = self.next_range(i, count)?;
            items.swap(i as usize, j as usize);
        }
        Ok(())
    }
    /// The noise map: one f32 draw per row-major tile, followed by the
    /// land-only transform sample*(1-wetness)+wetness. Duplicates in the land
    /// list do not multiply draws; indices outside the array never match.
    pub fn generate_noise(
        &mut self,
        width: u16,
        height: u16,
        land_indices: &[i32],
        wetness: f32,
    ) -> Result<Vec<f32>, MapGenerationError> {
        let count = usize::from(width) * usize::from(height);
        if count > i32::MAX as usize {
            return Err(MapGenerationError::ArrayLengthOverflow);
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(count)
            .map_err(|_| MapGenerationError::AllocationFailed)?;
        let scale = 1.0_f32 - wetness;
        for i in 0..count {
            let sample = self.next_float();
            output.push(if land_indices.contains(&(i as i32)) {
                sample * scale + wetness
            } else {
                sample
            });
        }
        Ok(output)
    }
}

/// The minimum capital distance and the domain size divide by ceil(sqrt(players)).
/// In the supported2–4-player scope this divisor is exactly2.
pub fn domain_size(width: u16, players: u8) -> Result<i32, MapGenerationError> {
    if !(2..=4).contains(&players) {
        return Err(MapGenerationError::UnsupportedPlayerCount);
    }
    Ok(i32::from(width) / 2)
}
pub fn min_capital_distance(width: u16, players: u8) -> Result<i32, MapGenerationError> {
    domain_size(width, players)?;
    Ok(3 + i32::from((i32::from(width) - 1) / 2 > 4))
}

/// The noise smoothing: each iteration reads the
/// old array, sums the3x3 neighborhood in y-then-x order (out-of-bounds uses
/// surrounding), divides by9, restores all protected land values, then copies
/// the scratch array back. Zero/negative iterations leave the array untouched.
pub fn smooth_noise(
    width: u16,
    height: u16,
    noise: &mut [f32],
    land_indices: &[i32],
    iterations: i32,
    surrounding: f32,
) -> Result<(), MapGenerationError> {
    if noise.len() != usize::from(width) * usize::from(height) {
        return Err(MapGenerationError::InvalidNoiseShape);
    }
    if iterations <= 0 {
        return Ok(());
    }
    if land_indices
        .iter()
        .any(|&i| i < 0 || i as usize >= noise.len())
    {
        return Err(MapGenerationError::InvalidLandIndex);
    }
    let mut scratch = vec![0.0_f32; noise.len()];
    for _ in 0..iterations {
        for y in 0..i32::from(height) {
            for x in 0..i32::from(width) {
                let mut sum = 0.0_f32;
                for ny in y - 1..=y + 1 {
                    for nx in x - 1..=x + 1 {
                        let value = if nx >= 0
                            && ny >= 0
                            && nx < i32::from(width)
                            && ny < i32::from(height)
                        {
                            noise[(ny as usize) * usize::from(width) + nx as usize]
                        } else {
                            surrounding
                        };
                        sum += value;
                    }
                }
                // The sum adds (9-area.Count)*surrounding before dividing; the
                // radius1/diagonal/include-center area always contains9 entries.
                scratch[(y as usize) * usize::from(width) + x as usize] =
                    (sum + 0.0_f32 * surrounding) / 9.0_f32;
            }
        }
        for &i in land_indices {
            scratch[i as usize] = noise[i as usize];
        }
        noise.copy_from_slice(&scratch);
    }
    Ok(())
}
