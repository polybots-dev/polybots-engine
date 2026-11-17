//! Banked restarts (`GpuBatch::queue` and `restart_finished`): a slot restarted
//! from the bank must equal the CPU batch's `reset` of the same slot with the same
//! configuration, the games going to the finished slots in slot order and queue order; and the
//! restart copies only the status array to the host.
//!
//! Random play with short games, so that many slots finish and restart: a check of the
//! restart path's plumbing; the kernels' rules are checked against the CPU engine elsewhere.
#![cfg(feature = "cuda")]

use engine_fast::reset::Config;
use engine_fast_batch::{Applied, Batch, Status, SKIP};
use engine_fast_gpu::batch::GpuBatch;
use engine_fast_gpu::device::Buffer;
use engine_fast_gpu::state as device;

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));

fn config(size: i32, seed: i32, seats: usize) -> Config {
    Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes: vec![[7, 7, 2, 4][seed as usize % 4]; seats],
        action_limit: 40 + (seed as u32 % 50),
    }
}

fn next(rng: &mut u64) -> usize {
    *rng = rng
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (*rng >> 33) as usize
}

/// The host view of every slot equals the CPU batch's.
fn compare(cpu: &Batch, gpu: &mut GpuBatch, at: &str) {
    gpu.sync().unwrap();
    for k in 0..cpu.len() {
        assert_eq!(gpu.status(k), cpu.status(k), "{at}, slot {k}: status");
        assert_eq!(gpu.winner(k), cpu.winner(k), "{at}, slot {k}: winner");
        assert_eq!(gpu.moves(k), cpu.moves(k), "{at}, slot {k}: legal rows");
        if cpu.status(k) == Status::Refused {
            assert_eq!(gpu.reason(k), cpu.reason(k), "{at}, slot {k}: reason");
        }
    }
}

#[test]
fn a_banked_restart_equals_a_reset() {
    let (slots, size) = (24, 11);
    let mut cpu = Batch::new(CATALOG, slots, 4).unwrap();
    let mut gpu = GpuBatch::new(CATALOG, slots, 4, size as usize).unwrap();
    // nothing queued: nothing restarts, and the slots stay empty
    assert_eq!(gpu.restart_finished(true).unwrap(), Vec::<usize>::new());
    // a game on another board than the batch's is refused at the queue
    assert!(gpu.queue(vec![config(16, 1, 2)]).is_err());
    let starts: Vec<(usize, Config)> = (0..slots)
        .map(|k| (k, config(size, 100 + k as i32, 2 + k % 3)))
        .collect();
    cpu.reset(&starts).unwrap();
    gpu.reset(&starts).unwrap();
    compare(&cpu, &mut gpu, "after the reset");
    // the queue in the order the games are to be used, mirrored for the CPU batch
    let mut queued = std::collections::VecDeque::new();
    let mut seed = 1000;
    let mut queue = |gpu: &mut GpuBatch, queued: &mut std::collections::VecDeque<Config>, n| {
        let mut games = Vec::new();
        for i in 0..n {
            seed += 1;
            let mut c = config(size, seed, 2 + seed as usize % 3);
            if i == 0 || i == n - 1 {
                // no such tribe: the slot is refused; the first and last of a queue call, so
                // at a page's edges
                c.tribes = vec![999, 999];
            }
            games.push(c);
        }
        queued.extend(games.iter().cloned());
        gpu.queue(games).unwrap();
    };
    let mut actions = Buffer::new(4 * slots as u64).unwrap();
    let mut applied = vec![Applied::Skipped; slots];
    let mut rng = 5u64;
    let (mut restarted, mut short, mut refused) = (0, 0, 0);
    for call in 0..400 {
        if call % 50 == 0 {
            // sometimes fewer than will finish (the bank runs dry), sometimes more
            let n = if call % 100 == 0 { 3 } else { 40 };
            queue(&mut gpu, &mut queued, n);
        }
        if call % 3 == 2 {
            if call % 6 == 5 {
                // the host copy current before the restart, as well as stale
                compare(&cpu, &mut gpu, &format!("call {call}, before the restart"));
            }
            let finished = (0..slots)
                .filter(|&k| cpu.status(k) != Status::Playing)
                .count();
            let before = gpu.traffic();
            // every fourth restart takes only the games already built, at least as many as
            // were ready just before (more may be built by the time it looks)
            let wait = call % 4 != 0;
            let waiting = gpu.queued();
            let least = if wait { waiting } else { gpu.ready().unwrap() };
            let got = gpu.restart_finished(wait).unwrap();
            let after = gpu.traffic();
            assert_eq!(
                after.1 - before.1,
                4 * slots as u64,
                "call {call}: host bytes"
            );
            let want: Vec<usize> = (0..slots)
                .filter(|&k| cpu.status(k) != Status::Playing)
                .take(waiting)
                .collect();
            // the first finished slots in slot order, one per game used
            assert!(
                want.starts_with(&got) && got.len() >= least.min(finished),
                "call {call}: the slots restarted, {got:?} of {want:?}"
            );
            short += usize::from(finished > got.len());
            let games: Vec<(usize, Config)> = got
                .iter()
                .map(|&k| (k, queued.pop_front().unwrap()))
                .collect();
            refused += games.iter().filter(|(_, c)| c.tribes[0] == 999).count();
            restarted += games.len();
            cpu.reset(&games).unwrap();
            assert_eq!(gpu.queued(), queued.len());
            compare(&cpu, &mut gpu, &format!("call {call}, after the restart"));
            if call % 30 == 2 {
                for k in 0..slots {
                    let (Some(want), Some(got)) = (cpu.game(k), gpu.game(k).unwrap()) else {
                        continue;
                    };
                    let (want, mut got) = (
                        device::canonical(want).unwrap(),
                        device::canonical(&got).unwrap(),
                    );
                    got.side = want.side.clone();
                    got.memory = want.memory.clone();
                    assert!(got == want, "call {call}, slot {k}: the states differ");
                }
            }
        }
        let moves: Vec<i32> = (0..slots)
            .map(|k| {
                let n = cpu.moves(k).len();
                let r = next(&mut rng);
                if n == 0 || r.is_multiple_of(17) {
                    SKIP
                } else {
                    (r % n) as i32
                }
            })
            .collect();
        actions.write(&moves).unwrap();
        gpu.step_device(actions.ptr, actions.bytes).unwrap();
        cpu.step(&moves, &mut applied).unwrap();
    }
    compare(&cpu, &mut gpu, "at the end");
    assert!(restarted > 100, "{restarted} restarts");
    assert!(short > 0, "the bank never ran short");
    assert!(refused > 0, "no queued game was refused");
}
