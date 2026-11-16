//! The legal list of the kernel sources against the batch's.
//!
//! Random games are played through `engine_fast_batch::Batch`, whose sorted rows are the
//! interface's legal moves. At every decision the game goes to the host build as a block and
//! its rows must equal the batch's, field for field and in order; where the engine's menu
//! stops, the kernel must stop with the same text. Every move kind the games offer is
//! counted, and the families that must have been compared are checked.
//!
//! Not covered: what random two-to-four-seat games do not reach (special tribes' abilities,
//! the final turn); random play is a smoke check, not verification.

use engine_fast::menu;
use engine_fast::reset::Config;
use engine_fast::tables::Tables;
use engine_fast_batch::{Applied, Batch, Status};
use engine_fast_gpu::menu::twin_menu;
use engine_fast_gpu::{state as device, tables};
use std::collections::BTreeMap;

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
const END_TURN: i32 = 15;
/// size, tribes, seed
const GAMES: [(i32, &[i32], i32); 10] = [
    (11, &[7, 7], 41),
    (11, &[7, 7, 7, 7], 42),
    (14, &[7, 7], 43),
    (14, &[7, 7, 7], 44),
    (16, &[7, 7], 45),
    (16, &[7, 7, 7, 7], 46),
    (11, &[2, 4], 47),
    (14, &[6, 9, 12], 48),
    // larger boards; random play, a smoke test
    (20, &[7, 7, 7], 49),
    (30, &[2, 4, 6, 9], 50),
];

#[derive(Default)]
struct Tally {
    /// the blocks compared, kept for the GPU check
    blocks: Vec<engine_fast_gpu::state::Block>,
    compared: usize,
    rows: usize,
    errors: usize,
    kinds: BTreeMap<i32, usize>,
    longest: usize,
}

fn play(
    t: &Tables,
    flat: &tables::DeviceTables,
    size: i32,
    tribes: &[i32],
    seed: i32,
    decisions: usize,
    tally: &mut Tally,
) {
    let mut batch = Batch::new(CATALOG, 1, 1).unwrap();
    let config = Config {
        rules: 124,
        seed,
        size,
        map: 2,
        mode: 4,
        tribes: tribes.to_vec(),
        action_limit: 10_000,
    };
    batch.reset(&[(0, config)]).unwrap();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64 ^ seed as u64;
    for decision in 0..decisions {
        let Some(g) = batch.game(0) else { return };
        let at = format!("size {size}, tribes {tribes:?}, seed {seed}, decision {decision}");
        let block = device::to_device(g).unwrap();
        let ours = twin_menu(&block, flat);
        if decision % 4 == 0 {
            // the GPU check packs blocks of one capacity: the largest board's
            let (tiles, units) = device::default_caps(device::MAX_TILES);
            tally
                .blocks
                .push(device::to_device_with(g, tiles, units).unwrap());
        }
        match batch.status(0) {
            Status::Playing => {
                let want = batch.moves(0);
                let got =
                    ours.unwrap_or_else(|e| panic!("{at}: only the port stopped: {}", e.text));
                if got != want {
                    let first = got.iter().zip(want).position(|(a, b)| a != b);
                    panic!(
                        "{at}: the rows differ at {first:?} ({} / {}): {:?} / {:?}",
                        got.len(),
                        want.len(),
                        first.map(|k| got[k]),
                        first.map(|k| want[k])
                    );
                }
                tally.compared += 1;
                tally.rows += want.len();
                tally.longest = tally.longest.max(want.len());
                for r in want {
                    *tally.kinds.entry(r[0]).or_default() += 1;
                }
            }
            _ => {
                // the game stopped: where the engine's menu stops, the port must give its text
                if let Err(e) = menu::menu(g, t) {
                    let stop =
                        ours.expect_err(&format!("{at}: only the engine's menu stopped: {e}"));
                    assert_eq!(stop.text, e, "{at}: the errors differ");
                    tally.errors += 1;
                }
                return;
            }
        }
        let moves = batch.moves(0);
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = (rng >> 33) as usize;
        let others: Vec<usize> = (0..moves.len())
            .filter(|&i| moves[i][0] != END_TURN && moves[i][0] != 35)
            .collect();
        let end = moves.iter().position(|m| m[0] == END_TURN);
        let pick = match end {
            Some(e) if others.is_empty() || r % 10 == 0 => e,
            _ if !others.is_empty() => others[(r / 10) % others.len()],
            _ => return,
        };
        let mut applied = [Applied::Skipped];
        batch.step(&[pick as i32], &mut applied).unwrap();
    }
}

#[test]
fn the_host_legal_list_equals_the_batchs() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut tally = Tally::default();
    for (size, tribes, seed) in GAMES {
        play(&t, &flat, size, tribes, seed, 400, &mut tally);
    }
    println!(
        "compared {} lists ({} rows, longest {}), {} menu stops; rows by kind {:?}",
        tally.compared, tally.rows, tally.longest, tally.errors, tally.kinds
    );
    // the families random play must have reached; not reached: HealOthers (4), Destroy (9),
    // Promote (13), EndMatch (22), Decompose (26), Disembark (36) (the rich games below reach
    // 9, 26 and 36)
    for kind in [1, 2, 3, 5, 6, 7, 8, 10, 11, 14, 15, 16, 27, 28, 29, 30, 35] {
        assert!(
            tally.kinds.contains_key(&kind),
            "no row of kind {kind} was compared"
        );
    }
    assert_eq!(
        engine_fast_gpu::step::scratch_overlaps(),
        0,
        "two search scratches held at once"
    );
}

/// The comparison can fail: a block whose mover has no stars offers fewer rows.
#[test]
fn a_changed_block_gives_other_rows() {
    let mut batch = Batch::new(CATALOG, 1, 1).unwrap();
    let config = Config {
        rules: 124,
        seed: 43,
        size: 14,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit: 10_000,
    };
    batch.reset(&[(0, config)]).unwrap();
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut block = device::to_device(batch.game(0).unwrap()).unwrap();
    assert_eq!(twin_menu(&block, &flat).unwrap(), batch.moves(0));
    let mover = block.current_player_index as usize;
    block.players[mover].currency = 0;
    assert_ne!(twin_menu(&block, &flat).unwrap(), batch.moves(0));
}

/// The GPU's legal list equals the host build's on blocks of the same games, row for row,
/// the stops included.
#[cfg(feature = "cuda")]
#[test]
fn the_gpu_legal_list_equals_the_host_builds() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut tally = Tally::default();
    for (size, tribes, seed) in GAMES {
        play(&t, &flat, size, tribes, seed, 400, &mut tally);
    }
    let gpu = engine_fast_gpu::menu::device_menus(&tally.blocks, &flat).unwrap();
    for (k, (block, (result, rows))) in tally.blocks.iter().zip(&gpu).enumerate() {
        match twin_menu(block, &flat) {
            Ok(want) => assert!(result.code == 0 && rows == &want, "block {k}"),
            Err(stop) => assert_eq!(
                (result.code, result.arg),
                (stop.code, stop.arg),
                "block {k}"
            ),
        }
    }
    println!("{} blocks equal on the GPU", gpu.len());
}

use engine_fast_batch::{as_move as row, sort_key};

/// Random games in which the player to move knows every technology and has 900 stars at every
/// decision, so the families plain random play never offers are made too; the rows are
/// compared with the engine's menu, converted and sorted as the batch does, and every step
/// with `step::advance` (cloaks' infiltration included).
#[test]
fn a_rich_players_list_equals_the_engines() {
    use engine_fast::{reset, step};
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut kinds: BTreeMap<i32, usize> = BTreeMap::new();
    let (mut compared, mut stops, mut steps, mut infiltrations) = (0, 0, 0, 0);
    // the techs of the standard tribes: their unlocks resolve and include no flooding or filling
    // improvement and no unit that moves on wetland or has a special tribe's ability (rules, which the engine
    // refuses)
    let everything: Vec<i32> = (0..t.techs.len())
        .filter(|&k| {
            t.techs.get(k).is_some_and(|x| {
                x.unit_unlocks.iter().all(|n| {
                    t.units.id(n).is_ok_and(|u| {
                        let d = t.units.get(u).unwrap();
                        let special = ["skate", "fly", "explode", "swarm", "freezearea", "boost"];
                        !d.movement_terrain.iter().any(|m| t.terrain.id(m) == Ok(7))
                            && !special.iter().any(|a| d.unit_abilities.has(a))
                    })
                }) && x.improvement_unlocks.iter().all(|n| {
                    t.improvements.id(n).is_ok_and(|i| {
                        let a = &t.improvements.get(i).unwrap().improvement_abilities;
                        !a.has("flood") && !a.has("fill")
                    })
                })
            })
        })
        .map(|k| k as i32)
        .collect();
    for (size, tribes, seed) in GAMES {
        let config = Config {
            rules: 124,
            seed,
            size,
            map: 2,
            mode: 4,
            tribes: tribes.to_vec(),
            action_limit: 10_000,
        };
        let mut g = reset::reset(&config, CATALOG, &t).unwrap();
        let mut rng = 0x51_7CC1_B727_220Au64 ^ seed as u64;
        for decision in 0..600 {
            let mover = g.current_player_index as usize;
            g.players[mover].available_tech = everything.iter().copied().collect();
            g.players[mover].action.currency = 900;
            let at = format!("size {size}, seed {seed}, decision {decision}");
            let block = device::to_device(&g).unwrap();
            let ours = twin_menu(&block, &flat);
            let legal = match menu::menu(&g, &t) {
                Ok(legal) => legal,
                Err(e) => {
                    let stop = ours.expect_err(&format!("{at}: only the engine stopped: {e}"));
                    assert_eq!(stop.text, e, "{at}");
                    println!("{at}: both stop: {e}");
                    stops += 1;
                    break;
                }
            };
            let mut want: Vec<[i32; 6]> = legal.iter().map(|c| row(g.width, c)).collect();
            want.sort_by_key(sort_key);
            let got = ours.unwrap_or_else(|e| panic!("{at}: only the port stopped: {}", e.text));
            assert_eq!(got, want, "{at}");
            compared += 1;
            for r in &want {
                *kinds.entry(r[0]).or_default() += 1;
            }
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = (rng >> 33) as usize;
            // a random move the engine's step takes (it refuses some rules the list offers)
            let others: Vec<usize> = (0..legal.len())
                .filter(|&i| ![END_TURN, 35].contains(&legal[i].action.kind))
                .collect();
            let end = legal.iter().position(|c| c.action.kind == END_TURN);
            let mut order: Vec<usize> = match end {
                Some(e) if others.is_empty() || r % 8 == 0 => vec![e],
                _ => (0..others.len())
                    .map(|k| others[(r / 8 + k) % others.len()])
                    .collect(),
            };
            order.extend(end);
            let next = order.iter().find_map(|&k| {
                let mut copy = g.clone();
                step::advance(&mut copy, &t, &legal[k].action)
                    .ok()
                    .map(|_| (k, copy))
            });
            let Some((k, next)) = next else {
                println!("{at}: the step takes no move");
                break;
            };
            // the port's step on the same state and move must give the engine's state
            let action = &legal[k].action;
            let mut block = device::to_device(&g).unwrap();
            engine_fast_gpu::step::twin_advance(&mut block, &flat, action).unwrap_or_else(|e| {
                panic!("{at}: only the port stopped on {action:?}: {}", e.text)
            });
            let (want, got) = (
                device::canonical(&next).unwrap(),
                device::from_device(&block, &g).and_then(|x| device::canonical(&x)),
            );
            let mut got = got.unwrap();
            got.side = want.side.clone();
            got.memory = want.memory.clone();
            assert!(got == want, "{at}: the states differ after {action:?}");
            steps += 1;
            let unit = action
                .source
                .and_then(|c| g.tile(engine_fast::state::Xy { x: c.x, y: c.y })?.unit);
            if action.kind == 2
                && unit.is_some_and(|n| {
                    t.units
                        .get(g.units[n as usize].kind as usize)
                        .is_some_and(|d| d.unit_abilities.has("infiltrate"))
                })
            {
                infiltrations += 1;
            }
            g = next;
        }
    }
    println!(
        "compared {compared} rich lists and {steps} steps ({infiltrations} infiltrations), {stops} menu stops; rows by kind {kinds:?}"
    );
    for kind in [9, 26] {
        assert!(
            kinds.contains_key(&kind),
            "no row of kind {kind} was compared"
        );
    }
}

/// The game's Destroy check leaves out kinds up to 47 outside its bitset, the lighthouse among
/// them: an owned lighthouse offers no Destroy, in the engine and in the port.
#[test]
fn an_owned_lighthouse_offers_no_destroy() {
    use engine_fast::reset;
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let config = Config {
        rules: 124,
        seed: 41,
        size: 11,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit: 10_000,
    };
    let mut g = reset::reset(&config, CATALOG, &t).unwrap();
    let mover = g.current_player_index as usize;
    let id = g.players[mover].action.id;
    let lighthouse = t.improvements.id("lighthouse").unwrap() as i32;
    let i = g
        .tiles
        .iter()
        .position(|x| x.improvement.as_ref().is_some_and(|m| m.kind == lighthouse))
        .expect("a lighthouse");
    g.tiles[i].owner = id;
    g.tiles[i].explorers.push(id);
    let destroy = t.techs.id("chivalry").unwrap() as i32;
    g.players[mover].available_tech.push(destroy);
    let want: Vec<[i32; 6]> = menu::menu(&g, &t)
        .unwrap()
        .iter()
        .map(|c| row(g.width, c))
        .collect();
    let at = g.tiles[i].coordinates;
    let here = at.y * g.width + at.x;
    assert!(!want.iter().any(|r| r[0] == 9 && r[2] == here));
    let mut sorted = want.clone();
    sorted.sort_by_key(sort_key);
    assert_eq!(
        twin_menu(&device::to_device(&g).unwrap(), &flat).unwrap(),
        sorted
    );
}

/// The recorded-game check (`check::check`) on a game played here: every command compares,
/// and a changed list is reported as a difference.
#[test]
fn a_played_game_checks_exactly() {
    use engine_fast::{reset, step};
    use engine_fast_gpu::check::{check, Outcome};
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let config = Config {
        rules: 124,
        seed: 44,
        size: 14,
        map: 2,
        mode: 4,
        tribes: vec![7, 7, 7],
        action_limit: 10_000,
    };
    let start = reset::reset(&config, CATALOG, &t).unwrap();
    // the commands of a random game, as a recording would hold them
    let mut g = start.clone();
    let mut commands = Vec::new();
    let mut rng = 44u64;
    for _ in 0..300 {
        let legal = menu::menu(&g, &t).unwrap();
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let a = legal[(rng >> 33) as usize % legal.len()].action.clone();
        if a.kind == 35 {
            continue;
        }
        let mut copy = g.clone();
        if step::command(&mut copy, &t, &a, |_| {}).is_ok() {
            g = copy;
            commands.push(a);
        }
    }
    let report = check(start.clone(), &t, &flat, &commands, &[], 0);
    println!("{}", report.to_json());
    assert_eq!(report.outcome, Some(Outcome::Exact));
    assert_eq!(report.compared, commands.len());
    // a different starting state is caught at the first command
    let mut other = start;
    let mover = other.current_player_index as usize;
    other.players[mover].action.currency += 7;
    let mut lie = device::to_device(&other).unwrap();
    lie.players[mover].currency -= 7;
    assert_ne!(
        twin_menu(&lie, &flat).ok(),
        engine_fast_gpu::check::engine_rows(&other, &t).ok()
    );
}

/// A batch slot on the host build (`kernel/status.h`: the row's move, then the status, winner
/// and legal list) against the batch's, stepping both by the same row indices; the block must
/// equal the batch's game after every step.
#[test]
fn the_host_slot_equals_the_batchs() {
    use engine_fast_gpu::menu::TwinSlot;
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut statuses: BTreeMap<u8, usize> = BTreeMap::new();
    for (size, tribes, seed) in GAMES {
        let mut batch = Batch::new(CATALOG, 1, 1).unwrap();
        let config = Config {
            rules: 124,
            seed,
            size,
            map: 2,
            mode: 4,
            tribes: tribes.to_vec(),
            action_limit: 120,
        };
        batch.reset(&[(0, config)]).unwrap();
        let mut slot = TwinSlot::default();
        // a configuration the engine refuses at the start has no game to give the port
        let Some(g) = batch.game(0) else {
            println!(
                "tribes {tribes:?}: refused at the start: {}",
                batch.reason(0)
            );
            continue;
        };
        let mut block = device::to_device(g).unwrap();
        let mut ours = slot.run(&mut block, &flat, None);
        let mut rng = seed as u64;
        for decision in 0..400 {
            let at = format!("size {size}, seed {seed}, decision {decision}");
            let status = batch.status(0);
            *statuses.entry(status as u8).or_default() += 1;
            match status {
                Status::Playing | Status::Ended | Status::Truncated => {
                    let (s, w) = ours.clone().unwrap_or_else(|e| panic!("{at}: {}", e.text));
                    assert_eq!((s, w), (status as i32, batch.winner(0)), "{at}");
                    assert_eq!(slot.rows(), batch.moves(0), "{at}");
                    if status == Status::Playing {
                        assert_eq!(
                            Some(&slot.view().unwrap()),
                            batch.view(0),
                            "{at}: the views differ"
                        );
                    }
                    let g = batch.game(0).unwrap();
                    let got = device::from_device(&block, g).unwrap();
                    let (mut got, want) = (
                        device::canonical(&got).unwrap(),
                        device::canonical(g).unwrap(),
                    );
                    got.side = want.side.clone();
                    got.memory = want.memory.clone();
                    assert!(got == want, "{at}: the states differ");
                }
                Status::Refused => {
                    let e = ours.clone().expect_err(&at);
                    assert_eq!(e.text, batch.reason(0), "{at}");
                }
                Status::Failed => assert!(ours.clone().is_err_and(|e| e.failed), "{at}"),
                Status::Empty => panic!("{at}: empty"),
            }
            if status != Status::Playing {
                break;
            }
            let moves = batch.moves(0);
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = (rng >> 33) as usize;
            let pick = if r % 10 == 0 {
                moves.iter().position(|m| m[0] == END_TURN).unwrap_or(0)
            } else {
                (r / 10) % moves.len()
            };
            let mut applied = [Applied::Skipped];
            batch.step(&[pick as i32], &mut applied).unwrap();
            ours = slot.run(&mut block, &flat, Some(pick));
        }
    }
    println!("statuses seen {statuses:?}");
    for s in [Status::Playing, Status::Ended, Status::Truncated] {
        assert!(statuses.contains_key(&(s as u8)), "no slot was {s:?}");
    }
}

/// A cloak next to an enemy city infiltrates it (attack.rs Infiltrate, infiltrate.rs
/// infiltrate and reward: the cloak dies, the city's production as stars, daggers placed by
/// rebellion weight, the battle with the city's unit): the port's step equals the engine's,
/// for each enemy city of several games and with and without a defender.
#[test]
fn a_cloak_infiltrates_as_in_the_engine() {
    use engine_fast::state::Xy;
    use engine_fast::{reset, step};
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let cloak = t.units.id("cloak").unwrap() as i32;
    let mut infiltrations = 0;
    for seed in 60..72 {
        let config = Config {
            rules: 124,
            seed,
            size: [11, 14, 16][seed as usize % 3],
            map: 2,
            mode: 4,
            tribes: vec![7; 2 + seed as usize % 3],
            action_limit: 10_000,
        };
        let start = reset::reset(&config, CATALOG, &t).unwrap();
        let mover = start.current_player_index as usize;
        let me = start.players[mover].action.id;
        for other in start
            .players
            .iter()
            .filter(|p| p.action.id != me && p.action.id != 255)
        {
            for defended in [false, true] {
                let mut g = start.clone();
                let city = other.start_tile;
                // a free neighbour of the capital on land
                let Some(spot) = (0..8).find_map(|k| {
                    let (dx, dy) = [
                        (1, 0),
                        (-1, 0),
                        (0, 1),
                        (0, -1),
                        (1, 1),
                        (-1, -1),
                        (1, -1),
                        (-1, 1),
                    ][k];
                    let c = Xy {
                        x: city.x + dx,
                        y: city.y + dy,
                    };
                    g.tile(c)
                        .filter(|x| x.unit.is_none() && (3..=6).contains(&x.terrain))
                        .map(|_| c)
                }) else {
                    continue;
                };
                let template = g.units
                    [g.tile(g.players[mover].start_tile).unwrap().unit.unwrap() as usize]
                    .clone();
                let mut u = template.clone();
                u.id = 9_000 + seed as u32;
                u.kind = cloak;
                u.coordinates = spot;
                u.health = t.units.get(cloak as usize).unwrap().health;
                u.moved = false;
                u.attacked = false;
                g.units.push(u);
                let n = (g.units.len() - 1) as u32;
                let i = g.index(spot).unwrap();
                g.tiles[i].unit = Some(n);
                for c in [spot, city] {
                    let j = g.index(c).unwrap();
                    if !g.tiles[j].explorers.contains(&me) {
                        g.tiles[j].explorers.push(me);
                    }
                }
                if !defended {
                    let j = g.index(city).unwrap();
                    g.tiles[j].unit = None;
                }
                let legal = menu::menu(&g, &t).unwrap();
                let at = |c: Xy| Some(engine_fast_frame::Xy { x: c.x, y: c.y });
                let Some(attack) = legal.iter().find(|c| {
                    c.action.kind == 2 && c.action.source == at(spot) && c.action.target == at(city)
                }) else {
                    continue;
                };
                let mut want = g.clone();
                let rust = step::advance(&mut want, &t, &attack.action);
                let mut block = device::to_device(&g).unwrap();
                let ours = engine_fast_gpu::step::twin_advance(&mut block, &flat, &attack.action);
                match (rust, ours) {
                    (Ok(a), Ok(b)) => {
                        assert_eq!(a, b);
                        let want = device::canonical(&want).unwrap();
                        let mut got =
                            device::canonical(&device::from_device(&block, &g).unwrap()).unwrap();
                        got.side = want.side.clone();
                        got.memory = want.memory.clone();
                        assert!(
                            got == want,
                            "seed {seed}, defended {defended}: the states differ"
                        );
                        infiltrations += 1;
                    }
                    (Err(e), Err(s)) => assert_eq!(s.text, e, "seed {seed}"),
                    (r, o) => panic!(
                        "seed {seed}, defended {defended}: {r:?} / {:?}",
                        o.map_err(|s| s.text)
                    ),
                }
            }
        }
    }
    println!("{infiltrations} infiltrations compared");
    assert!(
        infiltrations >= 10,
        "only {infiltrations} infiltrations compared"
    );
}

/// The attack previews and the defence plane. The defence
/// plane equals `Batch::defence` at every decision; the host build's preview of each
/// legal row equals the CPU batch's (`Batch::previews`), row for row, over random games long
/// enough for armies to meet, Attack rows with and without retaliation and lethal ones among
/// them. Random play: a smoke test.
#[test]
fn the_host_previews_and_defence_equal_the_batchs() {
    let flat = tables::DeviceTables::new(&Tables::compile(CATALOG).unwrap()).unwrap();
    let (mut lists, mut present, mut kills, mut back) = (0, 0, 0, 0);
    let (mut panels, mut bonus) = (0, 0);
    for (size, tribes, seed) in [
        (11, &[7, 7][..], 61),
        (11, &[2, 4, 7, 7][..], 62),
        (16, &[2, 4, 7][..], 63),
    ] {
        let mut batch = Batch::new(CATALOG, 1, 1).unwrap();
        let config = Config {
            rules: 124,
            seed,
            size,
            map: 2,
            mode: 4,
            tribes: tribes.to_vec(),
            action_limit: 10_000,
        };
        batch.reset(&[(0, config)]).unwrap();
        let mut rng = 0x2545_F491_4F6C_DD1Du64 ^ seed as u64;
        for decision in 0..1_500 {
            if batch.status(0) != Status::Playing {
                break;
            }
            let g = batch.game(0).unwrap();
            let block = device::to_device(g).unwrap();
            let (rows, previews) = engine_fast_gpu::menu::twin_menu_previews(&block, &flat)
                .unwrap_or_else(|e| panic!("{}", e.text));
            assert_eq!(
                rows,
                batch.moves(0),
                "seed {seed}, decision {decision}: rows"
            );
            assert_eq!(
                previews,
                batch.previews(0),
                "seed {seed}, decision {decision}: previews"
            );
            let defence = engine_fast_gpu::outputs::twin_defence(&block, &flat)
                .unwrap_or_else(|e| panic!("{}", e.text));
            assert_eq!(
                Some(defence.clone()),
                batch.defence(0).unwrap(),
                "seed {seed}, decision {decision}: defence"
            );
            bonus += defence.iter().filter(|&&d| d > 10).count();
            panels += defence.iter().filter(|&&d| d >= 0).count();
            lists += 1;
            present += previews.iter().filter(|p| p[0] == 1).count();
            kills += previews.iter().filter(|p| p[3] & 1 != 0).count();
            back += previews.iter().filter(|p| p[3] & 4 != 0).count();
            // attack whenever an attack is legal, so that armies meet and fight
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = (rng >> 33) as usize;
            let moves = batch.moves(0);
            let attacks: Vec<usize> = (0..moves.len()).filter(|&i| moves[i][0] == 2).collect();
            let others: Vec<usize> = (0..moves.len()).filter(|&i| moves[i][0] != 35).collect();
            let pick = if !attacks.is_empty() && r % 2 == 0 {
                attacks[r / 2 % attacks.len()]
            } else {
                others[r / 2 % others.len()]
            };
            let mut applied = [Applied::Skipped];
            batch.step(&[pick as i32], &mut applied).unwrap();
        }
    }
    println!("{lists} lists, {present} previews ({kills} lethal, {back} with retaliation), {panels} defence panels ({bonus} above 10) equal");
    assert!(
        panels > 1_000 && bonus > 0,
        "{panels} defence panels, {bonus} with a bonus"
    );
    assert!(
        present > 100 && kills > 0 && back > 0,
        "{present} previews, {kills} lethal, {back} retaliated"
    );
}
