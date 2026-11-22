//! The action table against the legal list at every decision
//! of engine-fast games: every row encodes and decodes to itself, no two rows share a slot,
//! the batch's mask holds exactly the rows (D1 to D4), and a batch stepped by table slot plays
//! exactly as one stepped by row index (D5): the same states and lists at every decision.
//!
//! Random play over boards 8, 11 and 14, two to four seats and the special tribes. It shows
//! the table holds what these games reach; channels they never reach are counted by
//! `src/bin/table_check.rs`, and the recorded games are checked there too.

// the batch is read by slot number, beside the per-slot choice arrays
#![allow(clippy::needless_range_loop)]

use engine_fast::reset::Config;
use engine_fast_batch::outputs::table_frame;
use engine_fast_batch::table;
use engine_fast_batch::{Applied, Batch, Status, SKIP};

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
const RESIGN: i32 = 35;
const END_TURN: i32 = 15;

fn configs() -> Vec<Config> {
    // Imperius (2), Aquarion (3), Elyrion (5), Kickoo (7), Polaris (16), Cymanti (17)
    [
        (1, 8, vec![7, 2]),
        (2, 11, vec![3, 2]),
        (3, 11, vec![16, 17, 5]),
        (4, 14, vec![7, 2, 3, 16]),
        (5, 11, vec![17, 5, 2, 7]),
    ]
    .into_iter()
    .map(|(seed, size, tribes)| Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes,
        action_limit: 500,
    })
    .collect()
}

/// A random legal row, never Resign, EndTurn about one time in eight.
fn choose(seed: u64, decision: u64, moves: &[[i32; 6]]) -> usize {
    let mut x = (seed + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ decision;
    x = (x ^ (x >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let end = moves.iter().position(|m| m[0] == END_TURN);
    let kept: Vec<usize> = (0..moves.len())
        .filter(|&i| moves[i][0] != RESIGN && moves[i][0] != END_TURN)
        .collect();
    match end {
        Some(e) if kept.is_empty() || (x >> 40).is_multiple_of(8) => e,
        _ => kept[((x >> 33) as usize) % kept.len()],
    }
}

#[test]
fn the_table_holds_the_list_and_stepping_by_slot_equals_stepping_by_index() {
    let configs = configs();
    let starts: Vec<(usize, Config)> = configs.iter().cloned().enumerate().collect();
    let (mut by_index, mut by_slot) = (
        Batch::new(CATALOG, configs.len(), 2).unwrap(),
        Batch::new(CATALOG, configs.len(), 2).unwrap(),
    );
    by_index.reset(&starts).unwrap();
    by_slot.reset(&starts).unwrap();
    let n = configs.len();
    let (mut applied, mut applied_slot) = (vec![Applied::Skipped; n], vec![Applied::Skipped; n]);
    let (mut rows, mut mask) = (Vec::new(), Vec::new());
    let mut decisions = 0;
    for decision in 0..500u64 {
        let (mut index, mut slot) = (vec![SKIP; n], vec![i64::from(SKIP); n]);
        for i in 0..n {
            assert_eq!(by_index.status(i), by_slot.status(i), "game {i}");
            if by_index.status(i) != Status::Playing {
                continue;
            }
            assert_eq!(
                by_index.game(i),
                by_slot.game(i),
                "game {i} decision {decision}"
            );
            assert_eq!(
                by_index.moves(i),
                by_slot.moves(i),
                "game {i} decision {decision}"
            );
            let g = by_index.game(i).unwrap();
            assert!(by_index.table(i, &mut rows, &mut mask).unwrap());
            let moves = by_index.moves(i);
            let slots = table::check(moves, &table_frame(g).unwrap(), &mask)
                .unwrap_or_else(|e| panic!("game {i} decision {decision}: {e}"));
            assert_eq!(rows, slots.iter().map(|&s| s as u32).collect::<Vec<_>>());
            let k = choose(i as u64, decision, moves);
            index[i] = k as i32;
            slot[i] = slots[k] as i64;
            decisions += 1;
        }
        if index.iter().all(|&m| m == SKIP) {
            break;
        }
        by_index.step(&index, &mut applied).unwrap();
        by_slot.step_table(&slot, &mut applied_slot).unwrap();
        assert_eq!(applied, applied_slot, "decision {decision}");
    }
    assert!(decisions > 1_000, "{decisions} decisions");
}

/// A slot that holds no legal row is rejected and changes nothing, as an index out of range.
#[test]
fn a_slot_with_no_legal_row_is_rejected() {
    let mut b = Batch::new(CATALOG, 1, 1).unwrap();
    b.reset(&[(0, configs().remove(1))]).unwrap();
    let before = b.game(0).cloned();
    let (mut rows, mut mask) = (Vec::new(), Vec::new());
    b.table(0, &mut rows, &mut mask).unwrap();
    let f = table_frame(b.game(0).unwrap()).unwrap();
    let free = (0..table::slots((f.side * f.side) as usize))
        .find(|&s| mask[s / 32] >> (s % 32) & 1 == 0 && table::decode(s, &f).is_some())
        .unwrap();
    let mut applied = vec![Applied::Skipped];
    for s in [free as i64, -5, i64::MAX] {
        b.step_table(&[s], &mut applied).unwrap();
        assert_eq!(applied[0], Applied::Rejected, "slot {s}");
        assert_eq!(b.game(0).cloned(), before);
    }
}
