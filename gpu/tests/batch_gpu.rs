//! The batch on the GPU (`GpuBatch`) against the CPU batch
//! (`engine_fast_batch::Batch`), slot for slot. Both start the same games and take the same
//! move indices; after every call each slot's status, reason, winner, legal rows and view must
//! be equal, and every tenth step its state too. Random play is a smoke check, not
//! verification.
#![cfg(feature = "cuda")]

use engine_fast::reset::Config;
use engine_fast_batch::{Applied, Batch, Status};
use engine_fast_gpu::batch::GpuBatch;
use engine_fast_gpu::state as device;

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));

fn compare(cpu: &Batch, gpu: &mut GpuBatch, at: &str) {
    for k in 0..cpu.len() {
        let s = cpu.status(k);
        assert_eq!(
            gpu.status(k),
            s,
            "{at}, slot {k}: status ({})",
            gpu.reason(k)
        );
        if s != Status::Failed {
            assert_eq!(gpu.reason(k), cpu.reason(k), "{at}, slot {k}: reason");
        }
        assert_eq!(gpu.winner(k), cpu.winner(k), "{at}, slot {k}: winner");
        assert_eq!(gpu.moves(k), cpu.moves(k), "{at}, slot {k}: rows");
        assert_eq!(gpu.view(k).unwrap(), cpu.view(k), "{at}, slot {k}: view");
    }
}

#[test]
fn the_gpu_batch_equals_the_cpu_batch() {
    let slots = 48;
    let mut cpu = Batch::new(CATALOG, slots, 4).unwrap();
    // boards of every priority side and two larger ones (20 and 30: random play, a smoke test)
    // in one batch sized for the largest
    let mut gpu = GpuBatch::new(CATALOG, slots, 4, 30).unwrap();
    let configs: Vec<(usize, Config)> = (0..slots)
        .map(|k| {
            let seats = 2 + k % 3;
            (
                k,
                Config {
                    rules: 124,
                    seed: 1000 + k as i32,
                    size: [11, 14, 16, 20, 30][k % 5],
                    map: 2,
                    mode: 4,
                    tribes: vec![[7, 7, 2, 4][k % 4]; seats],
                    action_limit: 300,
                },
            )
        })
        .collect();
    cpu.reset(&configs).unwrap();
    gpu.reset(&configs).unwrap();
    compare(&cpu, &mut gpu, "after the reset");
    let mut rng = 7u64;
    let (mut stepped, mut ended) = (0, 0);
    for decision in 0..320 {
        let moves: Vec<i32> = (0..slots)
            .map(|k| {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let n = cpu.moves(k).len();
                let r = (rng >> 33) as usize;
                match n {
                    0 => -1,
                    // a skip now and then, and an index past the list
                    _ if r % 29 == 0 => -1,
                    _ if r % 31 == 0 => n as i32,
                    _ => (r % n) as i32,
                }
            })
            .collect();
        let (mut a, mut b) = (vec![Applied::Skipped; slots], vec![Applied::Skipped; slots]);
        cpu.step(&moves, &mut a).unwrap();
        gpu.step(&moves, &mut b).unwrap();
        assert_eq!(a, b, "decision {decision}: applied");
        stepped += a.iter().filter(|x| **x == Applied::Stepped).count();
        let at = format!("decision {decision}");
        compare(&cpu, &mut gpu, &at);
        if decision % 10 == 0 {
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
                assert!(got == want, "{at}, slot {k}: the states differ");
            }
        }
    }
    for k in 0..slots {
        ended += usize::from(cpu.status(k) != Status::Playing);
    }
    println!("{stepped} moves stepped on both; {ended} of {slots} slots no longer playing");
}

/// A batch holds boards up to the side it was made for; a larger one is refused in its slot.
#[test]
fn a_board_larger_than_the_batch_is_refused() {
    let mut gpu = GpuBatch::new(CATALOG, 2, 1, 16).unwrap();
    let config = |size| Config {
        rules: 124,
        seed: 5,
        size,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit: 300,
    };
    gpu.reset(&[(0, config(16)), (1, config(20))]).unwrap();
    assert_eq!(gpu.status(0), Status::Playing);
    assert_eq!(gpu.status(1), Status::Refused);
    assert!(
        gpu.reason(1).contains("larger than the batch"),
        "{}",
        gpu.reason(1)
    );
    assert!(GpuBatch::new(CATALOG, 2, 1, 31).is_err());
}

/// A game of each special tribe plays on the device like any other.
#[test]
fn a_special_tribe_game_plays() {
    let t = engine_fast::tables::Tables::compile(CATALOG).unwrap();
    let special: Vec<i32> = ["aquarion", "polaris", "elyrion", "cymanti"]
        .iter()
        .map(|n| t.tribes.id_of[*n] as i32)
        .collect();
    let mut gpu = GpuBatch::new(CATALOG, 5, 1, 11).unwrap();
    let config = |tribes| Config {
        rules: 124,
        seed: 5,
        size: 11,
        map: 2,
        mode: 4,
        tribes,
        action_limit: 300,
    };
    let mut games = vec![(0, config(vec![7, 7]))];
    games.extend((0..4).map(|k| (k + 1, config(vec![7, special[k]]))));
    gpu.reset(&games).unwrap();
    assert_eq!(gpu.status(0), Status::Playing);
    for k in 0..5 {
        assert_eq!(gpu.status(k), Status::Playing, "{}", gpu.reason(k));
    }
    let mut applied = vec![Applied::Skipped; 5];
    gpu.step(&[0; 5], &mut applied).unwrap();
    assert!(applied.iter().all(|a| *a == Applied::Stepped));
}

/// A slot runs only while its block has passed the upload check: asked to take row 0 of a block
/// that failed it (no refresh first), the slot stops with the check's reason and is not stepped.
#[test]
fn a_slot_whose_block_failed_its_check_is_not_stepped() {
    let t = engine_fast::tables::Tables::compile(CATALOG).unwrap();
    let flat = engine_fast_gpu::tables::DeviceTables::new(&t).unwrap();
    let config = Config {
        rules: 124,
        seed: 5,
        size: 11,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit: 300,
    };
    let g = engine_fast::reset::reset(&config, CATALOG, &t).unwrap();
    let mut block = device::to_device(&g).unwrap();
    block.players[0].available_tech_len = 57;
    let caps = (block.tile_cap, block.unit_cap);
    let mut d = engine_fast_gpu::device::DeviceBatch::new(1, caps, &flat).unwrap();
    d.upload(0, &block).unwrap();
    d.run_slots(&[0], 32).unwrap();
    let out = d.read_slots().unwrap();
    assert!(out.results[0].code != 0);
    assert!(flat
        .error_text(out.results[0].code, out.results[0].arg)
        .starts_with("the device state is inconsistent"));
    assert_eq!(out.counts[0], 0);
    assert!(d.download(usize::MAX).is_err());
}
