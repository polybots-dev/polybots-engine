//! A game survives the trip to the device block and back.
//!
//! Random games are played by the Rust engine on boards of 11, 14 and 16 with two to four
//! seats. At every decision the state goes `Game` -> block -> `Game` and must equal the
//! original in the canonical unit numbering; the host build must read the same values
//! through the block's fields as Rust reads from the game; and with the `cuda` feature the
//! block must come back from the GPU byte for byte, with the digest and the field values the
//! GPU computes equal to the host's.
//!
//! Not covered: the action stack, which is not in the block yet, and games longer than the
//! decisions played here.

use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::{menu, step};
use engine_fast_gpu::action::EfAction;
use engine_fast_gpu::dump;
use engine_fast_gpu::state as device;
use std::mem::{offset_of, size_of};

const CATALOG: &str = include_str!("../../rules/rules-124.json");
const END_TURN: i32 = 15;
const RESIGN: i32 = 35;

/// Play `decisions` random decisions (ending the turn one time in ten, never resigning) and
/// hand every state between decisions to `each`.
fn play(size: i32, seats: usize, seed: i32, decisions: usize, mut each: impl FnMut(&Game)) {
    let tables = Tables::compile(CATALOG).unwrap();
    let config = Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes: vec![7; seats],
        action_limit: 10_000,
    };
    let mut g = reset::reset(&config, CATALOG, &tables).unwrap();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64 ^ seed as u64;
    for _ in 0..decisions {
        each(&g);
        let legal = menu::menu(&g, &tables).unwrap();
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = (rng >> 33) as usize;
        let others: Vec<usize> = (0..legal.len())
            .filter(|&i| ![END_TURN, RESIGN].contains(&legal[i].action.kind))
            .collect();
        let end = legal.iter().position(|c| c.action.kind == END_TURN);
        let pick = match end {
            Some(e) if others.is_empty() || r.is_multiple_of(10) => e,
            _ if !others.is_empty() => others[(r / 10) % others.len()],
            _ => return,
        };
        step::advance(&mut g, &tables, &legal[pick].action).unwrap();
    }
    each(&g);
}

const GAMES: [(i32, usize, i32); 8] = [
    (11, 2, 1),
    (11, 4, 2),
    (14, 2, 3),
    (14, 3, 4),
    (16, 2, 5),
    (16, 4, 6),
    // larger boards; random play, a smoke test
    (20, 3, 7),
    (30, 4, 8),
];

/// What the host build's probe reads through the block's fields, read from the game instead.
fn probe(g: &Game) -> [i64; 4] {
    let (mut units, mut health) = (0, 0);
    for tile in &g.tiles {
        let mut node = tile.unit;
        while let Some(n) = node {
            units += 1;
            health += i64::from(g.units[n as usize].health);
            node = g.units[n as usize].passenger;
        }
    }
    let stars = g.players.iter().map(|p| i64::from(p.action.currency)).sum();
    let terrain = g.tiles.iter().map(|t| i64::from(t.terrain)).sum();
    [units, health, stars, terrain]
}

#[test]
fn the_rust_mirror_has_the_c_layout() {
    use device::*;
    let rust: Vec<i64> = [
        size_of::<EfXy>(),
        size_of::<EfImprovement>(),
        size_of::<EfTile>(),
        size_of::<EfUnit>(),
        size_of::<EfEffectList>(),
        size_of::<EfTask>(),
        size_of::<EfRelation>(),
        size_of::<EfPlayer>(),
        size_of::<EfTrigger>(),
        size_of::<EfGame>(),
        offset_of!(EfImprovement, effects_n),
        offset_of!(EfTile, improvement),
        offset_of!(EfTile, effects_n),
        offset_of!(EfUnit, birth_climate_skin_n),
        offset_of!(EfEffectList, items_n),
        offset_of!(EfTask, explored_tiles),
        offset_of!(EfPlayer, tasks),
        offset_of!(EfPlayer, relations),
        offset_of!(EfPlayer, message_kinds),
        offset_of!(EfPlayer, capital_count_cache),
        offset_of!(EfTrigger, coordinates),
        offset_of!(EfGame, rules_mode),
        offset_of!(EfGame, current_state),
        offset_of!(EfGame, triggers),
        offset_of!(EfGame, players),
        device::lists_offset(MAX_TILES as i32, MAX_UNITS as i32),
        device::units_offset(),
        device::tiles_offset(MAX_UNITS as i32),
        offset_of!(EfGame, naming_seeds),
        device::block_size(MAX_TILES as i32, MAX_UNITS as i32),
        offset_of!(EfGame, tile_cap),
        size_of::<EfAction>(),
        offset_of!(EfAction, w),
        offset_of!(EfAction, p),
        offset_of!(EfAction, list),
        offset_of!(EfGame, actions),
    ]
    .iter()
    .map(|&v| v as i64)
    .collect();
    assert_eq!(device::twin_layout(), rust);
}

#[test]
fn a_game_survives_the_block_at_every_decision() {
    let (mut states, mut with_actions) = (0, 0);
    for (size, seats, seed) in GAMES {
        play(size, seats, seed, 400, |g| {
            let block = device::to_device(g).unwrap();
            let back = device::from_device(&block, g).unwrap();
            let want = device::canonical(g).unwrap();
            assert!(
                back == want,
                "size {size}, {seats} seats, decision {}: the game changed in the block",
                g.decisions
            );
            // the block of the game that came back is the same block
            assert!(device::to_device(&back).unwrap().bytes() == block.bytes());
            assert_eq!(
                device::twin_probe(&block),
                probe(g),
                "the host build reads other field values"
            );
            // every field, read by name from the block in C and from the game in Rust
            let (c, rust) = (dump::of_block(&block), dump::of_game(g));
            if c != rust {
                let at = c.iter().zip(&rust).position(|(a, b)| a != b);
                panic!(
                    "size {size}, decision {}: the field lists differ at {at:?}",
                    g.decisions
                );
            }
            states += 1;
            with_actions += usize::from(!g.actions.is_empty());
        });
    }
    assert!(
        with_actions > 0,
        "no state had queued actions between decisions"
    );
    println!("{with_actions} of {states} states had queued actions");
    assert_eq!(states, GAMES.len() * 401);
}

/// The canonical numbering changes nothing a rule reads: the engine offers the same moves and
/// reaches the same state from a game and from its canonical form.
#[test]
fn the_canonical_form_plays_like_the_game() {
    let tables = Tables::compile(CATALOG).unwrap();
    let mut checked = 0;
    play(11, 2, 9, 300, |g| {
        let mut c = device::canonical(g).unwrap();
        let legal = menu::menu(g, &tables).unwrap();
        let ours = menu::menu(&c, &tables).unwrap();
        assert!(
            legal.len() == ours.len() && legal.iter().zip(&ours).all(|(a, b)| a.action == b.action)
        );
        let mut next = g.clone();
        let pick = &legal[(g.decisions as usize * 7) % legal.len()].action;
        if pick.kind == RESIGN {
            return;
        }
        step::advance(&mut next, &tables, pick).unwrap();
        step::advance(&mut c, &tables, pick).unwrap();
        assert!(
            device::canonical(&next).unwrap() == device::canonical(&c).unwrap(),
            "decision {}",
            g.decisions
        );
        checked += 1;
    });
    assert!(checked > 250);
}

/// The comparison can fail: a changed field shows in the game that comes back, in the bytes
/// and in the digest.
#[test]
fn a_changed_field_is_seen() {
    let mut last = None;
    play(11, 2, 1, 60, |g| last = Some(g.clone()));
    let g = last.unwrap();
    let want = device::canonical(&g).unwrap();
    let block = device::to_device(&g).unwrap();
    let digest = device::twin_digest(&block);
    let changes: [fn(&mut device::Block); 6] = [
        |d| d.tiles_mut()[60].terrain_n ^= 1,
        |d| d.units_mut()[0].health_n += 1,
        |d| d.players[1].currency += 1,
        |d| d.players[0].score += 1,
        |d| d.current_turn += 1,
        |d| d.tiles_mut()[120].explorers_len_n ^= 1,
    ];
    for change in changes {
        let mut changed = device::to_device(&g).unwrap();
        change(&mut changed);
        assert!(device::from_device(&changed, &g).map_or(true, |back| back != want));
        assert!(changed.bytes() != block.bytes());
        assert_ne!(device::twin_digest(&changed), digest);
    }
}

/// Fields the random games leave at their defaults (task counters, city rewards, messages,
/// monuments, the carried player fields, pending choices) are given distinct values, so a
/// field mapped to the wrong place on the way in and out would show.
#[test]
fn rarely_set_fields_survive_the_block() {
    let mut last = None;
    play(14, 3, 3, 120, |g| last = Some(g.clone()));
    let mut g = last.unwrap();
    let mut n = 1000;
    let mut next = || {
        n += 7;
        n
    };
    for p in &mut g.players {
        p.tribe_mix = next();
        p.resigned_turn = next();
        p.wipe_outs = next();
        p.killer_id = next();
        p.killed_turn = next();
        p.capital_count_cache = next();
        p.end_score = next();
        p.color = next();
        p.climate = next() as i32;
        p.handicap = next() as i32;
        p.skin = next() as i32;
        p.resigned_at_command = next();
        p.wiped_at_command = next();
        p.block_train_units = true;
        p.auto_play = true;
        p.has_chosen_tribe = true;
        p.action.opinions_dirty = true;
        p.built_unique_improvements.clear();
        p.built_unique_improvements.push(next() as i32);
        p.built_unique_improvements.push(next() as i32);
        p.messages.clear();
        p.messages.push(engine_fast::state::Message {
            kind: next(),
            sender: next() as i32,
        });
        p.action.tasks.clear();
        p.action.tasks.push(engine_fast::state::Task {
            kind: engine_fast::state::TaskKind(3),
            flags: engine_fast::state::TaskFlags {
                started: true,
                completed: false,
            },
            peaceful_turns: Some(next()),
            kills: None,
            conversions: Some(next()),
            explored_tiles: Some(next()),
        });
        p.unlocked_tech_cache = Some(p.available_tech);
    }
    let city = g
        .tiles
        .iter()
        .position(|t| t.improvement.is_some())
        .unwrap();
    let m = g.tile_mut(city).improvement.as_mut().unwrap();
    for kind in [1, 4, 4, 9, 13] {
        m.rewards.push(kind);
    }
    m.effects.clear();
    // improvement effects are bytes in the block
    m.effects.push((next() % 120) as i32);
    g.triggers = vec![engine_fast::state::Trigger {
        player: 2,
        opponent: 3,
        kind: next(),
        coordinates: engine_fast::state::Xy { x: 4, y: 5 },
    }];
    let block = device::to_device(&g).unwrap();
    assert!(device::from_device(&block, &g).unwrap() == device::canonical(&g).unwrap());
    assert_eq!(dump::of_block(&block), dump::of_game(&g));
}

#[test]
fn a_game_too_large_for_the_block_is_refused() {
    let mut last = None;
    play(11, 2, 1, 5, |g| last = Some(g.clone()));
    let mut g = last.unwrap();
    g.triggers = vec![
        g.triggers
            .first()
            .copied()
            .unwrap_or(engine_fast::state::Trigger {
                player: 1,
                opponent: 0,
                kind: 0,
                coordinates: engine_fast::state::Xy { x: 0, y: 0 },
            });
        device::MAX_TRIGGERS + 1
    ];
    assert!(device::to_device(&g).is_err());
}

#[cfg(feature = "cuda")]
#[test]
fn the_gpu_holds_the_same_block() {
    let side = GAMES.iter().map(|g| g.0).max().unwrap() as usize;
    let cap = device::default_caps(side * side);
    let mut blocks = Vec::new();
    let mut probes = Vec::new();
    for (size, seats, seed) in GAMES {
        play(size, seats, seed, 400, |g| {
            if g.decisions % 8 == 0 {
                // one call holds blocks of one capacity: the largest board's, so the smaller
                // boards leave room unused
                blocks.push(device::to_device_with(g, cap.0, cap.1).unwrap());
                probes.push(probe(g));
            }
        });
    }
    let (back, digests, gpu_probes) = device::device_roundtrip(&blocks).unwrap();
    let size = blocks[0].bytes().len();
    for (i, block) in blocks.iter().enumerate() {
        assert!(
            back[i].bytes() == block.bytes(),
            "block {i} came back changed"
        );
        assert_eq!(
            digests[i],
            device::twin_digest(block),
            "block {i}: the GPU's digest differs"
        );
        assert_eq!(
            gpu_probes[i], probes[i],
            "block {i}: the GPU reads other field values"
        );
    }
    println!("{} blocks of {size} bytes equal on the GPU", blocks.len());
}

/// The narrowed fields of the block refuse a value that does not fit, at
/// the bound and not before: each field takes its largest value and is refused one past it.
#[test]
fn narrowed_fields_refuse_what_does_not_fit() {
    let t = Tables::compile(CATALOG).unwrap();
    let config = Config {
        rules: 124,
        seed: 51,
        size: 11,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit: 10_000,
    };
    let g = reset::reset(&config, CATALOG, &t).unwrap();
    let city = g
        .tiles
        .iter()
        .position(|x| x.improvement.is_some())
        .unwrap();
    type Set = fn(&mut Game, usize, i64);
    let cases: [(&str, i64, Set); 8] = [
        ("level", 65_535, |g, i, v| {
            g.tile_mut(i).improvement.as_mut().unwrap().level = v as i32
        }),
        ("founding turn", 32_767, |g, i, v| {
            g.tile_mut(i).improvement.as_mut().unwrap().founded = v as i32
        }),
        ("production", 65_535, |g, i, v| {
            g.tile_mut(i).improvement.as_mut().unwrap().production = v as i32
        }),
        ("border size", 127, |g, i, v| {
            g.tile_mut(i).improvement.as_mut().unwrap().border_size = v as i32
        }),
        ("tile owner", 255, |g, i, v| g.tile_mut(i).owner = v as i32),
        ("terrain", 127, |g, i, v| g.tile_mut(i).terrain = v as i32),
        ("continent", 32_767, |g, i, v| {
            g.tile_mut(i).continent = Some(v as i32)
        }),
        ("tile skin", 127, |g, i, v| g.tile_mut(i).skin = v as i32),
    ];
    for (what, max, set) in cases {
        let mut ok = g.clone();
        set(&mut ok, city, max);
        assert!(device::to_device(&ok).is_ok(), "{what} at {max}");
        let mut over = g.clone();
        set(&mut over, city, max + 1);
        assert_eq!(
            device::to_device(&over).err(),
            Some(format!("{what} {} is outside the device's range", max + 1)),
            "{what}"
        );
    } // a unit on the board
    let node = g.tiles.iter().find_map(|x| x.unit).unwrap() as usize;
    type SetUnit = fn(&mut Game, usize, i64);
    let units: [(&str, i64, SetUnit); 5] = [
        ("health", 65_535, |g, n, v| g.unit_mut(n).health = v as i32),
        ("unit xp", 32_767, |g, n, v| g.unit_mut(n).xp = v as i32),
        ("unit owner", 255, |g, n, v| g.unit_mut(n).owner = v as i32),
        ("promotion level", 127, |g, n, v| {
            g.unit_mut(n).promotion_level = v as i32
        }),
        ("creation turn", 32_767, |g, n, v| {
            g.unit_mut(n).created_turn = v as i32
        }),
    ];
    for (what, max, set) in units {
        let mut ok = g.clone();
        set(&mut ok, node, max);
        assert!(device::to_device(&ok).is_ok(), "{what} at {max}");
        let mut over = g.clone();
        set(&mut over, node, max + 1);
        assert_eq!(
            device::to_device(&over).err(),
            Some(format!("{what} {} is outside the device's range", max + 1)),
            "{what}"
        );
    }
}

/// A block lives in the capacities it was made with: a header changed to describe a larger
/// block is refused before the C side reads past the buffer, and one changed to a smaller
/// block fails the check (the layout would no longer match the room).
#[test]
#[should_panic(expected = "a block's capacities describe its own buffer")]
fn a_block_claiming_more_room_is_refused() {
    let mut last = None;
    play(11, 2, 1, 10, |g| last = Some(g.clone()));
    let mut block = device::to_device(&last.unwrap()).unwrap();
    block.unit_cap += 1;
    device::twin_digest(&block);
}

#[test]
fn a_block_with_unused_room_round_trips() {
    let mut last = None;
    play(11, 2, 1, 60, |g| last = Some(g.clone()));
    let g = last.unwrap();
    let want = device::canonical(&g).unwrap();
    let (tiles, units) = device::default_caps(device::MAX_TILES);
    let wide = device::to_device_with(&g, tiles, units).unwrap();
    assert_eq!(wide.bytes().len(), device::block_size(tiles, units));
    let back = device::from_device(&wide, &g).unwrap();
    assert!(device::canonical(&back).unwrap() == want);
    assert_eq!(
        device::twin_probe(&wide),
        device::twin_probe(&device::to_device(&g).unwrap())
    );
    // a board larger than the capacities, and more units than they hold, are refused
    assert!(device::to_device_with(&g, 120, units).is_err());
    assert!(device::to_device_with(&g, tiles, 1).is_err());
}

/// A header whose tile count passes its capacity is refused before the unchecked probe walks
/// the tiles, and from_device refuses it as an error.
#[test]
#[should_panic(expected = "a block's tiles fit its capacity")]
fn a_block_with_more_tiles_than_room_is_refused() {
    let mut last = None;
    play(11, 2, 1, 10, |g| last = Some(g.clone()));
    let g = last.unwrap();
    let mut block = device::to_device(&g).unwrap();
    block.tile_count = block.tile_cap + 1;
    assert!(device::from_device(&block, &g).is_err());
    device::twin_probe(&block);
}
