//! The action table's coverage over engine-fast games: at
//! every decision the checks D1 to D4 (`table::check`), and counts for the report: decisions
//! per board side and seat count, per channel how often a slot was legal and how many
//! distinct slots were hit, every channel never hit (of those the rule data can use), and
//! how far moves and attacks reached.
//!
//!     table_check GAMES DECISIONS OUT.json [WORKERS]
//!
//! Plays GAMES games for each board side (8, 11, 14, 16) and seat count (2 to 4), with every
//! tribe in turn (the special tribes included), DECISIONS decisions at most each, by seeded
//! random play: no Resign except one time in 2 000, EndTurn one time in eight. Exits 1 on
//! the first failed check, with its game and decision.

// the batch is read by slot number, beside the per-slot choice arrays
#![allow(clippy::needless_range_loop)]

use engine_fast::reset::Config;
use engine_fast::tables::Tables;
use engine_fast_batch::outputs::table_frame;
use engine_fast_batch::table::{self, Channel};
use engine_fast_batch::{Applied, Batch, Status, SKIP};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
const RESIGN: i32 = 35;
const END_TURN: i32 = 15;
const MOVE: i32 = 6;
const ATTACK: i32 = 2;
/// tribe ids 2 to 17
const TRIBES: [i32; 16] = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];

fn choose(seed: u64, decision: u64, moves: &[[i32; 6]]) -> usize {
    let mut x =
        seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ decision.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    x = (x ^ (x >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    if x.is_multiple_of(2_000) {
        if let Some(r) = moves.iter().position(|m| m[0] == RESIGN) {
            return r;
        }
    }
    let end = moves.iter().position(|m| m[0] == END_TURN);
    let kept: Vec<usize> = (0..moves.len())
        .filter(|&i| moves[i][0] != RESIGN && moves[i][0] != END_TURN)
        .collect();
    match end {
        Some(e) if kept.is_empty() || (x >> 40).is_multiple_of(8) => e,
        _ => kept[((x >> 33) as usize) % kept.len()],
    }
}

/// The channels a pass must hit: every channel but the reserved ones and Boost, and only ids
/// the rule data has.
fn expected(t: &Tables) -> BTreeSet<Channel> {
    let mut out = BTreeSet::new();
    for s in [0, 1, 2].into_iter().chain(14..29) {
        out.insert(Channel::Global(s));
    }
    for id in 0..64 {
        if t.techs.get(id).is_some() {
            out.insert(Channel::Global(32 + id));
        }
        if t.improvements.get(id).is_some() {
            out.insert(Channel::Tile(10 + id));
        }
        if t.units.get(id).is_some() {
            out.insert(Channel::Tile(74 + id));
        }
    }
    for k in (0..17).filter(|&k| k != table::BOOST_CHANNEL) {
        out.insert(Channel::Unit(k));
    }
    for k in (0..49).filter(|&k| k != 24) {
        out.insert(Channel::Attack(k));
    }
    for k in 0..10 {
        out.insert(Channel::Tile(k));
    }
    out.insert(Channel::Move);
    out
}

/// `table::describe`, with the rule data's name of a technology, improvement or unit id.
fn named(t: &Tables, c: Channel) -> String {
    fn of<T>(s: &engine_fast::tables::Section<T>, id: usize) -> String {
        s.by_id
            .get(id)
            .and_then(|e| e.as_ref())
            .map_or(String::new(), |(n, _)| format!(" ({n})"))
    }
    let extra = match c {
        Channel::Global(s) if s >= 32 => of(&t.techs, s - 32),
        Channel::Tile(k) if (10..74).contains(&k) => of(&t.improvements, k - 10),
        Channel::Tile(k) if k >= 74 => of(&t.units, k - 74),
        _ => String::new(),
    };
    table::describe(c) + &extra
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: table_check GAMES DECISIONS OUT.json [WORKERS]");
        std::process::exit(2);
    }
    let games: usize = a[1].parse().expect("GAMES");
    let decisions: u32 = a[2].parse().expect("DECISIONS");
    let workers: usize = a.get(4).map_or(4, |w| w.parse().expect("WORKERS"));
    let t = Tables::compile(CATALOG).unwrap();
    let mut configs = Vec::new();
    for side in [8, 11, 14, 16] {
        for seats in 2..=4usize {
            for k in 0..games {
                let first = (configs.len() + k) % TRIBES.len();
                configs.push(Config {
                    rules: 124,
                    seed: configs.len() as i32 + 1,
                    size: side,
                    map: 1 + (k % 6) as i32,
                    mode: 4,
                    tribes: (0..seats)
                        .map(|s| TRIBES[(first + 5 * s) % TRIBES.len()])
                        .collect(),
                    action_limit: decisions,
                });
            }
        }
    }
    let n = configs.len();
    let mut b = Batch::new(CATALOG, n, workers).unwrap();
    b.reset(&configs.iter().cloned().enumerate().collect::<Vec<_>>())
        .unwrap();
    let mut per_board: BTreeMap<(i32, usize), u64> = BTreeMap::new();
    let mut legal: BTreeMap<Channel, u64> = BTreeMap::new();
    let mut distinct: BTreeMap<Channel, BTreeSet<(i32, usize)>> = BTreeMap::new();
    let mut move_reach: BTreeMap<i32, u64> = BTreeMap::new();
    let mut refused: BTreeMap<String, u64> = BTreeMap::new();
    let (mut rows, mut mask) = (Vec::new(), Vec::new());
    let mut applied = vec![Applied::Skipped; n];
    for decision in 0..u64::from(decisions) {
        let mut choice = vec![SKIP; n];
        for i in 0..n {
            if b.status(i) != Status::Playing {
                continue;
            }
            let g = b.game(i).unwrap();
            let f = table_frame(g).unwrap_or_else(|e| {
                eprintln!("game {i} decision {decision}: {e}");
                std::process::exit(1)
            });
            b.table(i, &mut rows, &mut mask).unwrap_or_else(|e| {
                eprintln!("game {i} decision {decision}: {e}");
                std::process::exit(1)
            });
            let moves = b.moves(i);
            let slots = table::check(moves, &f, &mask).unwrap_or_else(|e| {
                eprintln!("game {i} decision {decision}: {e}");
                std::process::exit(1)
            });
            *per_board.entry((f.side, f.seats as usize)).or_default() += 1;
            let tiles = (f.side * f.side) as usize;
            for (m, &s) in moves.iter().zip(&slots) {
                let c = table::channel(s, tiles);
                *legal.entry(c).or_default() += 1;
                distinct.entry(c).or_default().insert((f.side, s));
                if m[0] == MOVE {
                    let (dx, dy) = (m[2] % f.side - m[1] % f.side, m[2] / f.side - m[1] / f.side);
                    *move_reach.entry(dx.abs().max(dy.abs())).or_default() += 1;
                }
                debug_assert!(m[0] != ATTACK || matches!(c, Channel::Attack(_)));
            }
            choice[i] = choose(i as u64 + 1, decision, moves) as i32;
        }
        if choice.iter().all(|&m| m == SKIP) {
            break;
        }
        b.step(&choice, &mut applied).unwrap();
    }
    for i in 0..n {
        if matches!(b.status(i), Status::Refused | Status::Failed) {
            *refused.entry(b.reason(i).to_owned()).or_default() += 1;
        }
    }
    let never: Vec<String> = expected(&t)
        .into_iter()
        .filter(|c| !legal.contains_key(c))
        .map(|c| named(&t, c))
        .collect();
    let report = json!({
        "games": n,
        "decision_limit": decisions,
        "decisions": per_board.iter().map(|((side, seats), d)| json!({"side": side, "seats": seats, "decisions": d})).collect::<Vec<_>>(),
        "channels": legal.iter().map(|(c, k)| json!({
            "channel": table::describe(*c), "legal": k, "distinct_slots": distinct[c].len(),
        })).collect::<Vec<_>>(),
        "never_hit": never,
        "move_reach": move_reach.iter().map(|(d, k)| json!({"tiles": d, "moves": k})).collect::<Vec<_>>(),
        "attack_offsets_hit": legal.keys().filter(|c| matches!(c, Channel::Attack(_))).count(),
        "stopped_by_the_engine": refused,
        "d6_repeats_with_another_price": engine_fast::menu::repeats_with_another_price(),
        "verdict": "D1 to D4 held at every decision; D6 holds if the count above is 0",
    });
    std::fs::write(&a[3], serde_json::to_string_pretty(&report).unwrap()).unwrap();
    if engine_fast::menu::repeats_with_another_price() != 0 {
        eprintln!("D6: a repeat dropped from a list had another price");
        std::process::exit(1);
    }
    println!(
        "{} games, {} decisions, {} channels hit, {} never hit",
        n,
        per_board.values().sum::<u64>(),
        legal.len(),
        report["never_hit"].as_array().unwrap().len()
    );
}
