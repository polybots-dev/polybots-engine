//! `wire::decode` on a stored state: a seeded two-seat game on an 11 by 11 board, written by
//! this project's tools, with placeholder account ids. That decoded states are right is
//! checked elsewhere; this pins what the decoder reads from one state and what it refuses.

use engine_fast::wire::{account, decode};

fn state() -> Vec<u8> {
    let hex: String = include_str!("data/state.hex").split_whitespace().collect();
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn reads_the_stored_state() {
    let decoded = decode(&state()).unwrap();
    let game = &decoded.game;
    assert_eq!((game.width, game.tiles.len()), (11, 121));
    assert_eq!(game.current_turn, 24);
    // two seats and Nature, with the placeholder account ids
    assert_eq!(
        decoded.accounts,
        [(1, [1; 16]), (2, [2; 16]), (255, [0; 16])]
    );
    // the state holds its whole command log, the opening StartMatch first
    assert_eq!(decoded.commands.len(), 235);
    assert_eq!(game.current_command, 235);
    assert_eq!(decoded.commands[0], [20, 0, 1]);
    assert!(decoded.commands.iter().all(|c| c.len() >= 3));
}

#[test]
fn refuses_what_is_not_a_whole_state() {
    let state = state();
    for cut in [0, 3, 40, state.len() / 2, state.len() - 1] {
        assert!(decode(&state[..cut]).is_err(), "cut at {cut}");
    }
    let mut longer = state.clone();
    longer.push(0);
    assert!(decode(&longer).err().unwrap().contains("after the state"));
    let mut other = state.clone();
    other[0] = 99;
    assert!(decode(&other).err().unwrap().contains("rules version"));
    // the header's current player (byte 10) must be one of the three players
    let mut nobody = state;
    nobody[10] = 3;
    assert!(decode(&nobody).err().unwrap().contains("current player"));
}

#[test]
fn reads_an_account_id_in_the_games_byte_order() {
    let id = account(b"00112233-4455-6677-8899-aabbccddeeff").unwrap();
    assert_eq!(
        id,
        [
            0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff
        ]
    );
    assert_eq!(account(b"00112233445566778899aabbccddeeff").unwrap(), id);
    assert!(account(b"not an id").is_err());
    assert!(account(b"+0112233445566778899aabbccddeeff").is_err());
}
