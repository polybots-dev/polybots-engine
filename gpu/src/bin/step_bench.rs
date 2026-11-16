//! How fast the step of the kernel sources runs on the GPU, on games where every move ends the
//! turn (a workload the first ported rules cover completely), against the Rust engine and the
//! host build on one CPU core on the same positions.
//!
//!   step_bench SIZE GAMES CALLS [THREADS_PER_BLOCK]
//!
//! The starting positions are 256 different two-seat games after 0 to 120 random decisions,
//! repeated over the slots. Each kernel call takes one decision in every slot; a slot that
//! stops (a rule not ported, or the game ending) is not counted. After the run, every slot's
//! block is compared with the host build playing the same decisions. This is a figure for turn
//! ends only (upkeep, income, the winner check, the next player's start), not for the engine.

use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::{menu, step};
use engine_fast_frame::Action;
use engine_fast_gpu::device::DeviceBatch;
use engine_fast_gpu::state::{self as blocks, Block};
use engine_fast_gpu::step::{twin_advance, EfMove};
use engine_fast_gpu::tables::DeviceTables;
use std::time::Instant;

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));

fn positions(t: &Tables, size: i32, want: usize) -> Result<Vec<Game>, String> {
    let mut out = Vec::new();
    let mut seed = 500;
    while out.len() < want {
        seed += 1;
        let config = Config {
            rules: 124,
            seed,
            size,
            map: 2,
            mode: 4,
            tribes: vec![7, 7],
            action_limit: 10_000,
        };
        let mut g = reset::reset(&config, CATALOG, t)?;
        let mut rng = 0x9E37_79B9_7F4A_7C15u64 ^ seed as u64;
        let warm = (seed as usize * 37) % 121;
        for _ in 0..warm {
            let legal = menu::menu(&g, t)?;
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = (rng >> 33) as usize;
            let others: Vec<usize> = (0..legal.len())
                .filter(|&i| ![15, 35].contains(&legal[i].action.kind))
                .collect();
            let pick = match legal.iter().position(|c| c.action.kind == 15) {
                Some(e) if others.is_empty() || r % 10 == 0 => e,
                _ if !others.is_empty() => others[(r / 10) % others.len()],
                _ => break,
            };
            step::advance(&mut g, t, &legal[pick].action)?;
        }
        out.push(g);
    }
    Ok(out)
}

fn main() -> Result<(), String> {
    let a: Vec<usize> = std::env::args()
        .skip(1)
        .map(|x| x.parse().map_err(|_| "numbers"))
        .collect::<Result<_, _>>()?;
    let (size, games, calls) = match a[..] {
        [size, games, calls, ..] => (size as i32, games, calls),
        _ => return Err("usage: step_bench SIZE GAMES CALLS [THREADS_PER_BLOCK]".into()),
    };
    let threads = a.get(3).copied().unwrap_or(32);
    let t = Tables::compile(CATALOG)?;
    let tables = DeviceTables::new(&t)?;
    let states = positions(&t, size, 256)?;
    let end = Action {
        kind: 15,
        source: None,
        target: None,
        opponent: None,
        option: None,
        accept: None,
    };
    // one core: the Rust engine and the host build, CALLS turn ends from every position
    let (mut rust_decisions, at) = (0u64, Instant::now());
    for g in &states {
        let mut g = g.clone();
        for _ in 0..calls {
            if step::advance(&mut g, &t, &end).is_err() || g.current_state == 4 {
                break;
            }
            rust_decisions += 1;
        }
    }
    let rust_us = at.elapsed().as_secs_f64() * 1e6 / rust_decisions.max(1) as f64;
    let host_blocks: Vec<Block> = states
        .iter()
        .map(blocks::to_device)
        .collect::<Result<_, _>>()?;
    let (mut twin_decisions, at) = (0u64, Instant::now());
    let mut twin_after = Vec::new();
    for b in &host_blocks {
        let mut g = b.clone();
        let mut taken = 0usize;
        for _ in 0..calls {
            if twin_advance(&mut g, &tables, &end).is_err() {
                break;
            }
            taken += 1;
        }
        twin_decisions += taken as u64;
        twin_after.push((g, taken));
    }
    let twin_us = at.elapsed().as_secs_f64() * 1e6 / twin_decisions.max(1) as f64;
    // the GPU
    let caps = (host_blocks[0].tile_cap, host_blocks[0].unit_cap);
    let mut batch = DeviceBatch::new(games, caps, &tables)?;
    for slot in 0..games {
        batch.upload(slot, &host_blocks[slot % host_blocks.len()])?;
    }
    batch.set_moves(&vec![EfMove::of(&end); games])?;
    let mut stopped = vec![false; games];
    let (mut gpu_decisions, mut kernel_s) = (0u64, 0f64);
    for _ in 0..calls {
        let at = Instant::now();
        batch.run_advance(threads)?;
        kernel_s += at.elapsed().as_secs_f64();
        for (slot, r) in batch.read_results()?.iter().enumerate() {
            if r.code == 0 && !stopped[slot] {
                gpu_decisions += 1;
            } else {
                stopped[slot] = true;
            }
        }
    }
    // every slot equals the host build after the same number of decisions
    let mut equal = 0;
    for slot in 0..games {
        let (want, taken) = &twin_after[slot % twin_after.len()];
        if *taken == calls && !stopped[slot] && batch.download(slot)?.bytes() == want.bytes() {
            equal += 1;
        } else if *taken < calls && stopped[slot] {
            equal += 1;
        }
    }
    println!(
        "{{\"size\":{size},\"games\":{games},\"calls\":{calls},\"threads_per_block\":{threads},\"gpu_decisions\":{gpu_decisions},\"kernel_seconds\":{kernel_s:.4},\"gpu_decisions_per_second\":{:.0},\"slots_equal_to_host_build\":{equal},\"rust_one_core_us_per_decision\":{rust_us:.2},\"rust_one_core_decisions_per_second\":{:.0},\"host_build_one_core_us_per_decision\":{twin_us:.2},\"rust_decisions\":{rust_decisions},\"host_build_decisions\":{twin_decisions}}}",
        gpu_decisions as f64 / kernel_s,
        1e6 / rust_us,
    );
    if equal != games {
        return Err(format!(
            "only {equal} of {games} slots equal the host build"
        ));
    }
    Ok(())
}
