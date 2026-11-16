//! The device loop's throughput: a GpuBatch stepped by an on-device
//! random policy (a uniform legal row, never Resign), the views and legal lists written on the
//! device each step and left there, only the status word read back. Two figures per run:
//! step-only (no game is restarted; finished slots sit idle) and with resets (every RESET_EVERY
//! steps the finished slots are restarted on the CPU and uploaded, as a training loop would).
//!
//!   loop_bench SIZE GAMES CALLS WORKERS OUTPUTS   (OUTPUTS 1: with the previews and the
//!   defence plane, 0 without)
//!
//! Prints one JSON line per mode.

use engine_fast::reset::Config;
use engine_fast_batch::Status;
use engine_fast_gpu::batch::GpuBatch;
use engine_fast_gpu::device::{Buffer, Outputs};
use std::time::Instant;

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
const RESET_EVERY: usize = 16;

fn config(size: i32, seed: i32) -> Config {
    Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit: 10_000,
    }
}

fn main() {
    let a: Vec<usize> = std::env::args()
        .skip(1)
        .map(|x| x.parse().unwrap())
        .collect();
    let [size, games, calls, workers, outputs] = a[..] else {
        eprintln!("usage: loop_bench SIZE GAMES CALLS WORKERS OUTPUTS");
        std::process::exit(2);
    };
    let gpu_name = engine_fast_gpu::floats::device_name().unwrap_or_default();
    for resets in [false, true] {
        let wanted = Outputs {
            previews: outputs == 1,
            defence: outputs == 1,
        };
        let mut b = GpuBatch::with_outputs(CATALOG, games, workers, size, wanted).unwrap();
        let mut seed = 1;
        let starts: Vec<(usize, Config)> = (0..games)
            .map(|k| {
                seed += 1;
                (k, config(size as i32, seed))
            })
            .collect();
        b.reset(&starts).unwrap();
        let actions = Buffer::new(4 * games as u64).unwrap();
        let (mut decisions, mut restarted) = (0u64, 0u64);
        let (mut step_s, mut policy_s, mut reset_s) = (0.0, 0.0, 0.0);
        let mut not_playing = 0u32;
        let traffic_before = b.traffic();
        let at = Instant::now();
        for call in 0..calls {
            let t = Instant::now();
            b.random_actions(call as u64 + 1, &actions).unwrap();
            policy_s += t.elapsed().as_secs_f64();
            let t = Instant::now();
            let playing = games as u64 - u64::from(not_playing);
            let word = b.step_device(actions.ptr, actions.bytes).unwrap();
            step_s += t.elapsed().as_secs_f64();
            decisions += playing;
            not_playing = word[0];
            if resets && call % RESET_EVERY == RESET_EVERY - 1 && not_playing > 0 {
                let t = Instant::now();
                b.sync().unwrap();
                let done: Vec<(usize, Config)> = (0..games)
                    .filter(|&k| b.status(k) != Status::Playing)
                    .map(|k| {
                        seed += 1;
                        (k, config(size as i32, seed))
                    })
                    .collect();
                restarted += done.len() as u64;
                b.reset(&done).unwrap();
                not_playing = (0..games)
                    .filter(|&k| b.status(k) != Status::Playing)
                    .count() as u32;
                reset_s += t.elapsed().as_secs_f64();
            }
        }
        let secs = at.elapsed().as_secs_f64();
        let traffic = b.traffic();
        println!(
            "{}",
            serde_json::json!({"mode": if resets { "with_resets" } else { "step_only" },
                "size": size, "games": games, "outputs": outputs == 1, "calls": calls, "workers": workers, "gpu": gpu_name,
                "decisions": decisions, "restarted": restarted, "seconds": secs,
                "decisions_per_second": decisions as f64 / secs,
                "step_decisions_per_second": decisions as f64 / step_s,
                "step_seconds": step_s, "policy_seconds": policy_s, "reset_seconds": reset_s,
                "bytes_to_device": traffic.0 - traffic_before.0, "bytes_to_host": traffic.1 - traffic_before.1,
                "includes": "on-device random policy, step, legal list and view per slot, status word; with_resets adds CPU restarts and uploads of finished slots"})
        );
    }
}
