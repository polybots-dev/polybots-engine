//! End-to-end batch throughput, GPU against CPU: the batch interface
//! as a trainer drives it. Every slot takes a uniformly random legal move per call other than
//! Resign (which would end most two-seat games within a few decisions and make this a
//! measure of game start), chosen on the host from the rows the call handed out; finished
//! slots are reset in place.
//!
//!   batch_bench BACKEND SIZE GAMES CALLS WORKERS VIEWS
//!
//! BACKEND is cpu or gpu; VIEWS is 1 to read every slot's view after each call (as a trainer
//! must) and 0 not to. Prints one JSON line: decisions per second over the timed calls (the
//! first call is not timed), what the time includes (the step call, the host's choice of
//! moves, resets of finished slots, and views when VIEWS is 1; not the reset at the start),
//! and the machine's GPU name.

use engine_fast::reset::Config;
use engine_fast_batch::{Applied, Batch, Status};
use engine_fast_gpu::batch::GpuBatch;
use std::time::Instant;

const RESIGN: i32 = 35;
const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));

#[allow(clippy::large_enum_variant)] // one value for the whole run
enum Backend {
    Cpu(Batch),
    Gpu(Box<GpuBatch>),
}

impl Backend {
    fn status(&self, k: usize) -> Status {
        match self {
            Backend::Cpu(b) => b.status(k),
            Backend::Gpu(b) => b.status(k),
        }
    }
    fn rows(&self, k: usize) -> &[[i32; 6]] {
        match self {
            Backend::Cpu(b) => b.moves(k),
            Backend::Gpu(b) => b.moves(k),
        }
    }
    fn reset(&mut self, games: &[(usize, Config)]) {
        match self {
            Backend::Cpu(b) => b.reset(games).unwrap(),
            Backend::Gpu(b) => b.reset(games).unwrap(),
        }
    }
    fn step(&mut self, moves: &[i32], applied: &mut [Applied]) {
        match self {
            Backend::Cpu(b) => b.step(moves, applied).unwrap(),
            Backend::Gpu(b) => b.step(moves, applied).unwrap(),
        }
    }
    fn views(&mut self, n: usize) -> usize {
        let mut seen = 0;
        for k in 0..n {
            seen += usize::from(match self {
                Backend::Cpu(b) => b.view(k).is_some(),
                Backend::Gpu(b) => b.view(k).unwrap().is_some(),
            });
        }
        seen
    }
}

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
    let a: Vec<String> = std::env::args().collect();
    let [_, backend, size, games, calls, workers, views] = &a[..] else {
        eprintln!("usage: batch_bench cpu|gpu SIZE GAMES CALLS WORKERS VIEWS");
        std::process::exit(2);
    };
    let (size, games, calls, workers) = (
        size.parse::<i32>().unwrap(),
        games.parse::<usize>().unwrap(),
        calls.parse::<usize>().unwrap(),
        workers.parse::<usize>().unwrap(),
    );
    let views = views == "1";
    let free_before = engine_fast_gpu::device::memory().map_or(0, |m| m.0);
    let mut b = match backend.as_str() {
        "cpu" => Backend::Cpu(Batch::new(CATALOG, games, workers).unwrap()),
        "gpu" => Backend::Gpu(Box::new(
            GpuBatch::new(CATALOG, games, workers, size as usize).unwrap(),
        )),
        _ => panic!("backend cpu or gpu"),
    };
    let mut seed = 1;
    let start: Vec<(usize, Config)> = (0..games)
        .map(|k| {
            seed += 1;
            (k, config(size, seed))
        })
        .collect();
    b.reset(&start);
    let mut rng = 0x2545_F491_4F6C_DD1Du64;
    let mut applied = vec![Applied::Skipped; games];
    let (mut decisions, mut resets) = (0usize, 0usize);
    let mut timed = None;
    let (mut step_secs, mut view_secs, mut reset_secs) = (0.0, 0.0, 0.0);
    for call in 0..=calls {
        if call == 1 {
            timed = Some(Instant::now());
            decisions = 0;
            resets = 0;
        }
        let moves: Vec<i32> = (0..games)
            .map(|k| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                let rows = b.rows(k);
                let choices: Vec<usize> =
                    (0..rows.len()).filter(|&i| rows[i][0] != RESIGN).collect();
                if b.status(k) != Status::Playing || rows.is_empty() {
                    -1
                } else if choices.is_empty() {
                    0
                } else {
                    choices[(rng % choices.len() as u64) as usize] as i32
                }
            })
            .collect();
        let t = Instant::now();
        b.step(&moves, &mut applied);
        let stepped = t.elapsed().as_secs_f64();
        decisions += applied.iter().filter(|x| **x == Applied::Stepped).count();
        let t = Instant::now();
        if views {
            b.views(games);
        }
        let viewed = t.elapsed().as_secs_f64();
        let done: Vec<(usize, Config)> = (0..games)
            .filter(|&k| b.status(k) != Status::Playing)
            .map(|k| {
                seed += 1;
                (k, config(size, seed))
            })
            .collect();
        resets += done.len();
        let t = Instant::now();
        if !done.is_empty() {
            b.reset(&done);
        }
        let reset = t.elapsed().as_secs_f64();
        if call > 0 {
            step_secs += stepped;
            view_secs += viewed;
            reset_secs += reset;
        }
    }
    let secs = timed.unwrap().elapsed().as_secs_f64();
    let gpu = engine_fast_gpu::floats::device_name().unwrap_or_default();
    // the batch's device memory, its slots' blocks, views, lists and scratch (other consumers
    // of the GPU change between the two readings too: the figure is for a quiet GPU)
    let device_bytes =
        free_before.saturating_sub(engine_fast_gpu::device::memory().map_or(0, |m| m.0));
    println!(
        "{}",
        serde_json::json!({"backend": backend, "size": size, "games": games, "calls": calls,
            "workers": workers, "views": views, "decisions": decisions, "resets": resets,
            "seconds": secs, "decisions_per_second": decisions as f64 / secs, "gpu": gpu,
            "step_seconds": step_secs, "view_seconds": view_secs, "reset_seconds": reset_secs,
            "step_decisions_per_second": decisions as f64 / step_secs,
            "device_bytes": device_bytes,
            "includes": "step call, host move choice, resets of finished slots, views if asked"})
    );
}
