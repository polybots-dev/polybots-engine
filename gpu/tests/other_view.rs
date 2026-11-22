//! The host build's other-seat view (`kernel/outputs.h` ef_other_view, the privileged critic's
//! input) equals the CPU batch's `other_view` at every decision of engine-fast games at nano
//! (8 x 8) with two to four seats and both rule versions, so the device writes what the CPU
//! batch hands out. The seat rule against the game's own turn order is the batch crate's test;
//! the device's arrays are `device_loop`'s (cuda feature).

use engine_fast::reset::Config;
use engine_fast::tables::Tables;
use engine_fast_batch::{Applied, Batch, Status, SKIP};
use engine_fast_gpu::{outputs, state as device, tables};

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
const RESIGN: i32 = 35;
const END_TURN: i32 = 15;

#[test]
fn the_host_other_view_equals_the_batchs() {
    let flat = tables::DeviceTables::new(&Tables::compile(CATALOG).unwrap()).unwrap();
    let mut configs = Vec::new();
    for (seed, seats) in [(31, 2), (32, 3), (33, 4)] {
        for rules in [123, 124] {
            configs.push(Config {
                rules,
                seed,
                size: 8,
                map: 2,
                mode: 2,
                tribes: vec![7; seats],
                action_limit: 800,
            });
        }
    }
    let mut batch = Batch::new(CATALOG, configs.len(), 3).unwrap();
    batch
        .reset(&configs.iter().cloned().enumerate().collect::<Vec<_>>())
        .unwrap();
    let mut applied = vec![Applied::Skipped; batch.len()];
    let mut rng = 0x2545_F491_4F6C_DD1Du64;
    let (mut compared, mut seats) = (0u64, std::collections::BTreeSet::new());
    for decision in 0..800 {
        let mut moves = vec![SKIP; batch.len()];
        for k in 0..batch.len() {
            if batch.status(k) != Status::Playing {
                continue;
            }
            let block = device::to_device(batch.game(k).unwrap()).unwrap();
            let got = outputs::twin_other_view(&block, &flat).unwrap();
            assert_eq!(
                got,
                batch.other_view(k).unwrap(),
                "slot {k}, decision {decision}"
            );
            seats.insert((configs[k].tribes.len(), got.map(|v| v.viewer)));
            compared += 1;
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = (rng >> 33) as usize;
            let rows = batch.moves(k);
            let end = rows.iter().position(|m| m[0] == END_TURN);
            let kept: Vec<usize> = (0..rows.len())
                .filter(|&i| rows[i][0] != RESIGN && rows[i][0] != END_TURN)
                .collect();
            moves[k] = match end {
                Some(e) if kept.is_empty() || r.is_multiple_of(6) => e,
                _ => kept[(r / 6) % kept.len()],
            } as i32;
        }
        if moves.iter().all(|&m| m == SKIP) {
            break;
        }
        batch.step(&moves, &mut applied).unwrap();
    }
    println!("{compared} other-seat views equal; (seats, other seat) seen: {seats:?}");
    // every seat of the 3- and 4-seat games was the other seat at some point
    assert!(compared > 2_000, "{compared}");
    for (n, ids) in [(2, 1..=2), (3, 1..=3), (4, 1..=4)] {
        for id in ids {
            assert!(
                seats.contains(&(n, Some(id as i16))),
                "{n} seats: seat {id} never seen"
            );
        }
    }
}
