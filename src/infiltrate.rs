//! Cloak infiltration: the Infiltrate and InfiltrationReward combat actions, with the
//! resource-visibility query they use.

use crate::action::{Action, Combat, Economy, MoveEffect, Payload};
use crate::state::{Game, Unit, Xy};
use crate::step::{player_index, refresh_tech_cache};
use crate::tables::Tables;

use serde_json::Value;

const CITY: i32 = 1;
const DAGGER: i32 = 37;
const CLOAK: i32 = 38;
/// the unit a poison infiltration places
const POISON_UNIT: i32 = 60;
const MOTH: i32 = 61;
const POISON: i64 = 13;
const PIRATE: i32 = 40;
const REBELLION: i64 = 11;
const NULL: Xy = Xy { x: -1, y: -1 };

/// The Infiltrate action: the cloak dies, the city gets the rebellion reward
pub fn infiltrate(
    g: &mut Game,
    t: &Tables,
    p: i32,
    origin: Xy,
    target: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    // a Cloak when no unit stands on the origin
    let kind = g
        .tile(origin)
        .ok_or("infiltrate origin")?
        .unit
        .map_or(CLOAK, |n| g.units[n as usize].kind);
    crate::attack::kill(g, t, origin, out)?;
    out.push(Action::Combat(Combat::InfiltrationReward {
        player: p,
        // the Moth's infiltration poisons the city instead of raising a rebellion
        reward: if kind == MOTH { POISON } else { REBELLION },
        coordinates: target,
        kind,
    }));
    Ok(())
}

/// The improvements unlocked by tech, then by forward-unlockable tech, whose terrain
/// requirement names this resource; the tech-cache refresh is a state write
pub(crate) fn visible_resource(
    g: &mut Game,
    t: &Tables,
    p: i32,
    c: Xy,
) -> Result<Option<i32>, String> {
    match hidden_or_kind(g, p, c)? {
        Some(kind) => Ok(resource_revealed(g, t, p, kind)?.then_some(kind)),
        None => Ok(None),
    }
}

/// The resource kind on tile `c` that player `p` might see: None on a tile `p` has not
/// explored, without a resource, or with an improvement.
pub(crate) fn hidden_or_kind(g: &Game, p: i32, c: Xy) -> Result<Option<i32>, String> {
    let tile = g.tile(c).ok_or("resource tile")?;
    if p != 255 && !tile.explorers.contains(&p) || tile.improvement.is_some() {
        return Ok(None);
    }
    Ok(tile.resource)
}

/// The resource kinds player `p`'s tech (unlocked, or unlockable next) reveals, as a bit set.
/// The same answers as `resource_revealed` for every kind, without its cache refresh (the
/// cache is the player's techs that the catalog knows).
pub(crate) fn revealed_kinds(g: &Game, t: &Tables, p: i32) -> Result<u32, String> {
    let Ok(i) = player_index(g, p) else {
        return Ok(0);
    };
    let player = &g.players[i];
    let tree = t.tribe_tree(player.tribe)?;
    let reveals = |tech: usize| tree.reveals.get(tech).ok_or("unknown tech")?.clone();
    let mut kinds = 0;
    for &k in &player.available_tech {
        if t.techs.get(k as usize).is_some() {
            kinds |= reveals(k as usize)?;
        }
    }
    for k in crate::menu::unlockable_techs(g, t, p)? {
        kinds |= reveals(k)?;
    }
    // kind 0 is "no resource"
    Ok(kinds & !1)
}

/// Whether player `p`'s tech (unlocked, or unlockable next) reveals resource `kind`. The same
/// answer for every tile while the state is unchanged, so a caller may ask once per kind.
pub(crate) fn resource_revealed(
    g: &mut Game,
    t: &Tables,
    p: i32,
    kind: i32,
) -> Result<bool, String> {
    let Ok(i) = player_index(g, p) else {
        return Ok(false);
    };
    refresh_tech_cache(g, t, p)?;
    if kind == 0 {
        return Ok(false);
    }
    let tree = t.tribe_tree(g.players[i].tribe)?;
    let requires = |tech: usize| -> Result<bool, String> {
        let kinds = tree.reveals.get(tech).ok_or("unknown tech")?.clone()?;
        Ok((0..32).contains(&kind) && kinds >> kind & 1 == 1)
    };
    let cache = crate::step::tech_cache(g, i)?.to_vec();
    for k in cache {
        if requires(k as usize)? {
            return Ok(true);
        }
    }
    for k in crate::menu::unlockable_techs(g, t, p)? {
        if requires(k)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Rebellion weight: a small seeded jitter plus preferences for cities, land, resources,
/// defensive terrain, and against poison
fn rebellion_weight(g: &mut Game, t: &Tables, p: i32, c: Xy) -> Result<f32, String> {
    let hash = crate::step::random_hash(g)?;
    let h = hash.hash(&[c.x, c.y, g.current_turn]);
    let mut weight = hash.range_float(0.0, f32::from_bits(0x3dcccccd), &[h as i32]);
    let tile = *g.tile(c).ok_or("rebellion tile")?;
    if tile.improvement.as_ref().is_some_and(|m| m.kind == CITY) {
        weight += 2.0;
    }
    if tile.terrain == 1 || tile.terrain == 2 {
        weight += -0.1;
    }
    if visible_resource(g, t, p, c)?.is_some() {
        weight += 0.5;
    }
    refresh_tech_cache(g, t, p)?;
    let i = player_index(g, p)?;
    let cache = crate::step::tech_cache(g, i)?.to_vec();
    let terrain = t.terrain.name(tile.terrain as usize)?;
    for k in cache {
        if let Some(v) = t
            .techs
            .get(k as usize)
            .ok_or("unknown tech")?
            .defence_bonus_unlocks
            .get(terrain)
            .and_then(Value::as_i64)
        {
            if v > 1 {
                weight += 1.0;
            }
            break;
        }
    }
    if let Some(m) = &tile.improvement {
        if t.improvements
            .get(m.kind as usize)
            .is_some_and(|d| d.improvement_abilities.has("poison"))
            && !t
                .tribes
                .get(g.players[i].tribe as usize)
                .is_some_and(|d| d.tribe_abilities.has("poisonresist"))
        {
            weight += -0.5;
        }
    }
    Ok(weight)
}

/// Raw production for a city
fn raw_production(g: &Game, t: &Tables, c: Xy) -> Result<i32, String> {
    let tile = g.tile(c).ok_or("production tile")?;
    let Some(m) = &tile.improvement else {
        return Ok(0);
    };
    let Some(d) = t.improvements.get(m.kind as usize).filter(|_| m.kind != 0) else {
        return Ok(0);
    };
    let mut result = 0;
    if m.kind == CITY && tile.owner != 0 {
        let Some(p) = g.player(tile.owner) else {
            return Ok(0);
        };
        result = m.level + m.production - i32::from(p.start_tile != c);
    }
    let work = d.work.unwrap_or(0);
    if work > 0 && !d.adjacency_improvements.is_empty() {
        result = (work * m.level).max(0);
    }
    Ok(result)
}

/// a profile unit for placement
pub(crate) fn profile_unit(t: &Tables, p: i32, kind: i32, c: Xy) -> Result<Unit, String> {
    let d = t.units.get(kind as usize).ok_or("placement unit kind")?;
    Ok(Unit {
        id: 0,
        leader: 0,
        follower: 0,
        owner: p,
        kind,
        previous_turn_end_coordinates: Xy { x: 0, y: 0 },
        coordinates: c,
        home: c,
        passenger: None,
        health: d.health,
        promotion_level: 0,
        xp: 0,
        moved: false,
        attacked: false,
        direction: 0,
        flipped: false,
        created_turn: 0,
        // a fresh effect list: an index past every list reads as empty
        effects: u32::MAX,
        is_temporary_explorer: false,
        birth_climate: 0,
        birth_climate_skin: 0,
    })
}

/// The InfiltrationReward combat action (the rebellion reward)
pub fn reward(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    reward: i64,
    city: Xy,
    kind: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    if reward != REBELLION && reward != POISON {
        return Err(format!("infiltration reward {reward} not implemented"));
    }
    let Some(ci) = g.index(city) else {
        return Ok(());
    };
    let Some(improvement) = g.tiles[ci].improvement else {
        return Ok(());
    };
    let Ok(pi) = player_index(g, actor) else {
        return Ok(());
    };
    let mut temporary = profile_unit(t, actor, kind, city)?;
    temporary.home = Xy { x: 0, y: 0 };
    temporary.direction = 7;
    if reward == REBELLION {
        out.push(Action::Payload(Payload::IncreaseCurrency {
            player: actor,
            delay: 10,
            source: city,
            amount: raw_production(g, t, city)?,
        }));
    }
    let effects = &mut g.tile_mut(ci).improvement.as_mut().unwrap().effects;
    if !effects.contains(&1) {
        effects.push(1);
    }
    let defender = g.tiles[ci].unit.map(|n| g.units[n as usize]);
    let battle = match &defender {
        Some(d) => Some(crate::attack::battle_full(g, t, &temporary, d)?),
        None => None,
    };
    let survives = matches!((&battle, &defender), (Some(b), Some(d)) if b.0 < d.health);
    let poison = reward == POISON;
    if survives && poison {
        out.push(Action::Combat(Combat::Poison {
            player: actor,
            origin: city,
            target: city,
        }));
    }
    // the dagger (a poison reward's unit 60) as the actor's tribe trains it; the pirate is
    // not resolved, and a poison reward places no pirate
    let base = if poison { POISON_UNIT } else { DAGGER };
    let kind = t.tribe_unit(g.players[pi].tribe, base)?;
    let water = crate::routes::has_terrain(crate::routes::movements(g, t, actor)?, 1) && !poison;
    let profile = profile_unit(t, actor, kind, city)?;
    let sea = if water {
        Some(profile_unit(t, actor, PIRATE, city)?)
    } else {
        None
    };
    let r = improvement.border_size;
    let mut candidates = Vec::new();
    for xx in city.x - r..=city.x + r {
        for yy in city.y - r..=city.y + r {
            let c = Xy { x: xx, y: yy };
            if c == city {
                continue;
            }
            let Some(tile) = g.tile(c) else { continue };
            if tile.ruling_city != city || tile.unit.is_some() {
                continue;
            }
            let land = placement(g, t, &profile, c)?;
            let ok = land
                || match &sea {
                    Some(s) => placement(g, t, s, c)?,
                    None => false,
                };
            if ok {
                candidates.push(c);
            }
        }
    }
    if !survives {
        candidates.push(city);
    }
    if candidates.len() > 1 {
        let mut slots = Vec::with_capacity(candidates.len());
        for (k, &c) in candidates.iter().enumerate() {
            slots.push((-rebellion_weight(g, t, actor, c)?, k));
        }
        crate::explore::sort_slots(&mut slots);
        candidates = slots.into_iter().map(|(_, k)| candidates[k]).collect();
    }
    let count = (improvement.level as usize).min(5).min(candidates.len());
    for &c in candidates[..count].iter().rev() {
        if sea.is_some()
            && g.tile(c).is_some_and(|x| [1, 2].contains(&x.terrain))
            && !placement(g, t, &profile, c)?
        {
            out.push(Action::MoveEffect(MoveEffect::Embark {
                player: actor,
                coordinates: c,
            }));
        }
        out.push(Action::Economy(Economy::Train {
            player: actor,
            kind,
            target: c,
            cost: 0,
            direction_target: city,
            train_action_aborted: false,
        }));
    }
    if let Some(b) = battle {
        crate::attack::perform_attack(g, t, actor, NULL, city, b.0, out)?;
    }
    Ok(())
}

/// Placement accessibility with its tech-cache refresh
fn placement(g: &mut Game, t: &Tables, unit: &Unit, c: Xy) -> Result<bool, String> {
    refresh_tech_cache(g, t, unit.owner)?;
    crate::movement::placement_accessible(g, t, unit, c)
}
