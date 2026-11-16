//! The step of the kernel sources (`kernel/step.h`): one decision applied to a device block.

use crate::state::{EfGame, EfXy};
use crate::tables::DeviceTables;
use engine_fast_frame::Action;

/// The chosen move as `kernel/action.h` EfMove holds it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EfMove {
    pub kind: i32,
    pub has_source: i32,
    pub has_target: i32,
    pub has_option: i32,
    pub has_opponent: i32,
    pub accept: i32,
    pub source: EfXy,
    pub target: EfXy,
    pub option: i32,
    pub opponent: i32,
}

impl EfMove {
    pub fn of(a: &Action) -> EfMove {
        let xy = |c: Option<engine_fast_frame::Xy>| {
            c.map_or(EfXy::default(), |c| EfXy { x: c.x, y: c.y })
        };
        EfMove {
            kind: a.kind,
            has_source: i32::from(a.source.is_some()),
            has_target: i32::from(a.target.is_some()),
            has_option: i32::from(a.option.is_some()),
            has_opponent: i32::from(a.opponent.is_some()),
            accept: a.accept.map_or(-1, i32::from),
            source: xy(a.source),
            target: xy(a.target),
            option: a.option.unwrap_or(0),
            opponent: a.opponent.unwrap_or(0),
        }
    }
}

/// Why the kernel's step stopped: its code, the engine's text, and whether the engine panics
/// there rather than returning an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    pub code: i32,
    pub arg: i64,
    pub text: String,
    pub failed: bool,
}

impl Stop {
    /// The code the kernel gives a rule not ported yet (`EF_ERR_NOT_PORTED`), whose argument
    /// says which: 1000 + move kind, 2000 + command, 3000 + action kind.
    pub fn not_ported(&self) -> Option<i64> {
        (self.text.starts_with("not ported to the device")).then_some(self.arg)
    }
}

extern "C" {
    fn ef_twin_advance(
        g: *mut EfGame,
        t: *const crate::tables::EfTables,
        m: *const EfMove,
        elapsed: *mut i32,
        arg: *mut i64,
    ) -> i32;
    fn ef_twin_error_failed(code: i32) -> i32;
    fn ef_twin_error_capacity(code: i32) -> i32;
    fn ef_twin_command(
        g: *mut EfGame,
        t: *const crate::tables::EfTables,
        m: *const EfMove,
        arg: *mut i64,
    ) -> i32;
    fn ef_twin_city_name(
        g: *mut EfGame,
        t: *const crate::tables::EfTables,
        tribe: i32,
        skin: i32,
        x: i32,
        y: i32,
        word: *mut i32,
        len: *mut i32,
        arg: *mut i64,
    ) -> i32;
    fn ef_twin_move_size() -> i32;
}

/// Whether the Rust engine panics at the kernel's error `code` (the batch fails the game)
/// rather than returning an error (it refuses the game).
pub fn is_failure(code: i32) -> bool {
    // SAFETY: no side effects.
    unsafe { ef_twin_error_failed(code) != 0 }
}

/// Whether the kernel's error `code` is a device capacity (the CPU engine has none): a game
/// stopped there is outside the comparison.
pub fn is_capacity(code: i32) -> bool {
    // SAFETY: no side effects.
    unsafe { ef_twin_error_capacity(code) != 0 }
}

/// The C compiler's size of EfMove.
pub fn twin_move_size() -> usize {
    // SAFETY: no arguments, no side effects.
    unsafe { ef_twin_move_size() as usize }
}

/// One decision on the host build: the elapsed-turns flag, or why it stopped (the block is
/// then unspecified, as the engine's state is on an error).
pub fn twin_advance(g: &mut crate::state::Block, t: &DeviceTables, a: &Action) -> Result<u8, Stop> {
    let m = EfMove::of(a);
    let (mut elapsed, mut arg) = (0, 0);
    // SAFETY: valid blocks; the C side writes the flag and the argument.
    let code = unsafe { ef_twin_advance(g.as_mut_ptr(), &*t.block, &m, &mut elapsed, &mut arg) };
    if code == 0 {
        return Ok(elapsed as u8);
    }
    // SAFETY: no side effects.
    let failed = unsafe { ef_twin_error_failed(code) } != 0;
    Err(Stop {
        code,
        arg,
        text: t.error_text(code, arg),
        failed,
    })
}

/// step.rs command on the host build: one command alone, no Nature turn, no decision count.
pub fn twin_command(g: &mut crate::state::Block, t: &DeviceTables, a: &Action) -> Result<(), Stop> {
    let m = EfMove::of(a);
    let mut arg = 0;
    // SAFETY: valid blocks; the C side writes the argument.
    let code = unsafe { ef_twin_command(g.as_mut_ptr(), &*t.block, &m, &mut arg) };
    if code == 0 {
        return Ok(());
    }
    // SAFETY: no side effects.
    let failed = unsafe { ef_twin_error_failed(code) } != 0;
    Err(Stop {
        code,
        arg,
        text: t.error_text(code, arg),
        failed,
    })
}

/// The city name the port draws for a city of `tribe` and `skin` at (x, y) (capture.rs
/// city_name), on the host build; the block's naming state advances as in the engine.
pub fn twin_city_name(
    g: &mut crate::state::Block,
    t: &DeviceTables,
    tribe: i32,
    skin: i32,
    x: i32,
    y: i32,
) -> Result<String, Stop> {
    let mut word = [0i32; 16];
    let (mut len, mut arg) = (0, 0);
    // SAFETY: valid blocks; the word holds EF_WORD_UNITS (16) units.
    let code = unsafe {
        ef_twin_city_name(
            g.as_mut_ptr(),
            &*t.block,
            tribe,
            skin,
            x,
            y,
            word.as_mut_ptr(),
            &mut len,
            &mut arg,
        )
    };
    if code != 0 {
        // SAFETY: no side effects.
        let failed = unsafe { ef_twin_error_failed(code) } != 0;
        return Err(Stop {
            code,
            arg,
            text: t.error_text(code, arg),
            failed,
        });
    }
    let units: Vec<u16> = word[..len as usize].iter().map(|&u| u as u16).collect();
    Ok(String::from_utf16(&units).expect("the port's word is UTF-16"))
}

#[cfg(feature = "cuda")]
extern "C" {
    fn ef_cuda_advance(
        games: *mut EfGame,
        count: i32,
        tile_cap: i32,
        unit_cap: i32,
        t: *const crate::tables::EfTables,
        moves: *const EfMove,
        results: *mut crate::tables::KernelResult,
        elapsed: *mut i32,
    ) -> i32;
}

/// One decision in each block on the GPU, the blocks updated in place: per block the kernel's
/// result and the elapsed-turns flag. The outer `Err` is a CUDA failure.
#[cfg(feature = "cuda")]
pub fn device_advance(
    games: &mut [crate::state::Block],
    t: &DeviceTables,
    actions: &[Action],
) -> Result<Vec<(crate::tables::KernelResult, i32)>, String> {
    assert_eq!(games.len(), actions.len());
    let (mut flat, (tile_cap, unit_cap)) = crate::state::pack(games);
    let moves: Vec<EfMove> = actions.iter().map(EfMove::of).collect();
    let mut results = vec![crate::tables::KernelResult::default(); games.len()];
    let mut elapsed = vec![0i32; games.len()];
    // SAFETY: `flat` holds `count` blocks of these capacities, copied by the C side in both
    // directions; the other arrays have one entry per block.
    let code = unsafe {
        ef_cuda_advance(
            flat.as_mut_ptr().cast(),
            games.len() as i32,
            tile_cap,
            unit_cap,
            &*t.block,
            moves.as_ptr(),
            results.as_mut_ptr(),
            elapsed.as_mut_ptr(),
        )
    };
    if code != 0 {
        return Err(format!("CUDA error {code}"));
    }
    crate::state::unpack(&flat, games);
    Ok(results.into_iter().zip(elapsed).collect())
}

extern "C" {
    fn ef_twin_coverage_reset();
    fn ef_twin_coverage_read(commands: *mut i64, actions: *mut i64);
}

extern "C" {
    fn ef_twin_scratch_overlaps() -> i64;
}

/// How often the host build took a search scratch while another was held, in this process:
/// the device keeps the three kinds in one region per thread, which is sound only while this
/// stays zero (kernel/movement.h EF_SCRATCH_TAKE).
pub fn scratch_overlaps() -> i64 {
    // SAFETY: reads a counter.
    unsafe { ef_twin_scratch_overlaps() }
}

/// How often each command (by `EF_C_*`) and action kind (by `EF_A_*`) ran on the host build
/// since the last `coverage_reset`. The counters are global: tests that read them must not run
/// host steps on other threads meanwhile.
pub fn coverage() -> ([i64; 32], [i64; 96]) {
    let (mut c, mut a) = ([0i64; 32], [0i64; 96]);
    // SAFETY: the C side copies its two arrays, of these sizes.
    unsafe { ef_twin_coverage_read(c.as_mut_ptr(), a.as_mut_ptr()) };
    (c, a)
}

pub fn coverage_reset() {
    // SAFETY: no arguments.
    unsafe { ef_twin_coverage_reset() }
}

/// The command names, in `EF_C_*` order.
pub const COMMANDS: [&str; 31] = [
    "Research",
    "PeaceTreaty",
    "Build",
    "Destroy",
    "BreakPeace",
    "PeaceResponse",
    "Move",
    "Train",
    "Disembark",
    "Upgrade",
    "EstablishEmbassy",
    "Attack",
    "Capture",
    "CityReward",
    "ExamineRuins",
    "HealOthers",
    "Promote",
    "Resign",
    "EndTurn",
    "EndMatch",
    "Recover",
    "Disband",
    "StartMatch",
    "FreezeArea",
    "BreakIce",
    "ClearTileEffect",
    "Swarm",
    "Boost",
    "Explode",
    "Decompose",
    "Flood",
];
