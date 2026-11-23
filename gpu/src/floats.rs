//! The rule path's floating-point expressions, evaluated by the kernel sources
//! (`kernel/floats.h`). A case is an expression (`Op`) and four raw 32-bit arguments, floats
//! passed as their bits; the result is the value's bits.

#[cfg(feature = "cuda")]
use std::os::raw::c_char;

/// The expressions, numbered as in `kernel/floats.h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    TechPrice,
    MovePriority,
    RoutePriority,
    UnderLimit,
    MeetTiers,
    HashValue,
    HashRangeF32,
    HashRangeI32,
    Sample,
    NextFloat,
    FloatRange,
    MaxOf,
    RangeOf,
    WideRange,
    NameLength,
    NameShort,
    Rebellion,
    DamageShown,
    TenthsUp,
}

pub const OPS: [Op; 19] = [
    Op::TechPrice,
    Op::MovePriority,
    Op::RoutePriority,
    Op::UnderLimit,
    Op::MeetTiers,
    Op::HashValue,
    Op::HashRangeF32,
    Op::HashRangeI32,
    Op::Sample,
    Op::NextFloat,
    Op::FloatRange,
    Op::MaxOf,
    Op::RangeOf,
    Op::WideRange,
    Op::NameLength,
    Op::NameShort,
    Op::Rebellion,
    Op::DamageShown,
    Op::TenthsUp,
];

extern "C" {
    fn ef_twin_float_cases(op: i32, count: i64, args: *const u32, out: *mut u64);
    fn ef_twin_hash_words(seed: u32, words: *const u32, count: i32) -> u32;
    fn ef_twin_random_stream(
        seed: i32,
        count: i32,
        kinds: *const u8,
        lows: *const i32,
        highs: *const i32,
        out: *mut i32,
    );
}

#[cfg(feature = "cuda")]
extern "C" {
    fn ef_cuda_float_cases(op: i32, count: i64, args: *const u32, out: *mut u64) -> i32;
    fn ef_cuda_device_name(name: *mut c_char, capacity: i32) -> i32;
}

/// Each case's result on the host twin.
pub fn twin(op: Op, cases: &[[u32; 4]]) -> Vec<u64> {
    let mut out = vec![0; cases.len()];
    // SAFETY: `cases` holds 4 words per case and `out` one word per case, as the C side reads
    // and writes.
    unsafe {
        ef_twin_float_cases(
            op as i32,
            cases.len() as i64,
            cases.as_ptr().cast(),
            out.as_mut_ptr(),
        )
    };
    out
}

/// Each case's result on the GPU, or the CUDA error.
#[cfg(feature = "cuda")]
pub fn device(op: Op, cases: &[[u32; 4]]) -> Result<Vec<u64>, String> {
    let mut out = vec![0; cases.len()];
    // SAFETY: as for `twin`.
    let code = unsafe {
        ef_cuda_float_cases(
            op as i32,
            cases.len() as i64,
            cases.as_ptr().cast(),
            out.as_mut_ptr(),
        )
    };
    if code == 0 {
        Ok(out)
    } else {
        Err(format!("CUDA error {code}"))
    }
}

/// The name of GPU 0, or the CUDA error (for example when there is no GPU).
#[cfg(feature = "cuda")]
pub fn device_name() -> Result<String, String> {
    let mut name = [0 as c_char; 256];
    // SAFETY: the C side writes at most `capacity` bytes, the last one zero.
    let code = unsafe { ef_cuda_device_name(name.as_mut_ptr(), name.len() as i32) };
    if code != 0 {
        return Err(format!("CUDA error {code}"));
    }
    // SAFETY: zero-terminated by the C side.
    Ok(unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}

/// The twin's `hash_words`.
pub fn twin_hash_words(seed: u32, words: &[u32]) -> u32 {
    // SAFETY: the C side reads `words.len()` words.
    unsafe { ef_twin_hash_words(seed, words.as_ptr(), words.len() as i32) }
}

/// Draws from the twin's seeded random stream seeded with `seed`: `None` is `next()`,
/// `Some((low, high))` is `next_range(low, high)` with `low <= high`.
pub fn twin_random_stream(seed: i32, draws: &[Option<(i32, i32)>]) -> Vec<i32> {
    let kinds: Vec<u8> = draws.iter().map(|d| u8::from(d.is_some())).collect();
    let lows: Vec<i32> = draws.iter().map(|d| d.map_or(0, |r| r.0)).collect();
    let highs: Vec<i32> = draws.iter().map(|d| d.map_or(0, |r| r.1)).collect();
    let mut out = vec![0; draws.len()];
    // SAFETY: the four arrays have `draws.len()` entries each.
    unsafe {
        ef_twin_random_stream(
            seed,
            draws.len() as i32,
            kinds.as_ptr(),
            lows.as_ptr(),
            highs.as_ptr(),
            out.as_mut_ptr(),
        )
    };
    out
}
