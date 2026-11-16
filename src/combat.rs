//! Attack candidates: the attack-option scan
//! for an own unit that can attack, each with the hover
//! damage preview (over `battle`).

use crate::menu::{relation, unit_effects, visible_unit};
use crate::state::{Game, Tile, Unit, Xy};
use crate::state::{COOL_DOWN, PEACE};
use crate::tables::{Tables, UnitData};
use serde_json::Value;

pub const ATTACK: u64 = 2;

const CITY: i32 = 1;
const FROZEN: i64 = 0;
const POISONED: i64 = 1;
const BOOSTED: i64 = 2;
// City reward and improvement effect values
const CITY_WALL: i32 = 1;
const ROBBED: i32 = 1;

pub(crate) fn data<'a>(t: &'a Tables, u: &Unit) -> Result<&'a UnitData, String> {
    t.units
        .get(u.kind as usize)
        .ok_or_else(|| "unknown unit kind".into())
}

pub(crate) fn has(t: &Tables, u: &Unit, a: &str) -> Result<bool, String> {
    Ok(data(t, u)?.unit_abilities.has(a))
}

fn peaceful(r: i32) -> bool {
    r == PEACE || r == COOL_DOWN
}

/// The furthest a unit attacks; `Tables::compile` refuses a catalogue with a longer range.
pub const ATTACK_RANGE: usize = 3;

/// Attack targets around a unit, held in place: the square of `ATTACK_RANGE` without its centre.
pub(crate) type Targets =
    crate::inline::Inline<Xy, { (2 * ATTACK_RANGE + 1) * (2 * ATTACK_RANGE + 1) - 1 }>;

/// The targets `unit` can attack within `range`, as seen by its owner.
pub(crate) fn attack_options(
    g: &Game,
    t: &Tables,
    unit: &Unit,
    range: i32,
    ignore_diplomacy: bool,
) -> Result<Targets, String> {
    if unit.leader != 0 {
        return Ok(Targets::new());
    }
    let capturer = has(t, unit, "infiltrate")?;
    if data(t, unit)?.attack.wrapping_mul(10) == 0 && !has(t, unit, "convert")? && !capturer {
        return Ok(Targets::new());
    }
    let drench = has(t, unit, "drench")?;
    let (viewer, c) = (unit.owner, unit.coordinates);
    let mut out = Targets::new();
    if range < 0 {
        return Ok(out);
    }
    let height = g.tiles.len() as i32 / g.width;
    for x in (c.x - range).max(0)..=(c.x + range).min(g.width - 1) {
        for y in (c.y - range).max(0)..=(c.y + range).min(height - 1) {
            let at = Xy { x, y };
            if at == c {
                continue;
            }
            let tile = g.tile(at).ok_or("attack area tile")?;
            if !tile.explorers.contains(&viewer) {
                continue;
            }
            if capturer {
                if let Some(city) = tile.improvement.as_ref().filter(|m| m.kind == CITY) {
                    if tile.owner != 0 && tile.owner != unit.owner {
                        let r = relation(g, viewer, tile.owner);
                        if r == PEACE || being_captured(g, tile) || city.effects.contains(&ROBBED) {
                            continue;
                        }
                        if !ignore_diplomacy && peaceful(r) {
                            continue;
                        }
                        out.push(at);
                    }
                }
                continue;
            }
            if drench && crate::aquarion::floodable(tile) {
                out.push(at);
                continue;
            }
            if let Some(target) = visible_unit(g, tile, viewer) {
                if target.owner != viewer
                    && (ignore_diplomacy || !peaceful(relation(g, viewer, target.owner)))
                {
                    out.push(at);
                }
            }
        }
    }
    Ok(out)
}

/// Being captured: a foreign unit stands in an owned city
fn being_captured(g: &Game, tile: &Tile) -> bool {
    tile.unit
        .and_then(|n| g.units.get(n as usize))
        .is_some_and(|u| {
            u.coordinates == tile.coordinates && tile.owner != 0 && u.owner != tile.owner
        })
}

/// Maximum health: the passenger's health data unless the carrier has Protect
pub(crate) fn max_health(g: &Game, t: &Tables, u: &Unit) -> Result<i32, String> {
    let selected = match u.passenger {
        Some(p) if !has(t, u, "protect")? => g
            .units
            .get(p as usize)
            .ok_or("passenger node out of range")?,
        _ => u,
    };
    Ok(data(t, selected)?
        .health
        .wrapping_add(selected.promotion_level.wrapping_mul(50)))
}

/// Defence bonus (×10): tech terrain bonus, own city with Fortify, halved when poisoned
pub(crate) fn defence_bonus(g: &Game, t: &Tables, u: &Unit) -> Result<i32, String> {
    let mut bonus = 10;
    if let Some(tile) = g.tile(u.coordinates) {
        if let Some(p) = g.player(u.owner).filter(|_| u.owner != 0) {
            let terrain = t.terrain.name(tile.terrain as usize)?;
            for &k in &p.available_tech {
                let Some(tech) = t.techs.get(k as usize) else {
                    continue;
                };
                if let Some(v) = tech
                    .defence_bonus_unlocks
                    .get(terrain)
                    .and_then(Value::as_i64)
                {
                    if v > 1 {
                        bonus = 15;
                    }
                    break;
                }
            }
        }
        if let Some(city) = tile.improvement.as_ref().filter(|m| m.kind == CITY) {
            if tile.owner == u.owner && has(t, u, "fortify")? {
                bonus = if city.rewards.contains(&CITY_WALL) {
                    40
                } else {
                    15
                };
            }
        }
    }
    if unit_effects(g, u).contains(&POISONED) {
        bonus >>= 1;
    }
    Ok(bonus)
}

pub(crate) fn can_retaliate(
    g: &Game,
    t: &Tables,
    attacker: &Unit,
    defender: &Unit,
) -> Result<bool, String> {
    if has(t, defender, "stiff")?
        || has(t, attacker, "convert")?
        || has(t, attacker, "surprise")?
        || unit_effects(g, defender).contains(&FROZEN)
        || has(t, attacker, "freeze")?
    {
        return Ok(false);
    }
    Ok(
        attack_options(g, t, defender, data(t, defender)?.range, true)?
            .contains(&attacker.coordinates),
    )
}

/// BattleHelpers.Round: integer division truncates toward zero
fn round_damage(v: i32) -> i32 {
    let base = (v / 10).wrapping_mul(10);
    if v % 10 >= 10 - v % 10 {
        base.wrapping_add(10)
    } else {
        base
    }
}

fn div(n: i64, d: i64) -> Result<i64, String> {
    n.checked_div(d)
        .ok_or_else(|| "battle division overflow".into())
}

/// Battle results (simple data, not accounting for peace): (attack damage, retaliation or -1,
/// whether the game got as far as the defence bonus and as far as the retaliation query)
pub(crate) fn battle(
    g: &Game,
    t: &Tables,
    attacker: &Unit,
    defender: &Unit,
) -> Result<(i32, i32, bool, bool), String> {
    let none = (0, -1, false, false);
    let ah = max_health(g, t, attacker)?;
    let dh = max_health(g, t, defender)?;
    if ah < 1 || dh < 1 {
        return Ok(none);
    }
    let a = data(t, attacker)?;
    let atk = if has(t, attacker, "tentacles")? {
        a.defence.wrapping_mul(10)
    } else {
        a.attack
            .wrapping_mul(10)
            .wrapping_add(if unit_effects(g, attacker).contains(&BOOSTED) {
                50
            } else {
                0
            })
    };
    let bonus = defence_bonus(g, t, defender)?;
    let def = data(t, defender)?.defence.wrapping_mul(bonus);
    let af = i64::from(attacker.health.wrapping_mul(atk).wrapping_mul(100)) / i64::from(ah);
    let df = i64::from(def.wrapping_mul(defender.health).wrapping_mul(100)) / i64::from(dh);
    let total = af.wrapping_add(df);
    if total <= 0 {
        return Ok((0, -1, true, false));
    }
    let retaliates = can_retaliate(g, t, attacker, defender)?;
    let denominator = total.wrapping_mul(1000);
    let dealt = round_damage(div(
        i64::from(atk).wrapping_mul(af).wrapping_mul(450),
        denominator,
    )? as i32);
    let back = if retaliates {
        round_damage(div(
            df.wrapping_mul(i64::from(def)).wrapping_mul(4500),
            denominator.wrapping_mul(i64::from(bonus)),
        )? as i32)
    } else {
        -1
    };
    Ok((
        dealt,
        if dealt >= defender.health { 0 } else { back },
        true,
        true,
    ))
}

/// Damage indicator: None for the lethal icon, else the displayed number
fn shown(raw: i32, health: i32) -> Option<i32> {
    (raw < health).then(|| (raw as f32 * 0.1_f32).round_ties_even() as i32)
}

/// The visible root unit for the attacker's owner: a live, non-explorer visible unit
fn visible_root<'a>(g: &'a Game, tile: &Tile, viewer: i32) -> Option<&'a Unit> {
    visible_unit(g, tile, viewer).filter(|u| u.health != 0 && !u.is_temporary_explorer)
}

/// Attack candidates for the mover's unit on tile `i`, in the game's target order, each with its preview.
/// `mover` must be the current player with no pending trigger (as `menu` guarantees): the validator's
/// current-player and trigger checks are not repeated here. The menu's can-perform-action gate needs no
/// check of its own: a unit that can attack and has a target always passes it.
pub fn attacks_at(
    g: &Game,
    t: &Tables,
    mover: i32,
    i: usize,
) -> Result<Vec<crate::menu::Cand>, String> {
    let mut out = Vec::new();
    attacks_into(g, t, mover, i, &mut out)?;
    Ok(out)
}

/// `attacks_at`, appending to `out`
pub(crate) fn attacks_into(
    g: &Game,
    t: &Tables,
    mover: i32,
    i: usize,
    out: &mut Vec<crate::menu::Cand>,
) -> Result<(), String> {
    let tile = &g.tiles[i];
    let Some(node) = tile.unit else {
        return Ok(());
    };
    let unit = g.units.get(node as usize).ok_or("unit node out of range")?;
    if unit.owner != mover
        || unit.attacked
        || unit_effects(g, unit).contains(&FROZEN)
        || unit.moved && !has(t, unit, "dash")?
    {
        return Ok(());
    }
    for &to in attack_options(g, t, unit, data(t, unit)?.range, false)?.iter() {
        let target = g.tile(to).unwrap();
        // the hover readout needs both ends to be live visible roots
        let attacker = visible_root(g, tile, mover);
        let preview = match attacker
            .and(visible_root(g, target, mover))
            .filter(|d| d.owner != mover)
        {
            Some(defender) => {
                let (dealt, back, ..) = battle(g, t, unit, defender)?;
                let taken = (back != -1).then(|| shown(back, unit.health));
                Some(engine_fast_frame::AttackPreview {
                    damage_dealt: shown(dealt, defender.health),
                    kills_target: shown(dealt, defender.health).is_none(),
                    retaliation: taken.is_some(),
                    damage_taken: taken.flatten(),
                    attacker_dies: taken.is_some_and(|x| x.is_none()),
                })
            }
            None => None,
        };
        let mut c = crate::menu::cand(ATTACK)
            .source(tile.coordinates)
            .target(to);
        c.details.attack_preview = preview;
        out.push(c);
    }
    Ok(())
}
