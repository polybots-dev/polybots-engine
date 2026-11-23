//! The batch plays each game exactly as the single-game path does: at every decision the
//! same legal moves (each with its price), the same view and the same game state, and the
//! same ending with the same winner, at 1, 3 and 8 workers and in two different batches. A
//! refused or rejected slot leaves the others alone.
//!
//! The single-game path here calls the engine directly and states the move layout and the
//! ending rule a second time on purpose, so a mistake in the batch's own copy shows.

use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::view::View;
use engine_fast::{menu, step, view};
use engine_fast_batch::{Applied, Batch, Move, Status, KINDS, SKIP};

const CATALOG: &str = include_str!("../../rules/rules-124.json");
const DECISIONS: u32 = 300;

fn config(seed: i32, size: i32, seats: usize) -> Config {
    Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes: vec![7; seats],
        action_limit: DECISIONS,
    }
}

fn configs() -> Vec<Config> {
    vec![
        config(1, 11, 2),
        config(2, 11, 2),
        config(3, 11, 3),
        config(4, 11, 4),
        config(5, 14, 4),
        config(6, 16, 2),
    ]
}

const RESIGN: i32 = 35;

/// The game (by seed) and decision at which the player to move resigns, so one game ends.
const RESIGNS: (i32, u64) = (2, 60);

/// The move every player makes: seeded by the game and the decision, from the sorted list,
/// and never Resign except in the one game that ends by it.
fn choose(seed: i32, decision: u64, moves: &[Move]) -> usize {
    if (seed, decision) == RESIGNS {
        return moves.iter().position(|m| m[0] == RESIGN).unwrap();
    }
    let mut x = (seed as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ decision;
    x = (x ^ (x >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let kept: Vec<usize> = (0..moves.len())
        .filter(|&i| moves[i][0] != RESIGN)
        .collect();
    kept[((x >> 33) as usize) % kept.len()]
}

/// One game on the single-game path: per decision, the sorted legal moves, the view and the
/// state; and how it ended.
struct Record {
    decisions: Vec<(Vec<Move>, View, Game)>,
    status: Status,
    winner: i32,
}

fn single(t: &Tables, c: &Config) -> Record {
    let mut g = reset::reset(c, CATALOG, t).unwrap();
    let mut decisions = Vec::new();
    loop {
        if let Some(winner) = engine_fast::queries::winner(&g).unwrap() {
            return Record {
                decisions,
                status: Status::Ended,
                winner,
            };
        }
        if g.decisions == u64::from(c.action_limit) {
            return Record {
                decisions,
                status: Status::Truncated,
                winner: 0,
            };
        }
        let mover = g.players[g.current_player_index as usize].action.id;
        let mut seen = View::default();
        view::write(&g, t, mover, &mut seen).unwrap();
        let width = g.width;
        let tile = |at: Option<engine_fast_frame::Xy>| at.map_or(-1, |c| c.y * width + c.x);
        let mut listed: Vec<(Move, engine_fast_frame::Action)> = menu::menu(&g, t)
            .unwrap()
            .into_iter()
            .map(|c| {
                let a = &c.action;
                let m = [
                    a.kind,
                    tile(a.source),
                    tile(a.target),
                    a.option.or(a.accept.map(i32::from)).unwrap_or(-1),
                    a.opponent.unwrap_or(-1),
                    c.details.price.unwrap_or(-1),
                ];
                (m, c.action)
            })
            .collect();
        listed.sort_by_key(|(m, _)| *m);
        // the first five fields identify a move
        assert!(listed.windows(2).all(|w| w[0].0[..5] != w[1].0[..5]));
        let legal: Vec<Move> = listed.iter().map(|(m, _)| *m).collect();
        let pick = choose(c.seed, g.decisions, &legal);
        assert!(legal.iter().all(|m| KINDS.iter().any(|(_, k)| *k == m[0])));
        decisions.push((legal, seen, g.clone()));
        step::advance(&mut g, t, &listed[pick].1).unwrap();
    }
}

/// Play `configs` in one batch and compare every decision with `records` (same order).
fn play(workers: usize, configs: &[Config], records: &[&Record]) {
    let mut batch = Batch::new(CATALOG, configs.len() + 1, workers).unwrap();
    // the last slot stays empty
    let starts: Vec<(usize, Config)> = configs.iter().cloned().enumerate().collect();
    batch.reset(&starts).unwrap();
    let empty = configs.len();
    assert_eq!(batch.status(empty), Status::Empty);
    let mut at = vec![0usize; configs.len()];
    let mut applied = vec![Applied::Skipped; batch.len()];
    loop {
        let mut moves = vec![SKIP; batch.len()];
        for (i, c) in configs.iter().enumerate() {
            let record = &records[i];
            if at[i] == record.decisions.len() {
                assert_eq!(batch.status(i), record.status, "game {i}");
                assert_eq!(batch.winner(i), record.winner, "game {i}");
                continue;
            }
            assert_eq!(
                batch.status(i),
                Status::Playing,
                "game {i}: {}",
                batch.reason(i)
            );
            let (legal, seen, state) = &record.decisions[at[i]];
            assert_eq!(batch.game(i), Some(state), "game {i} decision {}", at[i]);
            assert_eq!(
                batch.moves(i),
                legal.as_slice(),
                "game {i} decision {}",
                at[i]
            );
            assert_eq!(batch.view(i), Some(seen), "game {i} decision {}", at[i]);
            moves[i] = choose(c.seed, at[i] as u64, legal) as i32;
        }
        if moves.iter().all(|&m| m == SKIP) {
            break;
        }
        // a move for the empty slot is rejected and disturbs nothing
        moves[empty] = 0;
        batch.step(&moves, &mut applied).unwrap();
        assert_eq!(applied[empty], Applied::Rejected);
        for i in 0..configs.len() {
            if moves[i] == SKIP {
                assert_eq!(applied[i], Applied::Skipped);
            } else {
                assert_eq!(applied[i], Applied::Stepped);
                at[i] += 1;
            }
        }
    }
}

#[test]
fn batch_matches_single_games_at_any_worker_count() {
    let t = Tables::compile(CATALOG).unwrap();
    let configs = configs();
    let records: Vec<Record> = configs.iter().map(|c| single(&t, c)).collect();
    // one game ends by resignation, with a winner; the others reach the decision limit
    let ended: Vec<&Record> = records
        .iter()
        .filter(|r| r.status == Status::Ended)
        .collect();
    assert!(ended.len() == 1 && ended[0].winner != 0 && ended[0].decisions.len() == 61);
    assert!(records
        .iter()
        .all(|r| r.status == Status::Ended || r.decisions.len() == DECISIONS as usize));
    for workers in [1, 3, 8] {
        play(workers, &configs, &records.iter().collect::<Vec<_>>());
    }
    // another batch: fewer games, in another order
    let order = [4, 1, 3];
    let fewer: Vec<Config> = order.iter().map(|&i| configs[i].clone()).collect();
    let theirs: Vec<&Record> = order.iter().map(|&i| &records[i]).collect();
    play(3, &fewer, &theirs);
}

#[test]
fn a_refused_game_and_bad_indices_leave_the_rest_alone() {
    let mut batch = Batch::new(CATALOG, 3, 2).unwrap();
    let mut outside = config(9, 11, 2);
    outside.rules = 122;
    batch
        .reset(&[(0, config(7, 11, 2)), (1, outside), (2, config(7, 11, 2))])
        .unwrap();
    assert_eq!(batch.status(1), Status::Refused);
    assert!(batch.reason(1).contains("rules"));
    assert!(batch.moves(1).is_empty() && batch.view(1).is_none());
    let before = batch.game(0).cloned();
    let mut applied = vec![Applied::Skipped; 3];
    let past = batch.moves(0).len() as i32;
    batch.step(&[past, 0, 0], &mut applied).unwrap();
    assert_eq!(
        applied,
        [Applied::Rejected, Applied::Rejected, Applied::Stepped]
    );
    assert_eq!(batch.game(0).cloned(), before);
    assert_eq!(batch.status(2), Status::Playing);
    // the moves on offer are unchanged by the rejection, and an index below SKIP is rejected
    let offered = batch.moves(0).to_vec();
    batch.step(&[-2, SKIP, SKIP], &mut applied).unwrap();
    assert_eq!(
        applied,
        [Applied::Rejected, Applied::Skipped, Applied::Skipped]
    );
    assert_eq!(batch.moves(0), offered.as_slice());
    assert!(batch.reset(&[(3, config(1, 11, 2))]).is_err());
    assert!(batch
        .reset(&[(0, config(1, 11, 2)), (0, config(2, 11, 2))])
        .is_err());
    // an ended slot that is then refused at reset no longer names a winner
    let resign = batch.moves(2).iter().position(|m| m[0] == RESIGN).unwrap();
    batch
        .step(&[SKIP, SKIP, resign as i32], &mut applied)
        .unwrap();
    assert!(batch.status(2) == Status::Ended && batch.winner(2) != 0);
    let mut outside = config(9, 11, 2);
    outside.rules = 122;
    batch.reset(&[(2, outside)]).unwrap();
    assert_eq!((batch.status(2), batch.winner(2)), (Status::Refused, 0));
    assert!(batch.step(&[0, 0], &mut applied).is_err());
}
