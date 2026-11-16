//! Throughput of the batch: decisions per second with every game stepped on each call.
//!
//!   batch_bench GAMES WORKERS CALLS [SIZE [SEATS]]
//!
//! Plays random play (EndTurn one time in ten, never Resign) on the
//! collection configuration (rules 124, Might, Lakes, Imperius), restarting a slot when its
//! game stops, and prints one JSON line. `step_seconds` covers the `step` calls alone (each
//! includes the next view and legal list of every game); `decisions_per_second` is moves
//! taken over that time, and `worker_us_per_decision` that time times the workers per move,
//! which counts a worker's idle time too. Starting games is timed apart (`reset_seconds`
//! for `resets` games): a reset reads the rule data and generates a map.

use engine_fast::reset::Config;
use engine_fast_batch::{Applied, Batch, Status, SKIP};
use std::time::Instant;

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
const END_TURN: i32 = 15;
const RESIGN: i32 = 35;

fn main() -> Result<(), String> {
    let a: Vec<usize> = std::env::args()
        .skip(1)
        .map(|x| {
            x.parse()
                .map_err(|_| "usage: batch_bench GAMES WORKERS CALLS [SIZE [SEATS]]")
        })
        .collect::<Result<_, _>>()?;
    let (&games, &workers, &calls) = match &a[..] {
        [g, w, c, ..] => (g, w, c),
        _ => return Err("usage: batch_bench GAMES WORKERS CALLS [SIZE [SEATS]]".into()),
    };
    let size = a.get(3).copied().unwrap_or(11) as i32;
    let seats = a.get(4).copied().unwrap_or(2);
    let config = |seed: i32| Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes: vec![7; seats],
        action_limit: 1000,
    };
    let mut batch = Batch::new(CATALOG, games, workers)?;
    let mut next_seed = 1;
    let mut starts = Vec::new();
    for slot in 0..games {
        starts.push((slot, config(next_seed)));
        next_seed += 1;
    }
    let at = Instant::now();
    batch.reset(&starts)?;
    let (mut reset_seconds, mut resets) = (at.elapsed().as_secs_f64(), games);
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    let mut moves = vec![SKIP; games];
    let mut applied = vec![Applied::Skipped; games];
    let (mut decisions, mut stopped, mut seconds) = (0u64, [0u64; 6], 0f64);
    for _ in 0..calls {
        starts.clear();
        for slot in 0..games {
            let status = batch.status(slot);
            if status != Status::Playing {
                stopped[status as usize] += 1;
                starts.push((slot, config(next_seed)));
                next_seed += 1;
            }
        }
        let at = Instant::now();
        batch.reset(&starts)?;
        reset_seconds += at.elapsed().as_secs_f64();
        resets += starts.len();
        for (slot, chosen) in moves.iter_mut().enumerate() {
            let legal = batch.moves(slot);
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = (rng >> 33) as usize;
            let end = legal.iter().position(|m| m[0] == END_TURN);
            let others = legal
                .iter()
                .filter(|m| m[0] != END_TURN && m[0] != RESIGN)
                .count();
            *chosen = match end {
                Some(e) if others == 0 || r % 10 == 0 => e as i32,
                _ if others > 0 => {
                    let nth = (r / 10) % others;
                    legal
                        .iter()
                        .enumerate()
                        .filter(|(_, m)| m[0] != END_TURN && m[0] != RESIGN)
                        .nth(nth)
                        .map_or(SKIP, |(i, _)| i as i32)
                }
                _ => SKIP,
            };
        }
        let at = Instant::now();
        batch.step(&moves, &mut applied)?;
        seconds += at.elapsed().as_secs_f64();
        decisions += applied.iter().filter(|&&x| x == Applied::Stepped).count() as u64;
    }
    let per = |n: f64| {
        if decisions == 0 {
            0.0
        } else {
            n / decisions as f64
        }
    };
    println!(
        "{{\"games\":{games},\"workers\":{workers},\"size\":{size},\"seats\":{seats},\"decisions\":{decisions},\"step_seconds\":{seconds:.3},\"decisions_per_second\":{:.0},\"worker_us_per_decision\":{:.1},\"resets\":{resets},\"reset_seconds\":{reset_seconds:.3},\"ended\":{},\"truncated\":{},\"refused\":{},\"failed\":{}}}",
        if seconds > 0.0 { decisions as f64 / seconds } else { 0.0 },
        per(seconds * 1e6 * workers.max(1) as f64),
        stopped[Status::Ended as usize],
        stopped[Status::Truncated as usize],
        stopped[Status::Refused as usize],
        stopped[Status::Failed as usize],
    );
    Ok(())
}
