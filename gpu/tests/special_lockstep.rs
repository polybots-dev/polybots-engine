//! The special tribes' rules on the host build against engine-fast. Random games with Aquarion,
//! Polaris, Elyrion or Cymanti beside a regular tribe: at every decision the host build's legal
//! list, view and step must equal engine-fast's on the same state; nothing may stop as "not
//! ported" (a text the kernels have and engine-fast does not).
//!
//! Random play is a smoke test, not verification.

use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::{menu, step, view};
use engine_fast_frame::Action;
use engine_fast_gpu::step::twin_advance;
use engine_fast_gpu::{check, menu as rows, state as device, tables, view as views};
use std::collections::BTreeMap;

const CATALOG: &str = include_str!("../../rules/rules-124.json");
const END_TURN: i32 = 15;
const RESIGN: i32 = 35;

#[derive(Default)]
struct Tally {
    lists: usize,
    steps: usize,
    not_ported: BTreeMap<String, usize>,
}

fn choose(legal: &[menu::Cand], rng: &mut u64) -> Option<Action> {
    *rng = rng
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let r = (*rng >> 33) as usize;
    let others: Vec<usize> = (0..legal.len())
        .filter(|&i| ![END_TURN, RESIGN].contains(&legal[i].action.kind))
        .collect();
    let pick = match legal.iter().position(|c| c.action.kind == END_TURN) {
        Some(e) if others.is_empty() || r.is_multiple_of(8) => e,
        _ if !others.is_empty() => others[(r / 8) % others.len()],
        _ => return None,
    };
    Some(legal[pick].action.clone())
}

/// The first part of two canonical states that differs, both sides shown.
fn first_difference(a: &Game, b: &Game) -> String {
    for (i, (x, y)) in a.tiles.iter().zip(&b.tiles).enumerate() {
        if x != y {
            return format!("tile {i}:\n port   {x:?}\n engine {y:?}");
        }
    }
    if a.units.len() != b.units.len() {
        return format!("unit count {} / {}", a.units.len(), b.units.len());
    }
    for (i, (x, y)) in a.units.iter().zip(&b.units).enumerate() {
        if x != y {
            return format!(
                "unit {i}:\n port   {x:?} {:?}\n engine {y:?} {:?}",
                a.effect_lists.get(x.effects as usize),
                b.effect_lists.get(y.effects as usize)
            );
        }
    }
    for (i, (x, y)) in a.effect_lists.iter().zip(&b.effect_lists).enumerate() {
        if x != y {
            return format!("effect list {i}: {x:?} / {y:?}");
        }
    }
    for (i, (x, y)) in a.players.iter().zip(&b.players).enumerate() {
        if x != y {
            return format!("player {i}:\n port   {x:?}\n engine {y:?}");
        }
    }
    if a.actions != b.actions {
        return format!("actions:\n port   {:?}\n engine {:?}", a.actions, b.actions);
    }
    format!(
        "elsewhere (turn {} / {}, unit id {} / {})",
        a.current_turn, b.current_turn, a.current_unit_id, b.current_unit_id
    )
}

fn not_ported(text: &str, tally: &mut Tally) -> bool {
    if text.starts_with("not ported to the device") || text.ends_with("not implemented") {
        *tally.not_ported.entry(text.to_owned()).or_default() += 1;
        return true;
    }
    false
}

fn play(
    t: &Tables,
    flat: &tables::DeviceTables,
    size: i32,
    tribes: Vec<i32>,
    seed: i32,
    decisions: usize,
    tally: &mut Tally,
) {
    let config = Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes: tribes.clone(),
        action_limit: 10_000,
    };
    let mut g: Game = reset::reset(&config, CATALOG, t).unwrap();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64 ^ seed as u64;
    for _ in 0..decisions {
        let at = format!("tribes {tribes:?}, seed {seed}, decision {}", g.decisions);
        let Ok(legal) = menu::menu(&g, t) else { return };
        let Some(action) = choose(&legal, &mut rng) else {
            return;
        };
        // a state holding a queued action the kernels do not have yet cannot be a block
        let block = match device::to_device(&g) {
            Ok(b) => b,
            Err(e) => {
                assert!(not_ported(&e, tally), "{at}: {e}");
                if step::advance(&mut g, t, &action).is_err() || g.current_state == 4 {
                    return;
                }
                continue;
            }
        };
        // the legal list
        let want = check::engine_rows(&g, t);
        match rows::twin_menu(&block, flat) {
            Ok(got) => {
                assert_eq!(Ok(&got), want.as_ref(), "{at}: the legal lists differ");
                tally.lists += 1;
            }
            Err(s) if not_ported(&s.text, tally) => {}
            Err(s) => assert_eq!(
                Err(&s.text),
                want.as_ref().map(|_| &s.text),
                "{at}: {}",
                s.text
            ),
        }
        // the view of the player to move
        let mover = g.players[g.current_player_index as usize].action.id;
        let mut v = view::View::default();
        let want_view = view::write(&g, t, mover, &mut v).map(|()| v);
        match views::twin(&block, flat, mover) {
            Err(e) if not_ported(&e, tally) => {}
            got => assert_eq!(got, want_view, "{at}: the views differ"),
        }
        // the step
        let before = g.clone();
        let rust = step::advance(&mut g, t, &action);
        let mut ours = block;
        match (twin_advance(&mut ours, flat, &action), &rust) {
            (Err(s), _) if not_ported(&s.text, tally) => {}
            (Ok(a), Ok(b)) => {
                assert_eq!(a, *b, "{at}: the elapsed flag differs ({action:?})");
                let got = device::from_device(&ours, &before).unwrap();
                let mut got = device::canonical(&got).unwrap();
                let want = device::canonical(&g).unwrap();
                got.side = want.side.clone();
                got.memory = want.memory.clone();
                if got != want {
                    // the engine's actions for this decision, for finding the rule
                    let mut replay = before.clone();
                    let mut seen = Vec::new();
                    let _ = step::command(&mut replay, t, &action, |a| seen.push(format!("{a:?}")));
                    panic!(
                        "{at}: the states differ after {action:?}: {}\nengine ran:\n{}",
                        first_difference(&got, &want),
                        seen.join("\n")
                    );
                }
                tally.steps += 1;
            }
            (Err(s), Err(e)) => assert_eq!(&s.text, e, "{at}: the errors differ"),
            (got, want) => panic!(
                "{at}: {action:?}: port {:?}, engine {want:?}",
                got.map_err(|s| s.text)
            ),
        }
        if rust.is_err() || g.current_state == 4 {
            return;
        }
    }
}

#[test]
fn the_special_tribes_equal_the_engine() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut tally = Tally::default();
    let imperius = t.tribes.id_of["imperius"] as i32;
    for (k, name) in ["aquarion", "polaris", "elyrion", "cymanti"]
        .iter()
        .enumerate()
    {
        let tribe = t.tribes.id_of[*name] as i32;
        for (size, seed) in [(11, 100 + k as i32), (14, 200 + k as i32)] {
            play(
                &t,
                &flat,
                size,
                vec![tribe, imperius],
                seed,
                600,
                &mut tally,
            );
            play(
                &t,
                &flat,
                size,
                vec![imperius, tribe, tribe],
                seed + 50,
                600,
                &mut tally,
            );
        }
    }
    println!(
        "{} lists and {} steps equal; not ported: {:#?}",
        tally.lists, tally.steps, tally.not_ported
    );
    assert!(tally.lists > 1_000);
    assert!(tally.not_ported.is_empty());
}
