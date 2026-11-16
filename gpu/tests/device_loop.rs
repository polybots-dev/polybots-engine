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

fn configs(slots: usize, size: i32, seed: i32) -> Vec<(usize, Config)> {
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
                    tribes: vec![[7, 7, 2, 4][k % 4]; 2 + k % 3],
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

fn read(x: &Exports, n: usize) -> Read {
    let mut status = vec![0i32; n];
    let mut counts = vec![0i32; n];
    let mut applied = vec![0i32; n];
    peek(x.status, &mut status).unwrap();
    peek(x.counts, &mut counts).unwrap();
    peek(x.applied, &mut applied).unwrap();
    let mut rows = Vec::new();
    let mut tiles = Vec::new();
    for g in 0..n as u64 {
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
    peek(x.scalars, &mut scalars).unwrap();
    peek(x.players, &mut players).unwrap();
    peek(x.tech, &mut tech_flat).unwrap();
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

/// The exported arrays equal the CPU batch, in the Python package's layout: -1 rows for slots
/// not playing; no list row past a slot's count is anything but zero.
fn compare(cpu: &Batch, x: &Exports, at: &str) {
    let n = cpu.len();
    let r = read(x, n);
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
            peek(x.previews + k as u64 * x.max_rows as u64 * 8, &mut pv).unwrap();
            let want = cpu.previews(k);
            assert_eq!(&pv[..want.len()], &want[..], "{at}, slot {k}: previews");
            assert!(
                pv[want.len()..].iter().all(|p| *p == [0; 4]),
                "{at}, slot {k}: a stale preview"
            );
        }
        if x.defence != 0 {
            let mut dp = vec![0i16; cells];
            peek(x.defence + (k * cells * 2) as u64, &mut dp).unwrap();
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
    let mut cpu = Batch::new(CATALOG, slots, 4).unwrap();
    let mut gpu = GpuBatch::with_outputs(CATALOG, slots, 4, size as usize, outputs).unwrap();
    let starts = configs(slots, size, 3000 + size * 100);
    cpu.reset(&starts).unwrap();
    gpu.reset(&starts).unwrap();
    let x = gpu.exports().unwrap();
    assert_eq!(x.cells, size * size);
    assert_eq!(
        (x.previews != 0, x.defence != 0),
        (outputs.previews, outputs.defence)
    );
    compare(&cpu, &x, &format!("size {size}, after the reset"));
    let mut actions = Buffer::new(4 * slots as u64).unwrap();
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
            gpu.reset(&again).unwrap();
            let at = format!("size {size}, call {call}, after the resets");
            compare(&cpu, &x, &at);
            assert_ne!(cpu.status(1), Status::Playing, "{at}: the failing reset");
        }
        let moves = choose(&cpu, &mut rng);
        actions.write(&moves).unwrap();
        let word = gpu.step_device(actions.ptr, actions.bytes).unwrap();
        cpu.step(&moves, &mut applied).unwrap();
        let at = format!("size {size}, call {call}");
        compare(&cpu, &x, &at);
        let r = read(&x, slots);
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
        assert_eq!(gpu.status(k), cpu.status(k));
        assert_eq!(gpu.moves(k), cpu.moves(k));
        assert_eq!(gpu.winner(k), cpu.winner(k));
        if cpu.status(k) != Status::Failed {
            assert_eq!(gpu.reason(k), cpu.reason(k));
        }
    }
    assert!(
        stepped > slots * calls / 2 && rejected > 0,
        "{stepped} stepped, {rejected} rejected"
    );
    println!("size {size}: {stepped} decisions and {rejected} rejections equal, read in place");
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
