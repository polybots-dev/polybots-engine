//! The device loop's throughput: a GpuBatch stepped by an on-device
//! random policy (a uniform legal row, never Resign), the views and legal lists written on the
//! device each step and left there, only the status word read back. Two figures per run:
//! step-only (no game is restarted; finished slots sit idle) and with resets (every RESET_EVERY
//! steps the finished slots are restarted on the CPU and uploaded, as a training loop would).
//!
//!   loop_bench SIZE GAMES CALLS WORKERS OUTPUTS [WINDOW [LIMIT [MODES]]]   (OUTPUTS 1: with
//!   the previews and the defence plane, 0 without; WINDOW > 0: one JSON line per WINDOW calls
//!   with that window's figures and the host's resident memory, so the rate deep in games shows;
//!   LIMIT: decisions before a game stops as truncated, default 10 000; MODES: 0 both, 1
//!   step-only, 2 with resets, 3 the trained mix, 4 the trained mix overlapped with a model)
//!
//! With resets and LIMIT given, the first games get limits spread evenly over 1..=LIMIT, so the batch reaches
//! the steady state of a training loop (games of every age) after LIMIT calls instead of all
//! games ending and restarting together. Prints one JSON line per mode, with the device memory
//! the batch holds (free memory before it was made less free memory after the first step).
//!
//! A/B windows on the same games: LOOP_BENCH_VARIANTS="K=V K=V;K=V" (environment settings
//! the kernel's host code reads at every call, such as a prototype switch) gives each WINDOW of
//! calls after LOOP_BENCH_WARMUP calls (default 0) the next variant in turn, the first variant
//! before that; every window line names its variant. A key a variant leaves out is set to 0.
//! Calls past the last whole window are counted in the totals but get no window line.
//! LOOP_BENCH_ROWS=1 adds each window's mean legal-list length (read after its times).
//!
//! The trained mix (MODES 3) restarts like the resets mode, but each game's limit is drawn from
//! LOOP_BENCH_CAPS ("CAP:WEIGHT,..."; default TRAINED_CAPS), so the batch's decision ages match
//! training-style self-play rather than random play's longer games. The first games start part
//! way through a drawn cap (the steady state of capped games), so the mix holds from the start
//! except for the games that random play ends early. Its window lines add the playing slots'
//! age quantiles ("ages", decisions since their start; games of the first batch count from call 0).
//!
//! The overlap mode (MODES 4) splits GAMES into two trained-mix halves, A and B, and gives each
//! half a stand-in for the policy's forward pass after its step: `device::load`, products sized
//! so that alone they take LOOP_BENCH_LOAD_US (default 1.2, a small model's forward pass) per
//! decision of a half, calibrated at the start. Per call, with LOOP_BENCH_OVERLAP=0 each half
//! steps, waits, then runs its load; with LOOP_BENCH_OVERLAP=1 half A's step is in flight
//! (`step_launch`) while half B's load runs, then the other way round. Both do the same work, so
//! LOOP_BENCH_VARIANTS="LOOP_BENCH_OVERLAP=0;LOOP_BENCH_OVERLAP=1" times them on the same games;
//! window lines give decisions per second of wall time ("decisions" over "seconds").
//! LOOP_BENCH_BANK=N (N > 0): the resets mode restarts through the background bank instead
//! (`GpuBatch::queue` N games ahead, topped up after each `restart_finished`), as a training
//! loop would; its lines say "with_bank".

use engine_fast::reset::Config;
use engine_fast_batch::Status;
use engine_fast_gpu::batch::GpuBatch;
use engine_fast_gpu::device::{Buffer, Outputs};
use std::time::Instant;

const CATALOG: &str = include_str!("../../../rules/rules-124.json");
const RESET_EVERY: usize = 16;
/// Limits and weights (per mille) whose steady state matches the decision ages of the trained
/// nano policy. Capped games of every age have age density proportional to P(cap > age),
/// so a piecewise-flat age density between those quantiles needs point caps at its steps; the
/// last 1 % is spread to 700.
const TRAINED_CAPS: &[(u32, u64)] = &[(117, 515), (310, 380), (510, 93), (700, 12)];

/// LOOP_BENCH_CAPS or TRAINED_CAPS.
fn caps() -> Vec<(u32, u64)> {
    std::env::var("LOOP_BENCH_CAPS").map_or_else(
        |_| TRAINED_CAPS.to_vec(),
        |text| {
            text.split(',')
                .map(|cw| {
                    let (c, w) = cw.split_once(':').expect("CAP:WEIGHT");
                    let cap: u32 = c.parse().unwrap();
                    let weight: u64 = w.parse().unwrap();
                    assert!(
                        cap > 0 && weight > 0,
                        "LOOP_BENCH_CAPS: caps and weights above 0"
                    );
                    (cap, weight)
                })
                .collect()
        },
    )
}

/// A value fixed by the seed, spread over 0..n.
fn pick(seed: i32, salt: u64, n: u64) -> u64 {
    let x = (seed as u64 ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((x ^ (x >> 29)).wrapping_mul(0xBF58_476D_1CE4_E5B9) >> 32) % n
}

/// A new game's cap: drawn by weight.
fn draw_cap(caps: &[(u32, u64)], seed: i32) -> u32 {
    let mut r = pick(seed, 1, caps.iter().map(|c| c.1).sum());
    for &(c, w) in caps {
        if r < w {
            return c;
        }
        r -= w;
    }
    unreachable!()
}

/// A first-batch game's remaining limit: a slot of the steady state holds a cap with
/// probability proportional to cap times weight, at a uniform point of it.
fn first_limit(caps: &[(u32, u64)], seed: i32) -> u32 {
    let mut r = pick(seed, 2, caps.iter().map(|&(c, w)| u64::from(c) * w).sum());
    for &(c, w) in caps {
        if r < u64::from(c) * w {
            return 1 + pick(seed, 3, u64::from(c)) as u32;
        }
        r -= u64::from(c) * w;
    }
    unreachable!()
}

/// The 50th, 90th and 99th percentile ages of the playing slots.
fn age_quantiles(b: &mut GpuBatch, start: &[usize], now: usize) -> [usize; 3] {
    b.sync().unwrap();
    let mut ages: Vec<usize> = (0..start.len())
        .filter(|&k| b.status(k) == Status::Playing)
        .map(|k| now - start[k])
        .collect();
    ages.sort_unstable();
    let at = |q: usize| ages.get(ages.len() * q / 100).copied().unwrap_or(0);
    [at(50), at(90), at(99)]
}

fn config(size: i32, seed: i32, action_limit: u32) -> Config {
    Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit,
    }
}

/// The process's resident memory (Linux; 0 elsewhere).
fn host_rss() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        })
        .map_or(0, |kb| kb * 1024)
}

/// With LOOP_BENCH_ROWS=1, the mean legal-list length of the playing slots (read after a
/// window's times are taken), so a window's game depth shows; otherwise null.
fn mean_rows(b: &mut GpuBatch, games: usize) -> Option<f64> {
    if std::env::var("LOOP_BENCH_ROWS").map_or(true, |v| v != "1") {
        return None;
    }
    b.sync().unwrap();
    let playing: Vec<usize> = (0..games)
        .filter(|&k| b.status(k) == Status::Playing)
        .collect();
    let rows: usize = playing.iter().map(|&k| b.moves(k).len()).sum();
    Some(rows as f64 / playing.len().max(1) as f64)
}

/// The variants of LOOP_BENCH_VARIANTS, each as the full list of keys set to its values.
fn variants() -> Vec<Vec<(String, String)>> {
    let Ok(text) = std::env::var("LOOP_BENCH_VARIANTS") else {
        return Vec::new();
    };
    let parsed: Vec<Vec<(String, String)>> = text
        .split(';')
        .map(|v| {
            v.split_whitespace()
                .map(|kv| {
                    let (k, x) = kv.split_once('=').expect("K=V");
                    (k.to_owned(), x.to_owned())
                })
                .collect()
        })
        .collect();
    let mut keys: Vec<String> = parsed.iter().flatten().map(|(k, _)| k.clone()).collect();
    keys.sort();
    keys.dedup();
    parsed
        .iter()
        .map(|v| {
            keys.iter()
                .map(|k| {
                    let x = v
                        .iter()
                        .find(|(q, _)| q == k)
                        .map_or("0", |(_, x)| x.as_str());
                    (k.clone(), x.to_owned())
                })
                .collect()
        })
        .collect()
}

/// Which variant of LOOP_BENCH_VARIANTS runs at `call`: the first before the warm-up ends, then
/// one per window in turn.
fn variant_index(variants: usize, window: usize, warmup: usize, call: usize) -> usize {
    if variants == 0 || window == 0 || call < warmup {
        0
    } else {
        ((call - warmup) / window) % variants
    }
}

/// One half of the overlap mode: a trained-mix batch, its actions and its bookkeeping.
struct Half {
    b: GpuBatch,
    actions: Buffer,
    not_playing: u32,
    seed: i32,
}

/// MODES 4 (see the module comment).
fn overlap(size: usize, games: usize, calls: usize, workers: usize, window: usize) {
    let caps = caps();
    let half = games / 2;
    let schedule = variants();
    let warmup: usize = std::env::var("LOOP_BENCH_WARMUP").map_or(0, |w| w.parse().unwrap());
    let target_us: f64 = std::env::var("LOOP_BENCH_LOAD_US").map_or(1.2, |v| v.parse().unwrap());
    let mut halves: Vec<Half> = (0..2)
        .map(|h| {
            let mut b =
                GpuBatch::with_outputs(CATALOG, half, workers, size, Outputs::default()).unwrap();
            let mut seed = 1 + (h * half) as i32;
            let starts: Vec<(usize, Config)> = (0..half)
                .map(|k| {
                    seed += 1;
                    (k, config(size as i32, seed, first_limit(&caps, seed)))
                })
                .collect();
            b.reset(&starts).unwrap();
            Half {
                b,
                actions: Buffer::new(4 * half as u64).unwrap(),
                not_playing: 0,
                seed,
            }
        })
        .collect();
    // the load's repeats: one product timed alone, scaled to the target per decision
    engine_fast_gpu::device::load(half, 1).unwrap();
    engine_fast_gpu::device::load_wait().unwrap();
    let t = Instant::now();
    engine_fast_gpu::device::load(half, 20).unwrap();
    engine_fast_gpu::device::load_wait().unwrap();
    let product_s = t.elapsed().as_secs_f64() / 20.0;
    let repeats = ((target_us * 1e-6 * half as f64 / product_s).round() as usize).max(1);
    let t = Instant::now();
    engine_fast_gpu::device::load(half, repeats).unwrap();
    engine_fast_gpu::device::load_wait().unwrap();
    println!(
        "{}",
        serde_json::json!({"load": {"rows": half, "repeats": repeats,
            "us_per_decision_alone": t.elapsed().as_secs_f64() * 1e6 / half as f64, "target_us": target_us}})
    );
    let load = || engine_fast_gpu::device::load(half, repeats).unwrap();
    let load_wait = || engine_fast_gpu::device::load_wait().unwrap();
    let (mut decisions, mut last_decisions) = (0u64, 0u64);
    let mut last = Instant::now();
    for call in 0..calls {
        let variant = variant_index(schedule.len(), window, warmup, call);
        if !schedule.is_empty()
            && (call == 0
                || (window > 0 && call >= warmup && (call - warmup).is_multiple_of(window)))
        {
            for (key, value) in &schedule[variant] {
                // between calls, while no other thread reads the environment
                std::env::set_var(key, value);
            }
        }
        let overlapped = std::env::var("LOOP_BENCH_OVERLAP").is_ok_and(|v| v == "1");
        for h in &mut halves {
            decisions += (half as u32 - h.not_playing) as u64;
        }
        let seed = 2 * call as u64 + 1;
        if overlapped {
            // A's step in flight while B's load runs, then B's step beside A's load
            let [a, b] = &mut halves[..] else {
                unreachable!()
            };
            a.b.random_actions(seed, &a.actions).unwrap();
            a.b.step_launch(a.actions.ptr, a.actions.bytes).unwrap();
            load();
            a.not_playing = a.b.step_wait().unwrap()[0];
            load_wait();
            b.b.random_actions(seed + 1, &b.actions).unwrap();
            b.b.step_launch(b.actions.ptr, b.actions.bytes).unwrap();
            load();
            b.not_playing = b.b.step_wait().unwrap()[0];
            load_wait();
        } else {
            for (k, h) in halves.iter_mut().enumerate() {
                h.b.random_actions(seed + k as u64, &h.actions).unwrap();
                h.b.step_launch(h.actions.ptr, h.actions.bytes).unwrap();
                h.not_playing = h.b.step_wait().unwrap()[0];
                load();
                load_wait();
            }
        }
        if call % RESET_EVERY == RESET_EVERY - 1 {
            for h in &mut halves {
                if h.not_playing == 0 {
                    continue;
                }
                h.b.sync().unwrap();
                let done: Vec<(usize, Config)> = (0..half)
                    .filter(|&k| h.b.status(k) != Status::Playing)
                    .map(|k| {
                        h.seed += 1;
                        (k, config(size as i32, h.seed, draw_cap(&caps, h.seed)))
                    })
                    .collect();
                h.b.reset(&done).unwrap();
                h.not_playing = (0..half)
                    .filter(|&k| h.b.status(k) != Status::Playing)
                    .count() as u32;
            }
        }
        if window > 0 && call % window == window - 1 {
            println!(
                "{}",
                serde_json::json!({"window": [call + 1 - window, call + 1], "mode": "overlap",
                    "size": size, "games": games, "decisions": decisions - last_decisions,
                    "seconds": last.elapsed().as_secs_f64(), "variant": variant, "overlapped": overlapped})
            );
            last_decisions = decisions;
            last = Instant::now();
        }
    }
}

fn main() {
    let a: Vec<usize> = std::env::args()
        .skip(1)
        .map(|x| x.parse().unwrap())
        .collect();
    let [size, games, calls, workers, outputs, ref rest @ ..] = a[..] else {
        eprintln!("usage: loop_bench SIZE GAMES CALLS WORKERS OUTPUTS [WINDOW [LIMIT [MODES]]]");
        std::process::exit(2);
    };
    if rest.len() > 3 {
        eprintln!("usage: loop_bench SIZE GAMES CALLS WORKERS OUTPUTS [WINDOW [LIMIT [MODES]]]");
        std::process::exit(2);
    }
    let window = rest.first().copied().unwrap_or(0);
    // without LIMIT every game starts with the same limit, as before the argument existed
    let stagger = rest.len() >= 2;
    let limit = rest.get(1).map_or(10_000, |&l| l as u32);
    let modes = rest.get(2).copied().unwrap_or(0);
    if modes == 4 {
        overlap(size, games, calls, workers, window);
        return;
    }
    let gpu_name = engine_fast_gpu::floats::device_name().unwrap_or_default();
    let schedule = variants();
    let warmup: usize = std::env::var("LOOP_BENCH_WARMUP").map_or(0, |w| w.parse().unwrap());
    let bank: usize = std::env::var("LOOP_BENCH_BANK").map_or(0, |n| n.parse().unwrap());
    let apply = |k: usize| {
        for (key, value) in &schedule[k] {
            // between calls, while no other thread reads the environment
            std::env::set_var(key, value);
        }
    };
    let variant_at = |call: usize| variant_index(schedule.len(), window, warmup, call);
    // (resets, trained mix)
    let chosen: &[(bool, bool)] = match modes {
        1 => &[(false, false)],
        2 => &[(true, false)],
        3 => &[(true, true)],
        _ => &[(false, false), (true, false)],
    };
    let caps = caps();
    for &(resets, trained) in chosen {
        if trained && bank > 0 {
            eprintln!("the trained mix restarts without the bank");
            std::process::exit(2);
        }
        let limit_for = |seed: i32| {
            if trained {
                draw_cap(&caps, seed)
            } else {
                limit
            }
        };
        let wanted = Outputs {
            previews: outputs == 1,
            defence: outputs == 1,
            other_view: false,
        };
        let free_before = engine_fast_gpu::device::memory().unwrap().0;
        let mut b = GpuBatch::with_outputs(CATALOG, games, workers, size, wanted).unwrap();
        let mut seed = 1;
        let starts: Vec<(usize, Config)> = (0..games)
            .map(|k| {
                seed += 1;
                let first = if trained {
                    first_limit(&caps, seed)
                } else if resets && stagger {
                    1 + (k as u64 * u64::from(limit) / games as u64) as u32
                } else {
                    limit
                };
                (k, config(size as i32, seed, first))
            })
            .collect();
        b.reset(&starts).unwrap();
        let top_up = |b: &mut GpuBatch, seed: &mut i32| {
            let more: Vec<Config> = (b.queued()..bank)
                .map(|_| {
                    *seed += 1;
                    config(size as i32, *seed, limit)
                })
                .collect();
            b.queue(more).unwrap();
        };
        if resets && bank > 0 {
            top_up(&mut b, &mut seed);
        }
        let mode = match (resets, trained, bank > 0) {
            (false, _, _) => "step_only",
            (true, true, _) => "trained_mix",
            (true, false, false) => "with_resets",
            (true, false, true) => "with_bank",
        };
        // the call before each slot's game took its first decision
        let mut start = vec![0usize; games];
        let actions = Buffer::new(4 * games as u64).unwrap();
        let (mut decisions, mut restarted) = (0u64, 0u64);
        let (mut step_s, mut policy_s, mut reset_s) = (0.0, 0.0, 0.0);
        let mut not_playing = 0u32;
        let traffic_before = b.traffic();
        let at = Instant::now();
        let mut batch_bytes = 0;
        let mut last = (0u64, 0u64, 0.0, 0.0, 0.0, Instant::now());
        for call in 0..calls {
            if !schedule.is_empty()
                && (call == 0
                    || (window > 0 && call >= warmup && (call - warmup).is_multiple_of(window)))
            {
                apply(variant_at(call));
            }
            let t = Instant::now();
            b.random_actions(call as u64 + 1, &actions).unwrap();
            policy_s += t.elapsed().as_secs_f64();
            let t = Instant::now();
            let playing = games as u64 - u64::from(not_playing);
            let word = b.step_device(actions.ptr, actions.bytes).unwrap();
            step_s += t.elapsed().as_secs_f64();
            decisions += playing;
            not_playing = word[0];
            if call == 0 {
                batch_bytes =
                    free_before.saturating_sub(engine_fast_gpu::device::memory().unwrap().0);
            }
            if resets && bank > 0 && call % RESET_EVERY == RESET_EVERY - 1 && not_playing > 0 {
                let t = Instant::now();
                let got = b.restart_finished(true).unwrap();
                restarted += got.len() as u64;
                // a refused start counts as playing until the next status word
                not_playing -= got.len() as u32;
                top_up(&mut b, &mut seed);
                reset_s += t.elapsed().as_secs_f64();
            } else if resets && call % RESET_EVERY == RESET_EVERY - 1 && not_playing > 0 {
                let t = Instant::now();
                b.sync().unwrap();
                let done: Vec<(usize, Config)> = (0..games)
                    .filter(|&k| b.status(k) != Status::Playing)
                    .map(|k| {
                        seed += 1;
                        start[k] = call + 1;
                        (k, config(size as i32, seed, limit_for(seed)))
                    })
                    .collect();
                restarted += done.len() as u64;
                b.reset(&done).unwrap();
                not_playing = (0..games)
                    .filter(|&k| b.status(k) != Status::Playing)
                    .count() as u32;
                reset_s += t.elapsed().as_secs_f64();
            }
            if window > 0 && call % window == window - 1 {
                let (d, r, st, po, re, wall) = last;
                println!(
                    "{}",
                    serde_json::json!({"window": [call + 1 - window, call + 1], "mode": mode,
                        "size": size, "games": games, "decisions": decisions - d, "restarted": restarted - r,
                        "seconds": wall.elapsed().as_secs_f64(), "step_seconds": step_s - st,
                        "policy_seconds": policy_s - po, "reset_seconds": reset_s - re, "not_playing": not_playing,
                        "host_rss_bytes": host_rss(), "variant": variant_at(call), "mean_rows": mean_rows(&mut b, games),
                        "ages": trained.then(|| age_quantiles(&mut b, &start, call + 1))})
                );
                last = (
                    decisions,
                    restarted,
                    step_s,
                    policy_s,
                    reset_s,
                    Instant::now(),
                );
            }
        }
        let secs = at.elapsed().as_secs_f64();
        let traffic = b.traffic();
        println!(
            "{}",
            serde_json::json!({"mode": mode,
                "size": size, "games": games, "outputs": outputs == 1, "calls": calls, "workers": workers, "gpu": gpu_name,
                "decisions": decisions, "restarted": restarted, "seconds": secs,
                "decisions_per_second": decisions as f64 / secs,
                "step_decisions_per_second": decisions as f64 / step_s,
                "step_seconds": step_s, "policy_seconds": policy_s, "reset_seconds": reset_s,
                "bytes_to_device": traffic.0 - traffic_before.0, "bytes_to_host": traffic.1 - traffic_before.1,
                "action_limit": if trained { serde_json::json!(caps) } else { serde_json::json!(limit) }, "batch_device_bytes": batch_bytes,
                "device_total_bytes": engine_fast_gpu::device::memory().unwrap().1,
                "includes": "on-device random policy, step, legal list and view per slot, status word; with_resets adds CPU restarts and uploads of finished slots"})
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The steady-state age quantile q of capped games: age density P(cap > age) / mean cap.
    fn age_at(caps: &[(u32, u64)], q: f64) -> u32 {
        let total: u64 = caps.iter().map(|c| c.1).sum();
        let mean: f64 = caps
            .iter()
            .map(|&(c, w)| f64::from(c) * w as f64)
            .sum::<f64>()
            / total as f64;
        let mut seen = 0.0;
        for age in 0.. {
            let above: u64 = caps.iter().filter(|c| c.0 > age).map(|c| c.1).sum();
            seen += above as f64 / total as f64 / mean;
            if seen >= q {
                return age;
            }
        }
        unreachable!()
    }

    #[test]
    fn the_trained_caps_hold_the_trained_ages() {
        for (q, want) in [(0.5, 117), (0.9, 310), (0.99, 510)] {
            let got = age_at(TRAINED_CAPS, q);
            assert!(got.abs_diff(want) <= 3, "p{q}: {got} against {want}");
        }
    }

    #[test]
    fn draws_follow_the_weights() {
        let n = 100_000;
        let short = (0..n).filter(|&s| draw_cap(TRAINED_CAPS, s) == 117).count();
        assert!((short as f64 / n as f64 - 0.515).abs() < 0.01);
        let firsts: Vec<u32> = (0..n).map(|s| first_limit(TRAINED_CAPS, s)).collect();
        assert!(firsts.iter().all(|&l| (1..=700).contains(&l)));
    }
}
