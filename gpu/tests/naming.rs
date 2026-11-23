//! City names (capture.rs city_name) on the host build against the names the engine gives
//! capitals at reset, which draw them through the same function.
//!
//! A capture compares only whether the city is named and what the draw consumes, since the
//! block holds no text; this compares the words themselves, for every tribe the catalog
//! lets a game start with, and the names a capture of a village gives a player with each skin.
//! Not covered: draws that need a default-Random seed, since the reset's queue has moved on
//! (counted, not compared).

use engine_fast::reset::{self, Config};
use engine_fast::tables::Tables;
use engine_fast_gpu::step::twin_city_name;
use engine_fast_gpu::{state as device, tables};

const CATALOG: &str = include_str!("../../rules/rules-124.json");

#[test]
fn the_ports_city_names_are_the_engines() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let (mut compared, mut tribes, mut skipped) = (0, Vec::new(), 0);
    for tribe in 0..40 {
        for seed in 0..6 {
            let config = Config {
                rules: 124,
                seed: 100 + seed,
                size: 11,
                map: 2,
                mode: 4,
                tribes: vec![tribe, 7],
                action_limit: 10_000,
            };
            let Ok(g) = reset::reset(&config, CATALOG, &t) else {
                continue;
            };
            for p in &g.players {
                let c = p.start_tile;
                let Some(i) = g.index(c) else { continue };
                let Some(want) = g.side.names[i].clone() else {
                    continue;
                };
                let mut block = device::to_device(&g).unwrap();
                let seeds = block.naming_next;
                let got = twin_city_name(&mut block, &flat, p.tribe, p.skin, c.x, c.y);
                if block.naming_next != seeds {
                    skipped += 1;
                    continue;
                }
                assert_eq!(got.as_ref(), Ok(&want), "tribe {}, seed {seed}", p.tribe);
                compared += 1;
                if !tribes.contains(&p.tribe) {
                    tribes.push(p.tribe);
                }
            }
        }
    }
    println!("compared {compared} names of tribes {tribes:?}; {skipped} needed a default seed");
    assert!(tribes.len() >= 12, "only tribes {tribes:?} started a game");
}

/// A capture of a village names it in the capturer's language, the skin's where the catalog
/// gives the skin one: the port's word equals the name the engine stores, for several skins.
#[test]
fn a_captured_villages_name_is_the_engines_for_each_skin() {
    use engine_fast::{menu, step};
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut compared = 0;
    for seed in 200..212 {
        for skin in [0u8, 2, 17, 18, 19] {
            let config = Config {
                rules: 124,
                seed,
                size: 14,
                map: 2,
                mode: 4,
                tribes: vec![[7, 2, 4, 6][seed as usize % 4], 7],
                action_limit: 10_000,
            };
            let Ok(mut g) = reset::reset(&config, CATALOG, &t) else {
                continue;
            };
            let mover = g.current_player_index as usize;
            let me = g.players[mover].action.id;
            g.players[mover].skin = skin.into();
            let Some(v) = g.tiles.iter().position(|x| {
                x.unit.is_none()
                    && x.owner == 0
                    && x.improvement
                        .as_ref()
                        .is_some_and(|m| m.kind == 1 && !m.named)
            }) else {
                continue;
            };
            let capital = g.tile(g.players[mover].start_tile).unwrap().unit.unwrap() as usize;
            let mut u = g.units[capital];
            u.id = 7_000 + seed as u32;
            u.coordinates = g.tiles[v].coordinates;
            let node = g.push_unit(u);
            g.set_unit_at(v, Some(node));
            if !g.tiles[v].explorers.contains(&me) {
                g.tile_mut(v).explorers.push(me);
            }
            let c = g.tiles[v].coordinates;
            let at = Some(engine_fast_frame::Xy { x: c.x, y: c.y });
            let Ok(legal) = menu::menu(&g, &t) else {
                continue;
            };
            let Some(capture) = legal
                .iter()
                .find(|x| x.action.kind == 7 && x.action.source == at)
            else {
                continue;
            };
            let mut block = device::to_device(&g).unwrap();
            let tribe = g.players[mover].tribe;
            let ours = twin_city_name(&mut block, &flat, tribe, i32::from(skin), c.x, c.y);
            if block.naming_next != device::to_device(&g).unwrap().naming_next {
                continue;
            }
            let mut after = g.clone();
            step::advance(&mut after, &t, &capture.action).unwrap();
            let want = after.side.names[v]
                .clone()
                .expect("the captured village is named");
            assert_eq!(ours.as_ref(), Ok(&want), "seed {seed}, skin {skin}");
            compared += 1;
        }
    }
    println!("compared {compared} village names");
    assert!(compared >= 20, "only {compared} names compared");
}
