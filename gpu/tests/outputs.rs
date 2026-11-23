//! The batch's optional outputs on the host build of the kernel sources (`kernel/outputs.h`)
//! equal the CPU batch's (`engine_fast_batch::outputs`) for every seat, not only the player to
//! move, at every decision of random games on boards of 8, 11 and 16, the special tribes
//! among them; and a unit the viewer cannot see counts as absent in the masked outputs
//! (`queries::as_shown`), as on the CPU. The device's copies are `device_loop`'s.

use engine_fast::queries;
use engine_fast::reset::{self, Config};
use engine_fast::state::{Game, Xy};
use engine_fast::tables::Tables;
use engine_fast::view::{self, View};
use engine_fast::{menu, step};
use engine_fast_batch::outputs::{city_score_plane, progression_row, Plane, PROGRESSION_LEN};
use engine_fast_gpu::outputs::{twin_progression, twin_queries};
use engine_fast_gpu::{state as device, tables};

const CATALOG: &str = include_str!("../../rules/rules-124.json");
const END_TURN: i32 = 15;
const RESIGN: i32 = 35;
const NATURE: i32 = 255;

/// Play up to `decisions` random decisions and hand every state between decisions to `each`.
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
        panic!("seed {seed}: the reset is refused");
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

fn seats(g: &Game) -> Vec<i32> {
    g.players
        .iter()
        .map(|p| p.action.id)
        .filter(|&id| id != NATURE)
        .collect()
}

/// The host build's outputs of `g` for `viewer` equal the CPU's; `Err` names the first that
/// differs.
fn compare(g: &Game, t: &Tables, flat: &tables::DeviceTables, viewer: i32) -> Result<(), String> {
    let block = device::to_device(g).unwrap();
    let got = twin_queries(&block, flat, viewer)?;
    for p in Plane::ALL {
        let want = p.of(g, t, viewer)?;
        if got.plane(p) != want.as_slice() {
            return Err(format!("{p:?}"));
        }
    }
    if got.income != queries::income(g, t, viewer)? {
        return Err("income".into());
    }
    if got.city_score != city_score_plane(g, t, viewer) {
        return Err("city_score".into());
    }
    let mut v = View::default();
    view::write(g, t, viewer, &mut v)?;
    let mut want = vec![0; PROGRESSION_LEN];
    progression_row(g, &v, &mut want)?;
    if twin_progression(&block, flat, viewer)? != want {
        return Err("progression".into());
    }
    Ok(())
}

#[test]
fn the_host_outputs_equal_the_cpu_batchs_for_every_seat() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let special: Vec<i32> = ["aquarion", "polaris", "elyrion", "cymanti"]
        .map(|n| t.tribes.id_of[n] as i32)
        .to_vec();
    let mut games = vec![
        (8, vec![7, 7], 41),
        (11, vec![2, 4, 7, 7], 42),
        (11, vec![7, 7, 7], 43),
        (16, vec![2, 4, 7], 44),
    ];
    games.push((11, special.clone(), 45));
    games.push((16, vec![special[0], special[3]], 46));
    let mut compared = 0;
    // entries above zero, so a plane the port left at zero or -1 would show
    let mut positive = [0usize; 8];
    let (mut paying, mut scored, mut embassies) = (0, 0, 0);
    for (size, tribes, seed) in games {
        play(&t, size, tribes, seed, 500, |g| {
            for viewer in seats(g) {
                if let Err(e) = compare(g, &t, &flat, viewer) {
                    panic!("seed {seed}, turn {}, viewer {viewer}: {e}", g.current_turn);
                }
                for p in Plane::ALL {
                    let plane = p.of(g, &t, viewer).unwrap();
                    positive[p as usize] += plane.iter().filter(|&&v| v > 0).count();
                }
                paying += usize::from(queries::income(g, &t, viewer).unwrap() > 0);
                scored += city_score_plane(g, &t, viewer)
                    .iter()
                    .filter(|&&s| s > 0)
                    .count();
                let mut v = View::default();
                view::write(g, &t, viewer, &mut v).unwrap();
                let mut row = vec![0; PROGRESSION_LEN];
                progression_row(g, &v, &mut row).unwrap();
                embassies += row[PROGRESSION_LEN - 8..]
                    .iter()
                    .filter(|&&e| e > 0)
                    .count();
                compared += 1;
            }
        });
    }
    println!("{compared} views compared; above zero by plane {positive:?}; {paying} paying, {scored} city scores, {embassies} embassies");
    for p in Plane::ALL {
        // no unit is boosted in random play: `a_boosted_unit_shows_its_panel` boosts some
        if p != Plane::AttackModifier {
            assert!(positive[p as usize] > 0, "{p:?} never above zero");
        }
    }
    assert!(paying > 0 && scored > 0 && embassies > 0);
}

/// Every unit given an effect list in turn (boosted; boosted and poisoned; Swift; Bubble): the
/// panel planes of the host build equal the CPU's for every seat, and the attack modifier and
/// movement show the effects.
#[test]
fn a_boosted_unit_shows_its_panel() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut last = None;
    play(&t, 11, vec![7, 7, 7], 47, 400, |g| last = Some(g.clone()));
    let g = last.unwrap();
    let (mut boosted, mut faster) = (0, 0);
    for effects in [&[2][..], &[2, 1], &[6], &[4]] {
        let mut h = g.clone();
        let mut list = engine_fast::inline::Inline::<i64, 9>::new();
        for &e in effects {
            list.push(e);
        }
        let list = h.push_effects(list);
        for n in 0..h.units.len() {
            h.unit_mut(n).effects = list;
        }
        for viewer in seats(&h) {
            compare(&h, &t, &flat, viewer).unwrap_or_else(|e| panic!("{effects:?}: {e}"));
            let attack = Plane::AttackModifier.of(&h, &t, viewer).unwrap();
            boosted += attack.iter().filter(|&&a| a == 50).count();
            let now = Plane::Movement.of(&h, &t, viewer).unwrap();
            let before = Plane::Movement.of(&g, &t, viewer).unwrap();
            faster += now.iter().zip(&before).filter(|(a, b)| a > b).count();
        }
    }
    assert!(
        boosted > 0 && faster > 0,
        "{boosted} boosted, {faster} faster"
    );
}

/// A unit of a player the viewer is not at peace with, made invisible: on a city the viewer
/// sees (siege, work, income), homed in the viewer's city (city_units). The masked outputs
/// see none of them, on the CPU and the host build alike, and equal the outputs of the game
/// with those units taken off; the game's unmasked queries see them.
#[test]
fn a_unit_the_viewer_cannot_see_counts_as_absent() {
    let t = Tables::compile(CATALOG).unwrap();
    let flat = tables::DeviceTables::new(&t).unwrap();
    let mut states = 0;
    for seed in [51, 52, 53] {
        play(&t, 11, vec![7, 7, 7], seed, 600, |g| {
            if g.current_turn < 6 {
                return;
            }
            for viewer in seats(g) {
                let Some(hidden) = hide_units(g, viewer) else {
                    continue;
                };
                let mut removed = hidden.clone();
                for i in 0..removed.tiles.len() {
                    if removed.tiles[i].unit.is_some()
                        && menu::visible_unit(&hidden, &hidden.tiles[i], viewer).is_none()
                    {
                        removed.tiles_mut()[i].unit = None;
                    }
                }
                let at = format!("seed {seed}, turn {}, viewer {viewer}", g.current_turn);
                compare(&hidden, &t, &flat, viewer).unwrap_or_else(|e| panic!("{at}: {e}"));
                for p in [Plane::CityWork, Plane::CityUnits, Plane::CitySiege] {
                    assert_eq!(
                        p.of(&hidden, &t, viewer).unwrap(),
                        p.of(&removed, &t, viewer).unwrap(),
                        "{at}: {p:?}"
                    );
                }
                assert_eq!(
                    queries::income(&hidden, &t, viewer).unwrap(),
                    queries::income(&removed, &t, viewer).unwrap(),
                    "{at}: income"
                );
                states += 1;
            }
        });
    }
    println!("{states} states with hidden units");
    assert!(states > 50, "{states} states");
}

/// `g` with units of another seat the viewer is not at peace with made invisible: every such
/// unit standing on a city the viewer has explored, and one of them homed in a city of the
/// viewer's. None when there is no such unit.
fn hide_units(g: &Game, viewer: i32) -> Option<Game> {
    let mut out = g.clone();
    let foreign = |g: &Game, n: u32| {
        let o = g.units[n as usize].owner;
        let peace = g
            .player(viewer)
            .and_then(|p| p.relation(o))
            .is_some_and(|r| r.state == engine_fast::state::PEACE);
        o != viewer && o != NATURE && !peace
    };
    let mut invisible = engine_fast::inline::Inline::<i64, 9>::new();
    invisible.push(3);
    let list = out.push_effects(invisible);
    let own_city = (0..g.tiles.len()).find(|&i| {
        g.tiles[i].owner == viewer && g.tiles[i].improvement.as_ref().is_some_and(|m| m.kind == 1)
    })?;
    let home: Xy = g.tiles[own_city].coordinates;
    let mut changed = false;
    for i in 0..g.tiles.len() {
        let Some(n) = g.tiles[i].unit.filter(|&n| foreign(g, n)) else {
            continue;
        };
        let on_city = g.tiles[i].explorers.contains(&viewer)
            && g.tiles[i].improvement.as_ref().is_some_and(|m| m.kind == 1);
        if on_city || !changed {
            out.unit_mut(n as usize).effects = list;
            if !changed {
                out.unit_mut(n as usize).home = home;
            }
            changed = true;
        }
    }
    changed.then_some(out)
}
