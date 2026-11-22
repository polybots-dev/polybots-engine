//! `Batch::load` and `Batch::command` on the engine's stored state (a seeded two-seat game on
//! an 11 by 11 board with placeholder account ids): the slot plays for the account to move
//! and for no other, and a legal move comes back as the command `wire::encode` writes.

use engine_fast::wire;
use engine_fast_batch::{Applied, Batch, Loaded, Status, SKIP};

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));

fn state() -> Vec<u8> {
    let hex: String = include_str!("../../tests/data/state.hex")
        .split_whitespace()
        .collect();
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn a_loaded_state_plays_for_the_account_to_move_only() {
    let state = state();
    let decoded = wire::decode(&state).unwrap();
    let game = &decoded.game;
    let mover = game.players[game.current_player_index as usize].action.id;
    let mut batch = Batch::new(CATALOG, 2, 1).unwrap();
    for (seat, id) in [(1, [1u8; 16]), (2, [2u8; 16])] {
        let loaded = batch.load(0, &state, id).unwrap();
        assert_eq!(
            loaded,
            Loaded {
                seat,
                commands: 235,
                width: 11,
            }
        );
        if seat == mover {
            assert_eq!(batch.status(0), Status::Playing);
            assert!(batch.view(0).is_some());
            // the critic's output would be the opponent's view of a live game
            assert!(batch.other_view(0).is_err());
            let moves = batch.moves(0).len();
            assert!(moves > 1);
            for index in 0..moves {
                let command = batch.command(0, index).unwrap();
                // its kind, then the mover
                assert_eq!(i32::from(command[0]), batch.moves(0)[index][0]);
                assert_eq!((command[1], i32::from(command[2])), (0, seat));
            }
            assert!(batch.command(0, moves).is_err());
            // played on to the end of the turn, the slot never shows the other player's turn
            let end_turn = (0..moves).find(|&i| batch.moves(0)[i][0] == 15).unwrap() as i32;
            let mut applied = [Applied::Skipped; 2];
            batch.step(&[end_turn, SKIP], &mut applied).unwrap();
            assert_eq!(applied[0], Applied::Stepped);
            assert_eq!(batch.status(0), Status::Empty);
            assert!(batch.moves(0).is_empty());
        } else {
            // another player's turn: no view and no moves of theirs
            assert_eq!(batch.status(0), Status::Empty);
            assert!(batch.moves(0).is_empty());
            assert!(batch.command(0, 0).is_err());
            assert!(batch.other_view(0).is_err());
        }
    }
    // the other slot was never touched, and an unknown account has no seat
    assert_eq!(batch.status(1), Status::Empty);
    assert!(batch.load(1, &state, [9; 16]).is_err());
    assert!(batch.load(1, &state, [0; 16]).is_err());
    assert!(batch.load(1, &state[..100], [1; 16]).is_err());
    assert_eq!(batch.status(1), Status::Empty);
}
