//! A special tribe's tech tree in the kernels (the rules themselves: tests/special_lockstep.rs).

use engine_fast::reset::{self, Config};
use engine_fast::tables::Tables;
use engine_fast_gpu::check;
use engine_fast_gpu::state as device;
use engine_fast_gpu::tables::DeviceTables;

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));

fn game(t: &Tables, tribes: Vec<i32>) -> engine_fast::state::Game {
    let config = Config {
        rules: 124,
        seed: 5,
        size: 11,
        map: 2,
        mode: 4,
        tribes,
        action_limit: 300,
    };
    reset::reset(&config, CATALOG, t).unwrap()
}

/// Water Magic (Elyrion's in place of Whaling) unlocks an improvement the catalog lacks, which
/// the engine skips as the game does: a player holding it gets the engine's legal list from the
/// port, not a table error.
#[test]
fn an_unlock_the_catalog_lacks_leaves_the_legal_list_alone() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = DeviceTables::new(&t).unwrap();
    let mut g = game(&t, vec![7, 7]);
    let mover = g.current_player_index as usize;
    g.players[mover]
        .available_tech
        .push(t.techs.id("watermagic").unwrap() as i32);
    let block = device::to_device(&g).unwrap();
    let got = engine_fast_gpu::menu::twin_menu(&block, &flat).map_err(|s| s.text);
    assert_eq!(got, check::engine_rows(&g, &t));
}

/// Aquarion's ruin at sea (ruins.rs lost_city): an Aquarion unit examining a ruin on a valid
/// ocean tile raises a flooded city on a new continent and places resources around it; the
/// host build's state, the continents included, equals engine-fast's.
#[test]
fn a_lost_city_rises_as_in_the_engine() {
    use engine_fast::state::{Improvement, Xy};
    use engine_fast_gpu::step::twin_advance;
    let t = Tables::compile(CATALOG).unwrap();
    let flat = DeviceTables::new(&t).unwrap();
    let aquarion = t.tribes.id_of["aquarion"] as i32;
    let mut cases = 0;
    for seed in 1..40 {
        let config = Config {
            rules: 124,
            seed,
            size: 14,
            map: 3,
            mode: 4,
            tribes: vec![aquarion, 7],
            action_limit: 300,
        };
        let mut g = reset::reset(&config, CATALOG, &t).unwrap();
        let mover = g.players[g.current_player_index as usize].action.id;
        if g.players[g.current_player_index as usize].tribe != aquarion {
            continue;
        }
        // a valid lost-city tile: unowned ocean off the edge, no city or ruin within two
        let w = g.width;
        let h = g.tiles.len() as i32 / w;
        let near_city = |g: &engine_fast::state::Game, c: Xy| {
            (c.x - 2..=c.x + 2).any(|x| {
                (c.y - 2..=c.y + 2).any(|y| {
                    g.tile(Xy { x, y })
                        .and_then(|x| x.improvement.as_ref())
                        .is_some_and(|m| m.kind == 1 || m.kind == 2)
                })
            })
        };
        let Some(i) = (0..g.tiles.len()).find(|&i| {
            let x = &g.tiles[i];
            let c = x.coordinates;
            c.x > 0
                && c.y > 0
                && c.x < w - 1
                && c.y < h - 1
                && x.owner == 0
                && x.terrain == 2
                && x.unit.is_none()
                && !near_city(&g, c)
        }) else {
            continue;
        };
        let c = g.tiles[i].coordinates;
        let Some(node) = g
            .tiles
            .iter()
            .filter_map(|x| x.unit)
            .find(|&n| g.units[n as usize].owner == mover)
        else {
            continue;
        };
        // the mover's unit stands on a ruin there, ready to act
        let from = g.units[node as usize].coordinates;
        let fi = g.index(from).unwrap();
        g.tiles[fi].unit = None;
        g.tiles[i].unit = Some(node);
        let u = &mut g.units[node as usize];
        u.coordinates = c;
        u.moved = false;
        u.attacked = false;
        g.tiles[i].improvement = Some(Improvement {
            kind: t.improvements.id("ruin").unwrap() as i32,
            owner: 0,
            founder: 0,
            level: 0,
            founded: 0,
            xp: 0,
            population: 0,
            production: 0,
            base_score: 0,
            border_size: 0,
            upgrade: 0,
            connected_to_capital_of_player: 0,
            named: false,
            rewards: Default::default(),
            effects: Default::default(),
        });
        let action = engine_fast_frame::Action {
            kind: 14,
            source: Some(engine_fast_frame::Xy { x: c.x, y: c.y }),
            target: None,
            opponent: None,
            option: None,
            accept: None,
        };
        let before = g.clone();
        let mut block = device::to_device(&g).unwrap();
        let rust = engine_fast::step::advance(&mut g, &t, &action);
        let port = twin_advance(&mut block, &flat, &action).map_err(|s| s.text);
        assert_eq!(
            port.is_ok(),
            rust.is_ok(),
            "seed {seed}: port {port:?}, engine {rust:?}"
        );
        if rust.is_err() {
            continue;
        }
        let city = g.tile(c).unwrap();
        assert!(
            city.improvement.as_ref().is_some_and(|m| m.kind == 1),
            "seed {seed}: no lost city"
        );
        let got = device::canonical(&device::from_device(&block, &before).unwrap()).unwrap();
        let want = device::canonical(&g).unwrap();
        assert_eq!(got.tiles, want.tiles, "seed {seed}");
        assert_eq!(
            g.continents(),
            got.continents(),
            "seed {seed}: the continents differ"
        );
        let (mut got, want) = (got, want);
        got.side = want.side.clone();
        assert!(got == want, "seed {seed}: the states differ");
        cases += 1;
    }
    assert!(cases >= 3, "only {cases} lost cities made");
}
