//! The view written by the
//! kernel sources equals `engine_fast::view::write` at every decision of random games on
//! boards of 11, 14 and 16 with two to four seats, for the player to move and for every
//! other seat; with the `cuda` feature the GPU writes the same views.
//!
//! The other tribes the engine starts games for are played too, and every plane must take a
//! value other than unknown and 0 somewhere in the compared views (so a plane the port
//! wrote into the wrong place would show). Not covered: the rainbow-vision ruin hint (no
//! tested tribe has it), seats met in an order other than by seat, boards that are not
//! square; the invisible-unit rule and the error paths have direct tests.

use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::view::{self, View};
use engine_fast::{menu, step};
use engine_fast_gpu::{state as device, tables, view as gpu_view};
use std::mem::size_of;

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
const END_TURN: i32 = 15;
const RESIGN: i32 = 35;
const GAMES: [(i32, usize, i32); 8] = [
    (11, 2, 21),
    (11, 4, 22),
    (14, 2, 23),
    (14, 3, 24),
    (16, 2, 25),
    (16, 4, 26),
    // larger boards; random play, a smoke test
    (20, 3, 27),
    (30, 4, 28),
];

/// Play up to `decisions` random decisions and hand every state between decisions to `each`.
/// The game stops early where the engine refuses to go on (a rule outside its scope).
fn play(
    t: &Tables,
    size: i32,
    tribes: Vec<i32>,
    seed: i32,
    decisions: usize,
    mut each: impl FnMut(&Game),
) {
    let config = Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes,
        action_limit: 10_000,
    };
    let Ok(mut g) = reset::reset(&config, CATALOG, t) else {
        return;
    };
    let mut rng = 0x9E37_79B9_7F4A_7C15u64 ^ seed as u64;
    for _ in 0..decisions {
        each(&g);
        let Ok(legal) = menu::menu(&g, t) else { return };
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = (rng >> 33) as usize;
        let others: Vec<usize> = (0..legal.len())
            .filter(|&i| ![END_TURN, RESIGN].contains(&legal[i].action.kind))
            .collect();
        let pick = match legal.iter().position(|c| c.action.kind == END_TURN) {
            Some(e) if others.is_empty() || r.is_multiple_of(10) => e,
            _ if !others.is_empty() => others[(r / 10) % others.len()],
            _ => return,
        };
        if step::advance(&mut g, t, &legal[pick].action).is_err() {
            return;
        }
    }
    each(&g);
}

/// Games of the other tribes the device takes, four to a game, each with a seed: every
/// standard tribe but Imperius, the first game filled up with it, then the four special tribes.
fn other_tribes(t: &Tables) -> Vec<(i32, Vec<i32>)> {
    let games = vec![
        (31, vec![2, 4, 6, 7]),
        (32, vec![8, 9, 10, 11]),
        (33, vec![12, 13, 14, 15]),
        (34, vec![3, 5, 16, 17]),
    ];
    for (seed, tribes) in &games {
        let config = Config {
            rules: 124,
            seed: *seed,
            size: 11,
            map: 2,
            mode: 4,
            tribes: tribes.clone(),
            action_limit: 10,
        };
        reset::reset(&config, CATALOG, t).unwrap();
    }
    games
}

fn seats(g: &Game) -> Vec<i32> {
    g.players
        .iter()
        .map(|p| p.action.id)
        .filter(|&id| id != 255)
        .collect()
}

fn reference(g: &Game, t: &Tables, viewer: i32) -> Result<View, String> {
    let mut out = View::default();
    view::write(g, t, viewer, &mut out).map(|()| out)
}

#[test]
fn the_blocks_have_the_c_layout() {
    assert_eq!(tables::twin_size(), size_of::<tables::EfTables>());
    for tile_cap in [121, 256, 900] {
        assert_eq!(gpu_view::twin_size(tile_cap), gpu_view::view_size(tile_cap));
    }
    assert!(
        gpu_view::planes_match(),
        "the engine's view has other planes than the port"
    );
}

#[test]
fn the_host_build_writes_the_engines_view_at_every_decision() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut views = 0;
    let mut seen = [0usize; view::TILE_PLANES.len()];
    let games = GAMES.map(|(size, n, seed)| (size, vec![7; n], seed));
    let others = other_tribes(&t)
        .into_iter()
        .map(|(seed, tribes)| (14, tribes, seed));
    for (size, tribes, seed) in games.into_iter().chain(others) {
        play(&t, size, tribes, seed, 400, |g| {
            let block = device::to_device(g).unwrap();
            for viewer in seats(g) {
                let want = reference(g, &t, viewer);
                let got = gpu_view::twin(&block, &flat, viewer);
                assert!(
                    got == want,
                    "size {size}, decision {}, viewer {viewer}: the views differ",
                    g.decisions
                );
                if let Ok(v) = &got {
                    for (p, count) in seen.iter_mut().enumerate() {
                        *count += v
                            .tile_plane(p)
                            .iter()
                            .filter(|&&x| x != view::UNKNOWN && x != 0)
                            .count();
                    }
                }
                views += 1;
            }
        });
    }
    assert!(views > 8_000, "only {views} views compared");
    // the rainbow-vision hint needs a tribe with that ability, which no tested tribe has;
    // random play puts no effect on a tile or unit, so those two planes have their own test
    let unseen: Vec<&str> = (0..seen.len())
        .filter(|&p| {
            seen[p] == 0
                && !["ruin_hint", "effects", "unit_effects"].contains(&view::TILE_PLANES[p])
        })
        .map(|p| view::TILE_PLANES[p])
        .collect();
    assert!(
        unseen.is_empty(),
        "planes never other than unknown or 0: {unseen:?}"
    );
}

/// Other tribes change what the view shows (tech, resources revealed, rainbow vision).
#[test]
fn the_host_build_writes_the_view_for_other_tribes() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut views = 0;
    for (seed, tribes) in other_tribes(&t) {
        play(&t, 14, tribes, seed, 200, |g| {
            let block = device::to_device(g).unwrap();
            for viewer in seats(g) {
                let got = gpu_view::twin(&block, &flat, viewer);
                assert!(
                    got == reference(g, &t, viewer),
                    "seed {seed}, decision {}, viewer {viewer}",
                    g.decisions
                );
                views += 1;
            }
        });
    }
    assert!(views > 1_000, "only {views} views compared");
}

/// The comparison sees a changed plane value, and a refusal carries the engine's words.
#[test]
fn a_changed_tile_and_a_refusal_are_seen() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut last = None;
    play(&t, 11, vec![7, 7], 21, 80, |g| last = Some(g.clone()));
    let mut g = last.unwrap();
    let viewer = seats(&g)[0];
    let before = gpu_view::twin(&device::to_device(&g).unwrap(), &flat, viewer).unwrap();
    let explored = g
        .tiles
        .iter()
        .position(|x| x.explorers.contains(&viewer))
        .unwrap();
    g.tiles[explored].terrain += 1;
    let after = gpu_view::twin(&device::to_device(&g).unwrap(), &flat, viewer);
    assert!(after.as_ref() != Ok(&before) && after == reference(&g, &t, viewer));
    // an effect the view's bit set cannot hold
    g.tiles[explored].effects.push(15);
    let refused = gpu_view::twin(&device::to_device(&g).unwrap(), &flat, viewer);
    assert_eq!(refused, reference(&g, &t, viewer));
    assert_eq!(
        refused.unwrap_err(),
        "effect 15 does not fit the view's bit set"
    );
    assert_eq!(
        gpu_view::twin(&device::to_device(&g).unwrap(), &flat, 99),
        reference(&g, &t, 99)
    );
}

/// The catalog names an improvement it does not define (Water Magic unlocks `enchantwhale`); the
/// engine skips that null reference as the game does, and a viewer that has the tech gets the
/// engine's view from the port too.
#[test]
fn an_unlock_the_catalog_lacks_is_skipped_as_the_engine_skips_it() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let watermagic = t.techs.id("watermagic").unwrap();
    assert!(!t.improvements.id_of.contains_key("enchantwhale"));
    let entry = &flat.block.trees[0].links[watermagic];
    assert_eq!(
        (
            entry.reveals_error,
            entry.improvements_error,
            entry.embark_unlock_error
        ),
        (0, 0, 0)
    );
    let mut last = None;
    play(&t, 11, vec![7, 7], 21, 20, |g| last = Some(g.clone()));
    let mut g = last.unwrap();
    let viewer = seats(&g)[0];
    let i = g
        .players
        .iter()
        .position(|p| p.action.id == viewer)
        .unwrap();
    g.players[i].available_tech.push(watermagic as i32);
    let got = gpu_view::twin(&device::to_device(&g).unwrap(), &flat, viewer);
    let want = reference(&g, &t, viewer);
    assert!(
        got.is_ok() && got == want,
        "port {:?}, engine {:?}",
        got.err(),
        want.err()
    );
}

/// The viewer's own unit with an effect the bit set cannot hold, a 64-bit effect id, and a
/// block with a list longer than its array: the first two give the engine's reasons, the
/// third is caught as a broken invariant instead of being read past the array.
#[test]
fn bad_effects_and_a_corrupted_block() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut last = None;
    play(&t, 11, vec![7, 7], 21, 40, |g| last = Some(g.clone()));
    let mut g = last.unwrap();
    let viewer = seats(&g)[0];
    let tile = (0..g.tiles.len())
        .find(|&i| {
            g.tiles[i]
                .unit
                .is_some_and(|n| g.units[n as usize].owner == viewer)
        })
        .unwrap();
    let node = g.tiles[tile].unit.unwrap() as usize;
    for effect in [20i64, 127, 128, (1 << 32) + 3] {
        let mut list = engine_fast::inline::Inline::<i64, 9>::new();
        list.push(effect);
        g.effect_lists.push(list);
        g.units[node].effects = (g.effect_lists.len() - 1) as u32;
        // the block holds effect ids in a byte (up to 127): a wider one is refused at upload
        let block = device::to_device(&g);
        if effect > 127 {
            assert_eq!(
                block.err(),
                Some(format!(
                    "unit effect {effect} is outside the device's range"
                ))
            );
            g.units[node].effects = 0;
            g.effect_lists.pop();
            continue;
        }
        let got = gpu_view::twin(&block.unwrap(), &flat, viewer);
        assert_eq!(got, reference(&g, &t, viewer));
        assert_eq!(
            got.unwrap_err(),
            format!("effect {effect} does not fit the view's bit set")
        );
    }
    let mut block = device::to_device(&g).unwrap();
    block.players[0].available_tech_len = 57;
    let got = gpu_view::twin(&block, &flat, viewer).unwrap_err();
    assert!(got.starts_with("the device state is inconsistent"), "{got}");
    let mut block = device::to_device(&g).unwrap();
    block.tiles_mut()[3].unit_n = 30_000;
    assert!(gpu_view::twin(&block, &flat, viewer).is_err());
}

/// Tile and unit effects, which random play does not produce: every tile and every unit gets
/// a different set of effects the view can show, and the views of all seats are compared.
#[test]
fn tile_and_unit_effects_show_as_in_the_engine() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut states = Vec::new();
    play(&t, 14, vec![7, 7, 7], 24, 300, |g| {
        if g.decisions % 25 == 0 {
            states.push(g.clone());
        }
    });
    let mut shown = [0usize; 2];
    for (k, mut g) in states.into_iter().enumerate() {
        for (i, tile) in g.tiles.iter_mut().enumerate() {
            tile.effects.clear();
            for e in 0..((i + k) % 4) as i32 {
                tile.effects.push((i as i32 * 3 + e * 5 + k as i32) % 15);
            }
        }
        for node in 0..g.units.len() {
            let mut list = engine_fast::inline::Inline::<i64, 9>::new();
            for e in 0..((node + k) % 4) as i64 {
                list.push((node as i64 * 7 + e * 4) % 15);
            }
            g.effect_lists.push(list);
            g.units[node].effects = (g.effect_lists.len() - 1) as u32;
        }
        let block = device::to_device(&g).unwrap();
        for viewer in seats(&g) {
            let got = gpu_view::twin(&block, &flat, viewer);
            assert!(
                got == reference(&g, &t, viewer),
                "state {k}, viewer {viewer}"
            );
            let v = got.unwrap();
            for (n, plane) in [11, 17].into_iter().enumerate() {
                shown[n] += v.tile_plane(plane).iter().filter(|&&x| x > 0).count();
            }
        }
    }
    assert!(shown[0] > 100 && shown[1] > 20, "effects shown: {shown:?}");
}

/// Every error text of the kernel sources is the engine's own: the text (or its part before
/// the argument) appears in the engine's or the batch's source. The invariant check and the table-error
/// code have no text of their own in the engine.
#[test]
fn the_error_texts_are_the_engines() {
    // the engine's sources, and the batch's (its refresh stops a game with no legal move)
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let mut dirs = vec![
        std::path::PathBuf::from(root).join("src"),
        std::path::PathBuf::from(root).join("batch/src"),
    ];
    let mut source = String::new();
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            // a copied tree can hold metadata files that are not text (`._name.rs` on macOS)
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(path) {
                    source += &text;
                }
            }
        }
    }
    for text in tables::error_texts().iter().skip(1) {
        // the port's own reasons (capacity, a broken block, a rule not ported yet) name the device
        if text.is_empty() || text.contains("device") {
            continue;
        }
        let fixed = text.split("{}").next().unwrap();
        // texts the engine builds with format! from a literal and a name
        // (the literal before the name, the literal after it, the names it is built with)
        let stems: [(&str, &str, &[&str]); 2] = [
            ("attacking with ", " not implemented", &["consumed"]),
            (
                "action has no ",
                "",
                &["source", "target", "option", "opponent"],
            ),
        ];
        let built = stems.iter().any(|(before, after, names)| {
            source.contains(before)
                && source.contains(after)
                && names.iter().any(|n| fixed == format!("{before}{n}{after}"))
        });
        assert!(
            source.contains(fixed) || built,
            "the engine has no text {fixed:?}"
        );
    }
}

/// An invisible foreign unit is hidden unless the viewer is at peace with its owner.
#[test]
fn an_invisible_unit_shows_only_at_peace() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut last = None;
    play(&t, 11, vec![7, 7], 21, 300, |g| last = Some(g.clone()));
    let mut g = last.unwrap();
    let [me, other] = seats(&g)[..] else {
        panic!("two seats")
    };
    // a unit of the other player on a tile the viewer has explored
    let tile = (0..g.tiles.len())
        .find(|&i| {
            g.tiles[i].explorers.contains(&me)
                && g.tiles[i]
                    .unit
                    .is_some_and(|n| g.units[n as usize].owner == other)
        })
        .expect("the viewer sees an enemy unit after 300 decisions");
    let node = g.tiles[tile].unit.unwrap() as usize;
    let mut invisible = engine_fast::inline::Inline::<i64, 9>::new();
    invisible.push(3);
    g.effect_lists.push(invisible);
    g.units[node].effects = (g.effect_lists.len() - 1) as u32;
    let hidden = gpu_view::twin(&device::to_device(&g).unwrap(), &flat, me).unwrap();
    assert!(Ok(&hidden) == reference(&g, &t, me).as_ref());
    assert_eq!(hidden.tile_plane(13)[tile], 0, "the unit kind is hidden");
    let i = g.players.iter().position(|p| p.action.id == me).unwrap();
    let mut peace = engine_fast::state::Relation::DEFAULT;
    peace.state = engine_fast::state::PEACE;
    g.players[i].relations.retain(|(id, _)| *id != other);
    g.players[i].relations.push((other, peace));
    let shown = gpu_view::twin(&device::to_device(&g).unwrap(), &flat, me).unwrap();
    assert!(Ok(&shown) == reference(&g, &t, me).as_ref());
    assert_ne!(
        shown.tile_plane(13)[tile],
        0,
        "the unit kind shows at peace"
    );
}

#[cfg(feature = "cuda")]
#[test]
fn the_gpu_writes_the_engines_view() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let (mut blocks, mut viewers, mut want) = (Vec::new(), Vec::new(), Vec::new());
    let games = GAMES.map(|(size, n, seed)| (size, vec![7; n], seed, 400));
    let others = other_tribes(&t)
        .into_iter()
        .map(|(seed, tribes)| (14, tribes, seed, 200));
    for (size, tribes, seed, decisions) in games.into_iter().chain(others) {
        play(&t, size, tribes, seed, decisions, |g| {
            // every decision for the player to move, and the other seats now and then
            let mover = g.players[g.current_player_index as usize].action.id;
            for viewer in seats(g) {
                if viewer == mover || g.decisions % 16 == 0 {
                    // one launch holds blocks of one capacity: the largest board's
                    let (tiles, units) = device::default_caps(device::MAX_TILES);
                    blocks.push(device::to_device_with(g, tiles, units).unwrap());
                    viewers.push(viewer);
                    want.push(reference(g, &t, viewer));
                }
            }
        });
    }
    let got = gpu_view::device(&blocks, &flat, &viewers).unwrap();
    for (i, (got, want)) in got.iter().zip(&want).enumerate() {
        assert!(
            got == want,
            "view {i} (viewer {}) differs on the GPU",
            viewers[i]
        );
    }
    println!("{} views equal on the GPU", got.len());
}
