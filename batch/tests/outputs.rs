//! The batch's `income` and `max_health` outputs (the displayed income and
//! maximum health) equal the per-game queries at every decision of a few games, and the
//! maximum health equals a second statement of the panel's rule written here on purpose
//! (the passenger's unless the carrier has Protect, tenths
//! rounded up), so a mistake in the batch's or the query's copy shows. Masking of hidden
//! units is done by the queries (`queries::as_shown`) and not tested here.

use engine_fast::menu;
use engine_fast::queries::{self, UnitStats, UNKNOWN};
use engine_fast::reset::Config;
use engine_fast::state::{Game, Unit};
use engine_fast::tables::Tables;
use engine_fast_batch::{Applied, Batch, Status, SKIP};

const CATALOG: &str = include_str!(env!("ENGINE_FAST_RULES"));
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

#[test]
fn income_and_max_health_equal_the_queries() {
    let t = Tables::compile(CATALOG).unwrap();
    let configs = configs();
    let mut batch = Batch::new(CATALOG, configs.len() + 1, 3).unwrap();
    batch
        .reset(&configs.iter().cloned().enumerate().collect::<Vec<_>>())
        .unwrap();
    for i in 0..configs.len() {
        assert_eq!(batch.status(i), Status::Playing, "{}", batch.reason(i));
    }
    let empty = configs.len();
    assert_eq!(batch.income(empty).unwrap(), None);
    assert_eq!(batch.max_health(empty).unwrap(), None);
    let mut applied = vec![Applied::Skipped; batch.len()];
    // what the games covered: incomes above zero, foreign units shown, carriers showing a
    // passenger's maximum health different from their own kind's
    let (mut paying, mut foreign, mut carried) = (0, 0, 0);
    for decision in 0..u64::from(DECISIONS) {
        let mut moves = vec![SKIP; batch.len()];
        for (i, c) in configs.iter().enumerate() {
            if batch.status(i) != Status::Playing {
                assert_eq!(batch.income(i).unwrap(), None);
                assert_eq!(batch.max_health(i).unwrap(), None);
                continue;
            }
            let g = batch.game(i).unwrap();
            let mover = g.players[g.current_player_index as usize].action.id;
            let income = batch.income(i).unwrap().unwrap();
            assert_eq!(income, queries::income(g, &t, mover).unwrap(), "game {i}");
            paying += usize::from(income > 0);
            let plane = batch.max_health(i).unwrap().unwrap();
            let mut stats = UnitStats::default();
            queries::unit_stats(g, &t, mover, &mut stats).unwrap();
            assert_eq!(plane, stats.max_health, "game {i} decision {decision}");
            for (k, &v) in plane.iter().enumerate() {
                assert_eq!(v, panel_max_health(g, &t, k, mover), "game {i} tile {k}");
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
    assert!(
        paying > 0 && foreign > 0 && carried > 0,
        "{paying} {foreign} {carried}"
    );
}
