//! The device loop (views, legal moves and actions stay
//! on the GPU). A `GpuBatch` steps by actions in device memory; its exported arrays, read
//! through their device addresses and layouts as a consumer (torch) reads them, must equal the
//! CPU batch's status, legal rows, views and applied codes on boards 11, 16 and 20; and a step
//! copies the same few bytes to the host whatever the number of games.
//!
//! Random play: a check of the device path's plumbing, not verification. Not covered here:
//! the Python wrapper.
#![cfg(feature = "cuda")]

use engine_fast::reset::Config;
use engine_fast_batch::{Applied, Batch, Status, SKIP};
use engine_fast_gpu::batch::GpuBatch;
use engine_fast_gpu::device::{peek, Buffer, Exports, Outputs};

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
const RESIGN: i32 = 35;

const REGULAR: [i32; 4] = [7, 7, 2, 4];

fn configs(slots: usize, size: i32, seed: i32) -> Vec<(usize, Config)> {
    configs_of(slots, size, seed, &REGULAR)
}

/// Slot k plays `tribes[k % len]` in every seat, two to four seats.
fn configs_of(slots: usize, size: i32, seed: i32, tribes: &[i32]) -> Vec<(usize, Config)> {
    (0..slots)
        .map(|k| {
            (
                k,
                Config {
                    rules: 124,
                    seed: seed + k as i32,
                    size,
                    map: 2,
                    mode: 4,
                    tribes: vec![tribes[k % tribes.len()]; 2 + k % 3],
                    action_limit: 400,
                },
            )
        })
        .collect()
}

/// Everything the batch exports, copied from the device through the exported layout.
struct Read {
    status: Vec<i32>,
    counts: Vec<i32>,
    applied: Vec<i32>,
    rows: Vec<Vec<[i32; 6]>>,
    tiles: Vec<Vec<i16>>,
    scalars: Vec<[i32; 3]>,
    players: Vec<[i64; 16]>,
    tech: Vec<Vec<i16>>,
}

/// Slots `first..first + n` of the exports.
fn read(x: &Exports, first: usize, n: usize) -> Read {
    let f = first as u64;
    let mut status = vec![0i32; n];
    let mut counts = vec![0i32; n];
    let mut applied = vec![0i32; n];
    peek(x.status + 4 * f, &mut status).unwrap();
    peek(x.counts + 4 * f, &mut counts).unwrap();
    peek(x.applied + 4 * f, &mut applied).unwrap();
    let mut rows = Vec::new();
    let mut tiles = Vec::new();
    for g in f..f + n as u64 {
        let mut r = vec![[0i32; 6]; x.max_rows as usize];
        peek(x.rows + g * x.menu_stride, &mut r).unwrap();
        rows.push(r);
        let mut t = vec![0i16; (x.planes * x.cells) as usize];
        peek(x.tiles + g * x.view_stride, &mut t).unwrap();
        tiles.push(t);
    }
    let mut scalars = vec![[0i32; 3]; n];
    let mut players = vec![[0i64; 16]; n];
    let mut tech_flat = vec![0i16; n * x.techs as usize];
    peek(x.scalars + 12 * f, &mut scalars).unwrap();
    peek(x.players + 128 * f, &mut players).unwrap();
    peek(x.tech + 2 * f * x.techs as u64, &mut tech_flat).unwrap();
    let tech = tech_flat
        .chunks(x.techs.max(1) as usize)
        .map(|c| c.to_vec())
        .collect();
    Read {
        status,
        counts,
        applied,
        rows,
        tiles,
        scalars,
        players,
        tech,
    }
}

/// The exported arrays of device slots `first..` equal the CPU batch's slots `0..`, in the
/// Python package's layout: -1 rows for slots not playing; no list row past a slot's count is
/// anything but zero.
fn compare(cpu: &Batch, x: &Exports, first: usize, at: &str) {
    let n = cpu.len();
    let r = read(x, first, n);
    let cells = x.cells as usize;
    for k in 0..n {
        let s = cpu.status(k);
        assert_eq!(
            r.status[k],
            s as i32,
            "{at}, slot {k}: status ({})",
            cpu.reason(k)
        );
        let want = cpu.moves(k);
        assert_eq!(
            r.counts[k] as usize,
            want.len(),
            "{at}, slot {k}: row count"
        );
        assert_eq!(&r.rows[k][..want.len()], want, "{at}, slot {k}: rows");
        assert!(
            r.rows[k][want.len()..].iter().all(|row| *row == [0; 6]),
            "{at}, slot {k}: a row past the count is not cleared"
        );
        let mut tiles = vec![-1i16; x.planes as usize * cells];
        let mut scalars = [-1i32; 3];
        let mut players = [-1i64; 16];
        let mut tech = vec![-1i16; x.techs as usize];
        if let Some(v) = cpu.view(k) {
            tiles.copy_from_slice(&v.tiles);
            scalars = [i32::from(v.viewer), v.turn, v.stars];
            for (p, q) in v.players.iter().enumerate() {
                players[p * 4..p * 4 + 4].copy_from_slice(&[
                    i64::from(q.seat),
                    i64::from(q.tribe),
                    i64::from(q.relation),
                    q.score,
                ]);
            }
            tech[..v.tech.len()].copy_from_slice(&v.tech);
        }
        assert!(r.tiles[k] == tiles, "{at}, slot {k}: view planes");
        assert_eq!(r.scalars[k], scalars, "{at}, slot {k}: scalars");
        assert_eq!(r.players[k], players, "{at}, slot {k}: players");
        assert_eq!(r.tech[k], tech, "{at}, slot {k}: tech");
        if x.previews != 0 {
            let mut pv = vec![[0i16; 4]; x.max_rows as usize];
            peek(
                x.previews + (first + k) as u64 * x.max_rows as u64 * 8,
                &mut pv,
            )
            .unwrap();
            let want = cpu.previews(k);
            assert_eq!(&pv[..want.len()], &want[..], "{at}, slot {k}: previews");
            assert!(
                pv[want.len()..].iter().all(|p| *p == [0; 4]),
                "{at}, slot {k}: a stale preview"
            );
        }
        if x.defence != 0 {
            let mut dp = vec![0i16; cells];
            peek(x.defence + ((first + k) * cells * 2) as u64, &mut dp).unwrap();
            let want = cpu.defence(k).unwrap().unwrap_or_else(|| vec![-1; cells]);
            assert_eq!(dp, want, "{at}, slot {k}: defence");
        }
    }
}

/// Moves for the CPU batch and the same as device actions: mostly a random legal row, and now
/// and then a skip, a row past the list, or a move for a slot not playing.
fn choose(cpu: &Batch, rng: &mut u64) -> Vec<i32> {
    (0..cpu.len())
        .map(|k| {
            *rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = (*rng >> 33) as usize;
            let rows = cpu.moves(k);
            match r % 23 {
                0 => SKIP,
                1 => rows.len() as i32 + 3,
                2 => -7,
                _ if cpu.status(k) != Status::Playing => (r % 5) as i32,
                _ => {
                    let kept: Vec<usize> =
                        (0..rows.len()).filter(|&i| rows[i][0] != RESIGN).collect();
                    if kept.is_empty() {
                        0
                    } else {
                        kept[(r / 23) % kept.len()] as i32
                    }
                }
            }
        })
        .collect()
}

fn lockstep(size: i32, slots: usize, calls: usize, outputs: Outputs) {
    lockstep_of(size, slots, 0, calls, outputs, &REGULAR, 4);
}

/// A GPU batch of `total` slots, of which slots `first..` are played against a CPU batch
/// holding just them (the CPU batch's slot k is device slot first + k); the other device slots
/// start their games and are skipped at every step.
fn lockstep_of(
    size: i32,
    total: usize,
    first: usize,
    calls: usize,
    outputs: Outputs,
    tribes: &[i32],
    workers: usize,
) {
    let slots = total - first;
    let mut cpu = Batch::new(CATALOG, slots, workers).unwrap();
    let mut gpu = GpuBatch::with_outputs(CATALOG, total, workers, size as usize, outputs).unwrap();
    let starts = configs_of(total, size, 3000 + size * 100, tribes);
    let shifted = |c: &[(usize, Config)]| -> Vec<(usize, Config)> {
        c.iter().map(|(k, cfg)| (k + first, cfg.clone())).collect()
    };
    let compared: Vec<(usize, Config)> = starts[first..]
        .iter()
        .map(|(k, cfg)| (k - first, cfg.clone()))
        .collect();
    cpu.reset(&compared).unwrap();
    gpu.reset(&starts).unwrap();
    let x = gpu.exports().unwrap();
    assert_eq!(x.cells, size * size);
    assert_eq!(
        (x.previews != 0, x.defence != 0),
        (outputs.previews, outputs.defence)
    );
    compare(&cpu, &x, first, &format!("size {size}, after the reset"));
    let mut actions = Buffer::new(4 * total as u64).unwrap();
    let mut device_moves = vec![SKIP; total];
    let mut rng = 11u64 ^ size as u64;
    let mut applied = vec![Applied::Skipped; slots];
    let (mut stepped, mut rejected) = (0, 0);
    for call in 0..calls {
        if call % 40 == 39 {
            // games restarted mid-loop, one of them with a reset that fails (no such tribe): the
            // device stops that slot as the CPU batch refuses it
            let mut again = configs(2, size, 7000 + call as i32);
            again[1].0 = 1;
            again[1].1.tribes = vec![999, 999];
            cpu.reset(&again).unwrap();
            gpu.reset(&shifted(&again)).unwrap();
            let at = format!("size {size}, call {call}, after the resets");
            compare(&cpu, &x, first, &at);
            assert_ne!(cpu.status(1), Status::Playing, "{at}: the failing reset");
        }
        let moves = choose(&cpu, &mut rng);
        device_moves[first..].copy_from_slice(&moves);
        actions.write(&device_moves).unwrap();
        let word = gpu.step_device(actions.ptr, actions.bytes).unwrap();
        cpu.step(&moves, &mut applied).unwrap();
        let at = format!("size {size}, call {call}");
        compare(&cpu, &x, first, &at);
        let r = read(&x, first, slots);
        for (k, a) in applied.iter().enumerate().take(slots) {
            assert_eq!(r.applied[k], *a as i32, "{at}, slot {k}: applied");
        }
        let not_playing = (0..slots)
            .filter(|&k| cpu.status(k) != Status::Playing)
            .count();
        assert_eq!(word[0] as usize, not_playing, "{at}: the status word");
        stepped += applied.iter().filter(|a| **a == Applied::Stepped).count();
        rejected += applied.iter().filter(|a| **a == Applied::Rejected).count();
    }
    // the host calls see the device's state after a sync
    gpu.sync().unwrap();
    for k in 0..slots {
        assert_eq!(gpu.status(first + k), cpu.status(k));
        assert_eq!(gpu.moves(first + k), cpu.moves(k));
        assert_eq!(gpu.winner(first + k), cpu.winner(k));
        if cpu.status(k) != Status::Failed {
            assert_eq!(gpu.reason(first + k), cpu.reason(k));
        }
    }
    // the skipped slots below the window never moved
    let mut below = vec![0i32; first];
    peek(x.applied, &mut below).unwrap();
    assert!(
        below.iter().all(|a| *a == Applied::Skipped as i32),
        "a skipped slot was stepped"
    );
    assert!(
        stepped > slots * calls / 2 && rejected > 0,
        "{stepped} stepped, {rejected} rejected"
    );
    println!("size {size}: {stepped} decisions and {rejected} rejections equal at device slots {first}..{total}, read in place");
}

#[test]
fn the_device_arrays_equal_the_cpu_batch_at_11() {
    lockstep(11, 24, 120, Outputs::default());
}

#[test]
fn the_device_arrays_equal_the_cpu_batch_at_16() {
    lockstep(16, 24, 120, Outputs::default());
}

#[test]
fn the_device_arrays_equal_the_cpu_batch_at_20() {
    lockstep(20, 16, 120, Outputs::default());
}

fn env_or(name: &str, default: usize) -> usize {
    std::env::var(name).map_or(default, |v| v.parse().unwrap())
}

/// The device loop at a training-size batch: a GPU
/// batch of EF_BIG_SLOTS slots (default 65 536) on board EF_BIG_SIZE (default 11), whose top
/// EF_BIG_COMPARE slots (default 8 192: the offsets past 2^31 bytes of the legal rows) play
/// against the CPU batch for EF_BIG_CALLS calls (default 48: one round of mid-loop resets),
/// regular and special tribes mixed. Ignored by default: at 65 536 slots of 11 x 11 it needs
/// about 18 GB of device memory and 25 GB of host memory.
#[test]
#[ignore]
fn a_big_batch_equals_the_cpu_batch() {
    let t = engine_fast::tables::Tables::compile(CATALOG).unwrap();
    let mut tribes = REGULAR.to_vec();
    for name in ["aquarion", "polaris", "elyrion", "cymanti"] {
        tribes.push(t.tribes.id_of[name] as i32);
    }
    let slots = env_or("EF_BIG_SLOTS", 65_536);
    let size = env_or("EF_BIG_SIZE", 11) as i32;
    let calls = env_or("EF_BIG_CALLS", 48);
    let compared = env_or("EF_BIG_COMPARE", 8_192).min(slots);
    lockstep_of(
        size,
        slots,
        slots - compared,
        calls,
        Outputs::default(),
        &tribes,
        32,
    );
}

/// The optional outputs: with them asked for, the attack
/// previews and the defence plane, read in place, equal the CPU batch's on boards 11, 16 and 20.
#[test]
fn the_device_outputs_equal_the_cpu_batch() {
    let both = Outputs {
        previews: true,
        defence: true,
    };
    lockstep(11, 24, 160, both);
    lockstep(16, 16, 160, both);
    lockstep(20, 12, 120, both);
}

/// The copy guard: a device step copies nothing to the device and the same few bytes to the
/// host (the status word) whatever the number of games, so no per-game data crosses. Resets
/// upload new games by design and are counted apart.
#[test]
fn a_device_step_moves_no_per_game_data() {
    let mut per_step = Vec::new();
    for slots in [64usize, 1024] {
        let mut gpu = GpuBatch::new(CATALOG, slots, 8, 11).unwrap();
        gpu.reset(&configs(slots, 11, 77)).unwrap();
        let actions = Buffer::new(4 * slots as u64).unwrap();
        let before = gpu.traffic();
        let steps = 40;
        for s in 0..steps {
            gpu.random_actions(s, &actions).unwrap();
            gpu.step_device(actions.ptr, actions.bytes).unwrap();
        }
        let after = gpu.traffic();
        let to_device = (after.0 - before.0) / steps;
        let to_host = (after.1 - before.1) / steps;
        assert_eq!((after.0 - before.0) % steps, 0);
        assert_eq!((after.1 - before.1) % steps, 0);
        per_step.push((to_device, to_host));
    }
    assert_eq!(
        per_step[0], per_step[1],
        "bytes per step depend on the number of games"
    );
    assert_eq!(per_step[0], (0, 8), "a step copies only the status word");
}

/// `step_launch` and `step_wait` give what `step_device` gives, call by call, with another
/// stream's work (the benchmark's load) running while the step is in flight and restarts on the
/// default stream between steps.
#[test]
fn a_launched_step_equals_a_waited_one() {
    let slots = 256;
    let mut waited = GpuBatch::new(CATALOG, slots, 8, 11).unwrap();
    let mut launched = GpuBatch::new(CATALOG, slots, 8, 11).unwrap();
    waited.reset(&configs(slots, 11, 41)).unwrap();
    launched.reset(&configs(slots, 11, 41)).unwrap();
    let (wa, la) = (
        Buffer::new(4 * slots as u64).unwrap(),
        Buffer::new(4 * slots as u64).unwrap(),
    );
    let mut seed = 1000;
    for s in 0..60u64 {
        waited.random_actions(s, &wa).unwrap();
        launched.random_actions(s, &la).unwrap();
        let w = waited.step_device(wa.ptr, wa.bytes).unwrap();
        launched.step_launch(la.ptr, la.bytes).unwrap();
        engine_fast_gpu::device::load(4096, 4).unwrap();
        let l = launched.step_wait().unwrap();
        engine_fast_gpu::device::load_wait().unwrap();
        assert_eq!(w, l, "status word at step {s}");
        let (x, y) = (waited.exports().unwrap(), launched.exports().unwrap());
        let (a, b) = (read(&x, 0, slots), read(&y, 0, slots));
        assert_eq!(a.status, b.status, "status at step {s}");
        assert_eq!(a.counts, b.counts, "counts at step {s}");
        assert_eq!(a.applied, b.applied, "applied at step {s}");
        assert_eq!(a.rows, b.rows, "rows at step {s}");
        assert_eq!(a.tiles, b.tiles, "views at step {s}");
        assert_eq!(a.scalars, b.scalars, "scalars at step {s}");
        assert_eq!(a.players, b.players, "players at step {s}");
        assert_eq!(a.tech, b.tech, "tech at step {s}");
        if s % 16 == 15 {
            for gpu in [&mut waited, &mut launched] {
                gpu.sync().unwrap();
            }
            let done: Vec<usize> = (0..slots)
                .filter(|&k| waited.status(k) != Status::Playing)
                .collect();
            let fresh: Vec<(usize, Config)> = done
                .iter()
                .map(|&k| {
                    seed += 1;
                    (k, configs(1, 11, seed).remove(0).1)
                })
                .collect();
            waited.reset(&fresh).unwrap();
            launched.reset(&fresh).unwrap();
        }
    }
}

/// While a launched step is in flight the batch refuses its other device calls, and a wait
/// without a launch is refused.
#[test]
fn a_step_in_flight_refuses_other_calls() {
    let slots = 8;
    let mut gpu = GpuBatch::new(CATALOG, slots, 1, 11).unwrap();
    gpu.reset(&configs(slots, 11, 9)).unwrap();
    let actions = Buffer::new(4 * slots as u64).unwrap();
    assert!(gpu.step_wait().is_err());
    gpu.random_actions(1, &actions).unwrap();
    gpu.step_launch(actions.ptr, actions.bytes).unwrap();
    assert!(gpu.sync().is_err());
    assert!(gpu.reset(&configs(1, 11, 10)).is_err());
    assert!(gpu.step_device(actions.ptr, actions.bytes).is_err());
    assert!(gpu.step_launch(actions.ptr, actions.bytes).is_err());
    assert!(gpu.game(0).is_err());
    gpu.step_wait().unwrap();
    assert!(gpu.step_wait().is_err());
    gpu.sync().unwrap();
}

/// Actions that are not one i32 per slot in GPU memory are refused before any kernel runs.
#[test]
fn host_memory_actions_are_refused() {
    let mut gpu = GpuBatch::new(CATALOG, 4, 1, 11).unwrap();
    gpu.reset(&configs(4, 11, 5)).unwrap();
    let host = [0i32; 4];
    assert!(gpu.step_device(host.as_ptr() as u64, 16).is_err());
    let small = Buffer::new(8).unwrap();
    assert!(gpu.step_device(small.ptr, small.bytes).is_err());
}

/// A batch holding a board smaller than its largest refuses the device arrays (their planes would
/// not have one layout).
#[test]
fn mixed_boards_refuse_the_device_arrays() {
    let mut gpu = GpuBatch::new(CATALOG, 2, 1, 16).unwrap();
    gpu.reset(&configs(1, 16, 5)).unwrap();
    assert!(gpu.exports().is_ok());
    let mut small = configs(1, 11, 6);
    small[0].0 = 1;
    gpu.reset(&small).unwrap();
    assert!(gpu.exports().is_err());
    let actions = Buffer::new(8).unwrap();
    assert!(gpu.step_device(actions.ptr, actions.bytes).is_err());
}
