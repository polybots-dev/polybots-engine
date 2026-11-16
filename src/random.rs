//! The game's deterministic randomness: the seeded random stream (city names) and the seeded integer
//! hash the game state uses (`seeds.random_hash`, `seeds.village_name_hash`).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemRandom {
    values: [i32; 56],
    current: usize,
    lag: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RandomError {
    NegativeMaximum,
    ReversedRange,
}

impl SystemRandom {
    pub fn new(seed: i32) -> Self {
        let magnitude = if seed == i32::MIN {
            i32::MAX
        } else {
            seed.abs()
        };
        let mut previous = 161803398_i32.wrapping_sub(magnitude);
        let mut next = 1_i32;
        let mut values = [0_i32; 56];
        values[55] = previous;
        for i in 1..55 {
            let index = 21 * i % 55;
            values[index] = next;
            next = previous.wrapping_sub(next);
            if next < 0 {
                next = next.wrapping_add(i32::MAX);
            }
            previous = values[index];
        }
        for _ in 0..4 {
            for i in 1..56 {
                values[i] = values[i].wrapping_sub(values[1 + (i + 30) % 55]);
                if values[i] < 0 {
                    values[i] = values[i].wrapping_add(i32::MAX);
                }
            }
        }
        Self {
            values,
            current: 0,
            lag: 21,
        }
    }

    pub fn next(&mut self) -> i32 {
        self.current = if self.current == 55 {
            1
        } else {
            self.current + 1
        };
        self.lag = if self.lag == 55 { 1 } else { self.lag + 1 };
        let mut value = self.values[self.current].wrapping_sub(self.values[self.lag]);
        if value == i32::MAX {
            value -= 1;
        }
        if value < 0 {
            value = value.wrapping_add(i32::MAX);
        }
        self.values[self.current] = value;
        value
    }

    pub fn next_double(&mut self) -> f64 {
        f64::from(self.next()) * (1.0 / f64::from(i32::MAX))
    }

    /// The extension NextFloat casts the NextDouble result to float.
    pub fn next_float(&mut self) -> f32 {
        self.next_double() as f32
    }

    /// The float range extension: subtract bounds in single precision,
    /// multiply/add in double precision, then round once to single precision.
    /// As in the game, reversed/NaN float bounds are not rejected.
    pub fn next_float_range(&mut self, min: f32, max: f32) -> f32 {
        let sample = self.next_double();
        (f64::from(max - min) * sample + f64::from(min)) as f32
    }

    pub fn next_max(&mut self, max: i32) -> Result<i32, RandomError> {
        if max < 0 {
            return Err(RandomError::NegativeMaximum);
        }
        Ok((self.next_double() * f64::from(max)) as i32)
    }

    pub fn next_range(&mut self, min: i32, max: i32) -> Result<i32, RandomError> {
        if min > max {
            return Err(RandomError::ReversedRange);
        }
        let width = i64::from(max) - i64::from(min);
        let sample = if width <= i64::from(i32::MAX) {
            self.next_double()
        } else {
            let first = self.next();
            let signed = if self.next() % 2 == 0 { -first } else { first };
            (f64::from(signed) + 2147483646.0) / 4294967293.0
        };
        Ok(((sample * width as f64) as i64 + i64::from(min)) as i32)
    }
}

/// XXHash of an int. Signed input is hashed as raw 32 bits.
/// XXHash of a uint array: words are consumed in order,
/// and the byte length added to the accumulator is four times the word count.
pub fn hash_words(seed: u32, words: &[u32]) -> u32 {
    hash_raw(seed, words, |word| word)
}

/// `hash_words` over values read as 32 raw bits each by `raw`.
fn hash_raw<T: Copy>(seed: u32, words: &[T], raw: impl Fn(T) -> u32) -> u32 {
    let mut offset = 0;
    let mut hash = if words.len() >= 4 {
        let mut lanes = [
            seed.wrapping_add(0x24234428),
            seed.wrapping_add(0x85ebca77),
            seed,
            seed.wrapping_sub(0x9e3779b1),
        ];
        while offset + 4 <= words.len() {
            for lane in 0..4 {
                lanes[lane] = lanes[lane]
                    .wrapping_add(raw(words[offset + lane]).wrapping_mul(0x85ebca77))
                    .rotate_left(13)
                    .wrapping_mul(0x9e3779b1);
            }
            offset += 4;
        }
        lanes[0]
            .rotate_left(1)
            .wrapping_add(lanes[1].rotate_left(7))
            .wrapping_add(lanes[2].rotate_left(12))
            .wrapping_add(lanes[3].rotate_left(18))
    } else {
        seed.wrapping_add(0x165667b1)
    };
    hash = hash.wrapping_add((words.len() as u32).wrapping_mul(4));
    for &word in &words[offset..] {
        hash = hash
            .wrapping_add(raw(word).wrapping_mul(0xc2b2ae3d))
            .rotate_left(17)
            .wrapping_mul(0x27d4eb2f);
    }
    avalanche(hash)
}

/// The signed-array form uses each integer's raw bits.
pub fn hash_ints(seed: u32, words: &[i32]) -> u32 {
    // Reinterpretation is explicit, independent of host endianness.
    hash_raw(seed, words, |word| word as u32)
}

fn avalanche(hash: u32) -> u32 {
    let hash = (hash ^ (hash >> 15)).wrapping_mul(0x85ebca77);
    let hash = (hash ^ (hash >> 13)).wrapping_mul(0xc2b2ae3d);
    hash ^ (hash >> 16)
}

/// The byte-array form reads
/// little-endian words; tail bytes use a separate rotate-11 round.
/// HashFunction integer ranges trap on a zero wrapped width.
/// An explicit error replaces the process-level divide exception.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashRangeError {
    ZeroWidth,
}

/// HashFunction.Value: round unsigned hash to f32 before multiplying
/// by the exactly representable 2^-32 constant.
pub fn hash_value(hash: u32) -> f32 {
    (hash as f32) * f32::from_bits(0x2f800000)
}

/// HashFunction.Range integer arithmetic.
/// max-min wraps as Int32. Negative widths are legal: the
/// dividend is the nonnegative, zero-extended UInt32 hash. The final add wraps.
pub fn hash_range_i32(hash: u32, min: i32, max: i32) -> Result<i32, HashRangeError> {
    let width = max.wrapping_sub(min);
    if width == 0 {
        return Err(HashRangeError::ZeroWidth);
    }
    let remainder = i64::from(hash) % i64::from(width);
    Ok(min.wrapping_add(remainder as i32))
}

/// The float Range computes width * roundedHash * 2^-32 + min, all in
/// f32. Normalizing the hash first changes overflow and
/// rounding, so this deliberately does not call hash_value.
pub fn hash_range_f32(hash: u32, min: f32, max: f32) -> f32 {
    let width = max - min;
    let scaled = width * (hash as f32);
    let normalized = scaled * f32::from_bits(0x2f800000);
    normalized + min
}

/// Stateless XXHash-backed HashFunction. The signed seed is retained;
/// hashing consumes its raw bits and never advances a mutable RNG stream.
/// The HashFunction: a stateless hash seeded by one integer.
#[derive(Clone, Copy, Debug)]
pub struct HashFunction(pub i32);

impl HashFunction {
    pub fn hash(self, data: &[i32]) -> u32 {
        hash_ints(self.0 as u32, data)
    }
    pub fn value(self, data: &[i32]) -> f32 {
        hash_value(self.hash(data))
    }
    pub fn range(self, min: i32, max: i32, data: &[i32]) -> Result<i32, HashRangeError> {
        hash_range_i32(self.hash(data), min, max)
    }
    pub fn range_float(self, min: f32, max: f32, data: &[i32]) -> f32 {
        hash_range_f32(self.hash(data), min, max)
    }
}
