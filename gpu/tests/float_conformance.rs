//! Every floating-point expression of the rule path gives the
//! same bits in Rust, in the host build of the kernel sources and, with the `cuda` feature,
//! on the GPU. Any difference fails; none is tolerated or rounded away.
//!
//! The Rust side calls `engine_fast::random` where the expression is a public function there
//! and otherwise repeats the expression as it stands in the engine's source (named at each
//! arm), so a wrong transcription here would show up as a difference, not hide one. What
//! this cannot show: that the engine's rule code uses these expressions where the port will;
//! the lockstep of the later stages checks that.
//!
//! Inputs: every combination of a set of edge values, plus random cases. Float arguments are
//! finite and below 1e30 in size and the route's offsets stay on a board, so no case
//! overflows to infinity or makes a NaN, whose bits are not the same on every machine.

use engine_fast::random::{self, SystemRandom};
use engine_fast_gpu::floats::{self, Op, OPS};

const EDGES: [u32; 16] = [
    0,
    1,
    2,
    3,
    9,
    10,
    11,
    999,
    1000,
    1001,
    0x00ff_ffff,
    0x0100_0001,
    0x7fff_fffe,
    0x7fff_ffff,
    0x8000_0000,
    0xffff_ffff,
];

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }
    /// a finite float below 1e30 in size, as bits
    fn float(&mut self) -> u32 {
        loop {
            let bits = self.next();
            let x = f32::from_bits(bits);
            if x.is_finite() && x.abs() < 1e30 {
                return bits;
            }
        }
    }
}

/// Bring a raw case into the range the expression is defined on.
fn shape(op: Op, mut c: [u32; 4], rng: &mut Lcg) -> [u32; 4] {
    let ordered = |c: &mut [u32; 4]| {
        if (c[1] as i32) > (c[2] as i32) {
            c.swap(1, 2);
        }
    };
    match op {
        Op::RoutePriority | Op::MovePriority => {
            c[0] = ((c[0] % 129) as i32 - 64) as u32;
            c[1] = ((c[1] % 129) as i32 - 64) as u32;
        }
        Op::HashRangeF32 | Op::FloatRange => {
            c[1] = rng.float();
            c[2] = rng.float();
        }
        Op::MaxOf => c[1] &= 0x7fff_ffff,
        Op::RangeOf | Op::WideRange => ordered(&mut c),
        _ => {}
    }
    c
}

fn cases(op: Op, random: usize) -> Vec<[u32; 4]> {
    let mut rng = Lcg(0x5eed_0000 + op as u64);
    let mut out = Vec::new();
    for a in EDGES {
        for b in EDGES {
            for c in EDGES {
                out.push(shape(op, [a, b, c, a ^ c], &mut rng));
            }
        }
    }
    for _ in 0..random {
        let raw = [rng.next(), rng.next(), rng.next(), rng.next()];
        // half of the random cases use small numbers, where the game's values are
        let raw = if rng.next() & 1 == 0 {
            raw.map(|x| x % 4096)
        } else {
            raw
        };
        out.push(shape(op, raw, &mut rng));
    }
    out
}

fn sample(next: i32) -> f64 {
    // random.rs next_double
    f64::from(next) * (1.0 / f64::from(i32::MAX))
}

/// The expression as the Rust engine computes it.
fn rust(op: Op, [a, b, c, d]: [u32; 4]) -> u64 {
    let (ia, ib, ic, id) = (a as i32, b as i32, c as i32, d as i32);
    match op {
        // menu.rs tech_price
        Op::TechPrice => {
            let mut x = (ia.wrapping_sub(1).wrapping_mul(ib)) as f32 + ib as f32 + 4.0;
            if ic & 1 == 1 {
                x *= f32::from_bits(0x3f2aaa3b);
            }
            (x.ceil() as i32) as u32 as u64
        }
        // movement.rs Profile::path
        Op::MovePriority => {
            let (dx, dy) = (ia as f64, ib as f64);
            ((dx * dx + dy * dy).sqrt() as f32 + ic as f32).to_bits() as u64
        }
        // routes.rs router_path
        Op::RoutePriority => {
            ((f64::from(ia * ia + ib * ib).sqrt()) as f32 + ic as f32).to_bits() as u64
        }
        // routes.rs router_path, movement.rs Profile::path
        Op::UnderLimit => u64::from((ia as f32) < ib.wrapping_mul(10) as f32),
        // explore.rs meet_action
        Op::MeetTiers => (((a as f32) / 1000.0_f32).ceil() as i32) as u32 as u64,
        Op::HashValue => random::hash_value(a).to_bits() as u64,
        Op::HashRangeF32 => {
            random::hash_range_f32(a, f32::from_bits(b), f32::from_bits(c)).to_bits() as u64
        }
        Op::HashRangeI32 => random::hash_range_i32(a, ib, ic).map_or(u64::MAX, |x| x as u32 as u64),
        Op::Sample => sample(ia).to_bits(),
        // random.rs next_float
        Op::NextFloat => (sample(ia) as f32).to_bits() as u64,
        // random.rs next_float_range
        Op::FloatRange => {
            let (min, max) = (f32::from_bits(b), f32::from_bits(c));
            ((f64::from(max - min) * sample(ia) + f64::from(min)) as f32).to_bits() as u64
        }
        // random.rs next_max
        Op::MaxOf => ((sample(ia) * f64::from(ib)) as i32) as u32 as u64,
        // random.rs next_range, the usual sample and the wide-range one
        Op::RangeOf | Op::WideRange => {
            let width = i64::from(ic) - i64::from(ib);
            let s = if op == Op::RangeOf {
                sample(ia)
            } else {
                let signed = if id % 2 == 0 { ia.wrapping_neg() } else { ia };
                (f64::from(signed) + 2147483646.0) / 4294967293.0
            };
            (((s * width as f64) as i64).wrapping_add(i64::from(ib)) as i32) as u32 as u64
        }
        // capture.rs name_city
        Op::NameLength => ((sample(ia) as f32) * 3.0_f32 + 3.0_f32).to_bits() as u64,
        Op::NameShort => u64::from((ib as f32) < (sample(ia) as f32) * 3.0_f32 + 3.0_f32),
        // infiltrate.rs rebellion_weight and the negated sort key
        // combat.rs shown
        Op::DamageShown => ((ia as f32 * 0.1_f32).round_ties_even() as i32) as u32 as u64,
        Op::TenthsUp => ((ia as f32 * 0.1_f32).ceil() as i32) as u32 as u64,
        Op::Rebellion => {
            let mut weight = random::hash_range_f32(a, 0.0, f32::from_bits(0x3dcccccd));
            if ib & 1 != 0 {
                weight += 2.0;
            }
            if ib & 2 != 0 {
                weight += -0.1;
            }
            if ib & 4 != 0 {
                weight += 0.5;
            }
            if ib & 8 != 0 {
                weight += 1.0;
            }
            (-weight).to_bits() as u64
        }
    }
}

fn compare(what: &str, op: Op, cases: &[[u32; 4]], got: &[u64]) {
    for (case, &got) in cases.iter().zip(got) {
        let want = rust(op, *case);
        assert_eq!(
            got, want,
            "{what} differs from Rust on {op:?} {case:x?}: {got:#x} against {want:#x}"
        );
    }
}

#[test]
fn the_host_build_computes_every_float_expression_as_rust_does() {
    for op in OPS {
        let cases = cases(op, 200_000);
        compare("the host build", op, &cases, &floats::twin(op, &cases));
    }
}

/// The attack preview's rounding over every
/// damage a game can show, ties included: 5 shows 0 and 15 shows 2, as `round_ties_even` (a
/// `roundf` port would show 1 and 2).
#[test]
fn the_preview_rounding_is_half_to_even() {
    let cases: Vec<[u32; 4]> = (-20_000i32..=20_000).map(|r| [r as u32, 0, 0, 0]).collect();
    compare(
        "the host build",
        Op::DamageShown,
        &cases,
        &floats::twin(Op::DamageShown, &cases),
    );
    let shown = floats::twin(
        Op::DamageShown,
        &[[5, 0, 0, 0], [15, 0, 0, 0], [25, 0, 0, 0]],
    );
    assert_eq!(shown, [0, 2, 2]);
}

/// The comparison itself must be able to fail: a result changed by one bit is reported.
#[test]
#[should_panic(expected = "differs from Rust")]
fn a_one_bit_difference_is_reported() {
    let cases = cases(Op::NameLength, 10);
    let mut got = floats::twin(Op::NameLength, &cases);
    got[3] ^= 1;
    compare("the host build", Op::NameLength, &cases, &got);
}

/// A fused multiply-add would change these expressions; the check would see it. (This guards
/// the test's reach, since the build flag that forbids fusing is easy to lose.)
#[test]
fn the_cases_include_ones_a_fused_multiply_add_would_change() {
    let cases = cases(Op::NameLength, 200_000);
    let fused = cases.iter().filter(|c| {
        let x = sample(c[0] as i32) as f32;
        x.mul_add(3.0, 3.0).to_bits() != (x * 3.0 + 3.0).to_bits()
    });
    assert!(fused.count() > 100);
}

#[test]
fn the_host_build_hashes_as_rust_does() {
    let mut rng = Lcg(7);
    for len in 0..=13 {
        for _ in 0..2_000 {
            let words: Vec<u32> = (0..len).map(|_| rng.next()).collect();
            let seed = rng.next();
            assert_eq!(
                floats::twin_hash_words(seed, &words),
                random::hash_words(seed, &words),
                "{seed} {words:?}"
            );
        }
    }
}

#[test]
fn the_host_build_draws_the_same_random_stream_as_rust() {
    let mut rng = Lcg(11);
    let mut seeds = vec![0, 1, -1, i32::MAX, i32::MIN, i32::MIN + 1, 161803398];
    seeds.extend((0..300).map(|_| rng.next() as i32));
    for seed in seeds {
        let draws: Vec<Option<(i32, i32)>> = (0..400)
            .map(|_| match rng.next() % 4 {
                0 => None,
                1 => Some((0, (rng.next() % 40) as i32)),
                2 => {
                    let (a, b) = (rng.next() as i32, rng.next() as i32);
                    Some((a.min(b), a.max(b)))
                }
                _ => Some((i32::MIN, i32::MAX)),
            })
            .collect();
        let mut reference = SystemRandom::new(seed);
        let want: Vec<i32> = draws
            .iter()
            .map(|d| match d {
                None => reference.next(),
                Some((low, high)) => reference.next_range(*low, *high).unwrap(),
            })
            .collect();
        assert_eq!(
            floats::twin_random_stream(seed, &draws),
            want,
            "seed {seed}"
        );
    }
}

#[cfg(feature = "cuda")]
#[test]
fn the_gpu_computes_every_float_expression_as_rust_does() {
    let name = floats::device_name().expect("the cuda feature needs a GPU");
    for op in OPS {
        let cases = cases(op, 2_000_000);
        let got = floats::device(op, &cases).unwrap();
        compare("the GPU", op, &cases, &got);
    }
    println!(
        "float conformance on {name}: {} expressions equal",
        OPS.len()
    );
}
