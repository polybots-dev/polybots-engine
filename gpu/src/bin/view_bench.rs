//! How fast the view kernel runs on the GPU, against `view::write` on one CPU core.
//!
//!   view_bench SIZE GAMES [THREADS_PER_BLOCK]
//!
//! The states are 512 different positions from random two-seat games on a SIZE x SIZE board,
//! repeated over GAMES slots; each slot's viewer is its player to move. The kernel is timed
//! alone (launch and wait, best and median of 20 runs), and the download of the views apart.
//! The views read back are compared with the CPU engine's for every slot. This is a figure
//! for the view only, not for the engine.

use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::view::{self, View};
use engine_fast::{menu, step};
use engine_fast_gpu::device::{self, DeviceBatch};
use engine_fast_gpu::state as blocks;
use engine_fast_gpu::tables::DeviceTables;
use std::time::Instant;

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));

fn positions(t: &Tables, size: i32, want: usize) -> Result<Vec<Game>, String> {
    let mut out = Vec::new();
    let mut seed = 900;
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
        for decision in 0..600 {
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
            if decision % 40 == 39 && out.len() < want {
                out.push(g.clone());
            }
        }
    }
    Ok(out)
}

fn main() -> Result<(), String> {
    let a: Vec<usize> = std::env::args()
        .skip(1)
        .map(|x| x.parse().map_err(|_| "numbers"))
        .collect::<Result<_, _>>()?;
    let (size, games) = match a[..] {
        [size, games, ..] => (size as i32, games),
        _ => return Err("usage: view_bench SIZE GAMES [THREADS_PER_BLOCK]".into()),
    };
    let threads = a.get(2).copied().unwrap_or(32);
    let t = Tables::compile(CATALOG)?;
    let tables = DeviceTables::new(&t)?;
    let states = positions(&t, size, 512)?;
    let viewer = |g: &Game| g.players[g.current_player_index as usize].action.id;
    // the CPU engine's views of the distinct positions, and its time for them on one core
    let mut want: Vec<View> = vec![View::default(); states.len()];
    let at = Instant::now();
    let rounds = 20;
    for _ in 0..rounds {
        for (g, out) in states.iter().zip(&mut want) {
            view::write(g, &t, viewer(g), out)?;
        }
    }
    let cpu_us = at.elapsed().as_secs_f64() * 1e6 / (rounds * states.len()) as f64;
    let (free_before, total) = device::memory()?;
    let at = Instant::now();
    let host: Vec<_> = states
        .iter()
        .map(blocks::to_device)
        .collect::<Result<_, _>>()?;
    let mut batch = DeviceBatch::new(games, (host[0].tile_cap, host[0].unit_cap), &tables)?;
    for slot in 0..games {
        batch.upload(slot, &host[slot % host.len()])?;
    }
    let upload_s = at.elapsed().as_secs_f64();
    let viewers: Vec<i32> = (0..games)
        .map(|slot| viewer(&states[slot % states.len()]))
        .collect();
    batch.run_views(&viewers, threads)?;
    let mut kernel: Vec<f64> = (0..20)
        .map(|_| {
            let at = Instant::now();
            batch
                .run_views(&viewers, threads)
                .map(|()| at.elapsed().as_secs_f64())
        })
        .collect::<Result<_, _>>()?;
    kernel.sort_by(f64::total_cmp);
    let at = Instant::now();
    let (views, results) = batch.read_views()?;
    let download_s = at.elapsed().as_secs_f64();
    let mut equal = 0;
    for slot in 0..games {
        if results[slot].code == 0 && views.to_view(slot)? == want[slot % want.len()] {
            equal += 1;
        }
    }
    let (free_after, _) = device::memory()?;
    println!(
        "{{\"size\":{size},\"games\":{games},\"threads_per_block\":{threads},\"views_equal\":{equal},\"kernel_ms_best\":{:.3},\"kernel_ms_median\":{:.3},\"gpu_views_per_second_best\":{:.0},\"gpu_views_per_second_median\":{:.0},\"download_ms\":{:.1},\"upload_s\":{:.2},\"cpu_one_core_us_per_view\":{:.2},\"cpu_one_core_views_per_second\":{:.0},\"device_bytes_used\":{},\"device_bytes_total\":{total}}}",
        kernel[0] * 1e3,
        kernel[kernel.len() / 2] * 1e3,
        games as f64 / kernel[0],
        games as f64 / kernel[kernel.len() / 2],
        download_s * 1e3,
        upload_s,
        cpu_us,
        1e6 / cpu_us,
        free_before - free_after,
    );
    if equal != games {
        return Err(format!(
            "only {equal} of {games} views equal the CPU engine's"
        ));
    }
    Ok(())
}
