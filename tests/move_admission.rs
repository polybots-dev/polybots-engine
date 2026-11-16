//! A Move outside the legal list is refused, as the game refuses
//! it: a unit that has moved or is frozen, or a
//! destination with no path within the unit's movement, with the game unchanged by the path
//! check. Not covered: a Move onto a hidden enemy (it becomes a Reveal; engine-fast does not
//! check its path, where the game does, so engine-fast refuses less there).
use engine_fast::reset::{reset, Config};
use engine_fast::state::Game;
use engine_fast::step;
use engine_fast::tables::Tables;
use engine_fast_frame::{Action, Xy};

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
const MOVE: i32 = 6;
const FROZEN: i64 = 0;

fn game(t: &Tables) -> Game {
    let config = Config {
        rules: 124,
        seed: 5,
        size: 11,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit: 4096,
    };
    reset(&config, CATALOG, t).unwrap()
}

/// The mover's starting unit: its node and tile.
fn starting_unit(g: &Game) -> (usize, Xy) {
    let p = g.players[g.current_player_index as usize].action.id;
    let tile = g
        .tiles
        .iter()
        .find(|x| x.unit.is_some_and(|n| g.units[n as usize].owner == p))
        .unwrap();
    let c = tile.coordinates;
    (tile.unit.unwrap() as usize, Xy { x: c.x, y: c.y })
}

fn move_to(from: Xy, to: Xy) -> Action {
    Action {
        kind: MOVE,
        source: Some(from),
        target: Some(to),
        ..Action::default()
    }
}

/// a tile the unit cannot reach in one move: the far corner from it
fn far(g: &Game, from: Xy) -> Xy {
    let w = g.width;
    Xy {
        x: if from.x < w / 2 { w - 1 } else { 0 },
        y: if from.y < w / 2 { w - 1 } else { 0 },
    }
}

#[test]
fn a_move_with_no_path_is_refused() {
    let t = Tables::compile(CATALOG).unwrap();
    let mut g = game(&t);
    let (_, from) = starting_unit(&g);
    let to = far(&g, from);
    let err = step::advance(&mut g, &t, &move_to(from, to)).unwrap_err();
    assert!(err.contains("not a legal move"), "{err}");
}

#[test]
fn a_move_by_a_unit_that_cannot_move_is_refused() {
    let t = Tables::compile(CATALOG).unwrap();
    for frozen in [false, true] {
        let mut g = game(&t);
        let (node, from) = starting_unit(&g);
        let next = Xy {
            x: from.x + if from.x > 0 { -1 } else { 1 },
            y: from.y,
        };
        if frozen {
            let list = g.units[node].effects as usize;
            g.effect_lists[list].push(FROZEN);
        } else {
            g.units[node].moved = true;
        }
        let err = step::advance(&mut g, &t, &move_to(from, next)).unwrap_err();
        assert!(err.contains("cannot move"), "frozen {frozen}: {err}");
    }
}

/// `reset` refuses tables compiled from another catalogue text than the one it is given (the
/// game carries the text; its rules come from the tables).
#[test]
fn reset_refuses_tables_from_another_catalogue() {
    let other = format!("{CATALOG} ");
    let t = Tables::compile(&other).unwrap();
    let config = Config {
        rules: 124,
        seed: 5,
        size: 11,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit: 4096,
    };
    let err = reset(&config, CATALOG, &t).err().unwrap();
    assert!(err.contains("catalogue"), "{err}");
    assert!(reset(&config, &other, &t).is_ok());
}
