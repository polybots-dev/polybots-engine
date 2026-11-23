//! The batch's optional outputs equal the per-game queries at every decision of a few games:
//! `income`, every `Plane`, `city_score` and `progression`. The maximum health equals a second
//! statement of the panel's rule written here on purpose, and `progression` one of
//! its layout from the game's own fields, so a mistake in the batch's or the query's copy
//! shows. Masking of hidden units and the outputs' hidden categories are the lockstep crate's
//! non-interference test.

use engine_fast::menu;
use engine_fast::queries::{self, UnitStats, UNKNOWN};
use engine_fast::reset::Config;
use engine_fast::state::{Game, Unit};
use engine_fast::tables::Tables;
use engine_fast_batch::outputs::{Plane, PROGRESSION_LEN};
use engine_fast_batch::{Applied, Batch, Status, SKIP};

const CATALOG: &str = include_str!("../../rules/rules-124.json");
const DECISIONS: u32 = 400;
const RESIGN: i32 = 35;

fn configs() -> Vec<Config> {
    [(1, 11, 2), (3, 11, 3), (4, 11, 4), (6, 16, 2), (9, 14, 4)]
        .into_iter()
        .map(|(seed, size, seats)| Config {
            rules: 124,
            seed,
            size,
            map: 2,
            mode: 4,
            tribes: vec![7; seats],
            action_limit: DECISIONS,
        })
        .collect()
}

fn choose(seed: i32, decision: u64, n: usize, resign: &[bool]) -> usize {
    let mut x = (seed as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ decision;
    x = (x ^ (x >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let kept: Vec<usize> = (0..n).filter(|&i| !resign[i]).collect();
    kept[((x >> 33) as usize) % kept.len()]
}

fn kind<'a>(t: &'a Tables, u: &Unit) -> &'a engine_fast::tables::UnitData {
    t.units.get(u.kind as usize).unwrap()
}

/// The panel's maximum health of the unit on tile `i` for `viewer`, stated again.
fn panel_max_health(g: &Game, t: &Tables, i: usize, viewer: i32) -> i16 {
    let Some(u) = menu::visible_unit(g, &g.tiles[i], viewer) else {
        return UNKNOWN;
    };
    if u.health == 0 || u.is_temporary_explorer {
        return UNKNOWN;
    }
    let shown: &Unit = match u.passenger {
        Some(p) if !kind(t, u).unit_abilities.has("protect") => &g.units[p as usize],
        _ => u,
    };
    let max = kind(t, shown).health + shown.promotion_level * 50;
    ((max + 9) / 10) as i16
}

/// `progression` stated again from the game: the viewer's player record and relations, and
/// the view's lists.
fn progression_again(g: &Game, t: &Tables, viewer: i32) -> Vec<i32> {
    let mut v = engine_fast::view::View::default();
    engine_fast::view::write(g, t, viewer, &mut v).unwrap();
    let me = g.player(viewer).unwrap();
    let mut out = vec![
        me.casualties,
        i32::from(v.start_tile),
        v.revealed_resources as i32,
    ];
    let pad = |out: &mut Vec<i32>, rows: usize, width: usize, used: usize| {
        out.extend(std::iter::repeat_n(-1, (rows - used) * width))
    };
    out.extend(me.built_unique_improvements.iter().copied());
    pad(&mut out, 8, 1, me.built_unique_improvements.len());
    for task in &v.tasks {
        out.extend([
            i32::from(task.kind),
            i32::from(task.completed),
            task.counter,
        ]);
    }
    pad(&mut out, 10, 3, v.tasks.len());
    for l in &v.lighthouses {
        out.push(i32::from(l.tile));
        out.extend(l.explorers.iter().map(|&e| i32::from(e)));
    }
    pad(&mut out, 4, 6, v.lighthouses.len());
    let level = |p: &engine_fast::state::Player, other: i32| {
        p.relations
            .iter()
            .find(|(id, _)| *id == other)
            .map_or(0, |(_, r)| r.embassy_level)
    };
    for p in &v.players {
        let them = g.player(i32::from(p.seat)).unwrap();
        out.extend([level(me, i32::from(p.seat)), level(them, viewer)]);
    }
    pad(&mut out, 4, 2, v.players.len());
    out
}

#[test]
fn the_outputs_equal_the_queries() {
    let t = Tables::compile(CATALOG).unwrap();
    let configs = configs();
    let mut batch = Batch::new(CATALOG, configs.len() + 1, 3).unwrap();
    batch
        .reset(&configs.iter().cloned().enumerate().collect::<Vec<_>>())
        .unwrap();
    for i in 0..configs.len() {
        assert_eq!(batch.status(i), Status::Playing, "{}", batch.reason(i));
    }
    let none = |batch: &Batch, i: usize| {
        assert_eq!(batch.income(i).unwrap(), None);
        assert_eq!(batch.city_score(i), None);
        assert_eq!(batch.progression(i).unwrap(), None);
        for p in Plane::ALL {
            assert_eq!(batch.plane(i, p).unwrap(), None, "{p:?}");
        }
    };
    none(&batch, configs.len());
    let mut applied = vec![Applied::Skipped; batch.len()];
    // what the games covered: incomes above zero, foreign units shown, carriers showing a
    // passenger's maximum health different from their own kind's, and a few of the rest
    let (mut paying, mut foreign, mut carried) = (0, 0, 0);
    let mut seen = std::collections::BTreeMap::<&str, usize>::new();
    for decision in 0..u64::from(DECISIONS) {
        let mut moves = vec![SKIP; batch.len()];
        for (i, c) in configs.iter().enumerate() {
            if batch.status(i) != Status::Playing {
                none(&batch, i);
                continue;
            }
            let g = batch.game(i).unwrap();
            let mover = g.players[g.current_player_index as usize].action.id;
            let at = format!("game {i} decision {decision}");
            let income = batch.income(i).unwrap().unwrap();
            assert_eq!(income, queries::income(g, &t, mover).unwrap(), "{at}");
            paying += usize::from(income > 0);
            let mut stats = UnitStats::default();
            queries::unit_stats(g, &t, mover, &mut stats).unwrap();
            // in Plane::ALL's order
            let (mut work, mut units, mut siege, mut borders) = Default::default();
            queries::city_work(g, &t, mover, &mut work).unwrap();
            queries::city_units(g, mover, &mut units);
            queries::city_siege(g, mover, &mut siege);
            queries::border_edges(g, mover, &mut borders);
            let want: [Vec<i16>; 8] = [
                stats.defence,
                stats.max_health,
                stats.movement,
                stats.attack_modifier,
                work,
                units,
                siege,
                borders,
            ];
            for (p, want) in Plane::ALL.into_iter().zip(&want) {
                let got = batch.plane(i, p).unwrap().unwrap();
                assert_eq!(&got, want, "{at}: {p:?}");
                let positive = got.iter().filter(|&&v| v > 0).count();
                *seen.entry(p.name()).or_default() += positive;
            }
            let mut score = Vec::new();
            queries::city_score(g, &t, mover, &mut score);
            assert_eq!(batch.city_score(i).unwrap(), score, "{at}: city_score");
            let row = batch.progression(i).unwrap().unwrap();
            assert_eq!(row.len(), PROGRESSION_LEN);
            assert_eq!(row, progression_again(g, &t, mover), "{at}: progression");
            let embassies = row[PROGRESSION_LEN - 8..]
                .iter()
                .filter(|&&v| v > 0)
                .count();
            *seen.entry("embassy").or_default() += embassies;
            let plane = &want[1];
            for (k, &v) in plane.iter().enumerate() {
                assert_eq!(v, panel_max_health(g, &t, k, mover), "{at}: tile {k}");
                let Some(u) = g.tiles[k].unit.map(|n| &g.units[n as usize]) else {
                    continue;
                };
                if v == UNKNOWN {
                    continue;
                }
                foreign += usize::from(u.owner != mover);
                let own = (kind(&t, u).health + u.promotion_level * 50 + 9) / 10;
                carried += usize::from(u.passenger.is_some() && i32::from(v) != own);
            }
            let resign: Vec<bool> = batch.moves(i).iter().map(|m| m[0] == RESIGN).collect();
            moves[i] = choose(c.seed, decision, resign.len(), &resign) as i32;
        }
        if moves.iter().all(|&m| m == SKIP) {
            break;
        }
        batch.step(&moves, &mut applied).unwrap();
    }
    println!("positive entries by output: {seen:?}");
    assert!(
        paying > 0 && foreign > 0 && carried > 0,
        "{paying} {foreign} {carried}"
    );
    for name in ["city_work", "city_units", "border_edges", "movement"] {
        assert!(seen[name] > 0, "no {name} above zero");
    }
}
