//! The batch's `other_view` output at every decision of
//! engine-fast games at nano (8 x 8), 2 to 4 seats: it is the view function applied to the
//! other seat, and the other seat is the player the turn passes to next, checked against what
//! the game does at each EndTurn rather than against the rule's own code. A constructed game
//! checks that dead players and Nature are skipped, and the seat when every other is dead. That the output reaches no actor output is
//! the lockstep crate's non-interference test and the Python package's byte-equality test.

use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::turns::other_seat;
use engine_fast::view::{self, View};
use engine_fast_batch::{Applied, Batch, Status, SKIP};

const CATALOG: &str = include_str!("../../rules/rules-124.json");
const DECISIONS: u32 = 600;
const RESIGN: i32 = 35;
const END_TURN: i32 = 15;

fn configs() -> Vec<Config> {
    let mut out = Vec::new();
    for (seed, seats, mode) in [
        (1, 2, 4),
        (2, 2, 2),
        (3, 3, 4),
        (4, 3, 2),
        (5, 4, 4),
        (6, 4, 2),
    ] {
        for rules in [123, 124] {
            out.push(Config {
                rules,
                seed,
                size: 8,
                map: 2,
                mode,
                tribes: vec![7; seats],
                action_limit: DECISIONS,
            });
        }
    }
    out
}

/// Seeded random play: never Resign, EndTurn one time in six.
fn choose(seed: i32, decision: u64, moves: &[[i32; 6]]) -> usize {
    let mut x = (seed as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ decision;
    x = (x ^ (x >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let end = moves.iter().position(|m| m[0] == END_TURN);
    let kept: Vec<usize> = (0..moves.len())
        .filter(|&i| moves[i][0] != RESIGN && moves[i][0] != END_TURN)
        .collect();
    match end {
        Some(e) if kept.is_empty() || (x >> 40).is_multiple_of(6) => e,
        _ => kept[((x >> 33) as usize) % kept.len()],
    }
}

fn mover(g: &Game) -> i32 {
    g.players[g.current_player_index as usize].action.id
}

#[test]
fn other_view_is_the_view_of_the_seat_the_turn_passes_to() {
    let t = Tables::compile(CATALOG).unwrap();
    let configs = configs();
    let mut batch = Batch::new(CATALOG, configs.len() + 1, 3).unwrap();
    batch
        .reset(&configs.iter().cloned().enumerate().collect::<Vec<_>>())
        .unwrap();
    for i in 0..configs.len() {
        assert_eq!(batch.status(i), Status::Playing, "{}", batch.reason(i));
    }
    assert!(batch.other_view(configs.len()).unwrap().is_none());
    let mut applied = vec![Applied::Skipped; batch.len()];
    // per slot, the other seat when the last decision was an EndTurn
    let mut passed: Vec<Option<i32>> = vec![None; batch.len()];
    let (mut decisions, mut handovers, mut differs) = (0u64, 0u64, 0u64);
    for decision in 0..u64::from(DECISIONS) {
        let mut moves = vec![SKIP; batch.len()];
        for (i, c) in configs.iter().enumerate() {
            if batch.status(i) != Status::Playing {
                assert!(batch.other_view(i).unwrap().is_none());
                passed[i] = None;
                continue;
            }
            let g = batch.game(i).unwrap();
            if let Some(seat) = passed[i].take() {
                assert_eq!(mover(g), seat, "game {i} decision {decision}");
                handovers += 1;
            }
            let seat = other_seat(g).unwrap().expect("a game of two or more seats");
            assert_ne!(seat, mover(g));
            let mut expected = View::default();
            view::write(g, &t, seat, &mut expected).unwrap();
            let got = batch.other_view(i).unwrap().unwrap();
            assert_eq!(got, expected, "game {i} decision {decision}");
            assert_eq!(i32::from(got.viewer), seat);
            differs += u64::from(got.tiles != batch.view(i).unwrap().tiles);
            decisions += 1;
            moves[i] = choose(c.seed, decision, batch.moves(i)) as i32;
            if batch.moves(i)[moves[i] as usize][0] == END_TURN {
                passed[i] = Some(seat);
            }
        }
        if moves.iter().all(|&m| m == SKIP) {
            break;
        }
        batch.step(&moves, &mut applied).unwrap();
    }
    // every game played on, and the turn passed many times in each seat count
    assert!(
        decisions > 5_000 && handovers > 500 && differs > decisions / 2,
        "{decisions} {handovers} {differs}"
    );
}

/// Take player `p` out of the game: its tiles go to nobody (owner 0) and its units leave the
/// board, so it is dead under both death conditions.
fn kill(g: &mut Game, p: i32) {
    for i in 0..g.tiles.len() {
        let unit = g.tiles[i].unit.filter(|&n| g.units[n as usize].owner == p);
        let x = g.tile_mut(i);
        if x.owner == p {
            x.owner = 0;
        }
        if unit.is_some() {
            x.unit = None;
        }
    }
}

#[test]
fn dead_players_and_nature_are_skipped() {
    let t = Tables::compile(CATALOG).unwrap();
    let config = Config {
        rules: 124,
        seed: 7,
        size: 8,
        map: 2,
        mode: 2,
        tribes: vec![7; 4],
        action_limit: 100,
    };
    let mut g = reset::reset(&config, CATALOG, &t).unwrap();
    let order: Vec<i32> = g.players.iter().map(|p| p.action.id).collect();
    assert!(
        order.contains(&255),
        "Nature is in the turn order: {order:?}"
    );
    let seats: Vec<i32> = order.iter().copied().filter(|&p| p != 255).collect();
    let from = g.current_player_index as usize;
    let next_of = |k: usize| {
        (1..order.len())
            .map(|d| order[(from + d) % order.len()])
            .filter(|&p| p != 255)
            .nth(k)
            .unwrap()
    };
    assert_eq!(seats.len(), 4);
    assert_eq!(other_seat(&g).unwrap(), Some(next_of(0)));
    kill(&mut g, next_of(0));
    assert_eq!(other_seat(&g).unwrap(), Some(next_of(1)));
    kill(&mut g, next_of(1));
    assert_eq!(other_seat(&g).unwrap(), Some(next_of(2)));
    // every other player dead (the game ends at the mover's EndTurn): the next one all the same
    kill(&mut g, next_of(2));
    assert_eq!(other_seat(&g).unwrap(), Some(next_of(0)));
    let v = engine_fast_batch::outputs::other_view(&g, &t)
        .unwrap()
        .unwrap();
    assert_eq!(i32::from(v.viewer), next_of(0));
}
