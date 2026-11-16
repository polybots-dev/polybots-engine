//! Aquarion's rules: flooded tiles, Bubble, Drench, clearing a tile effect.

use crate::action::{Action, Combat, TurnEffect};
use crate::inline::Seats;
use crate::state::{Game, Tile, Unit, Xy};
use crate::step::player_index;
use crate::tables::Tables;

const WATER: i32 = 1;
const OCEAN: i32 = 2;
const MOUNTAIN: i32 = 4;
const ICE: i32 = 6;
pub(crate) const FLOODED: i32 = 1;
const SWAMPED: i32 = 2;
pub(crate) const ALGAE: i32 = 4;
const FOAM: i32 = 5;
const BUBBLE: i64 = 4;
const SWAMP_SKIN: i32 = 17;

/// Wetland: Flooded or Algae
pub(crate) fn wetland(t: &Tile) -> bool {
    let e = crate::menu::effects(t);
    e.contains(&FLOODED) || e.contains(&ALGAE)
}

pub(crate) fn flooded(t: &Tile) -> bool {
    crate::menu::effects(t).contains(&FLOODED)
}

/// Floodable: not water, mountain or ice, and not flooded yet
pub(crate) fn floodable(t: &Tile) -> bool {
    ![WATER, OCEAN, MOUNTAIN, ICE].contains(&t.terrain) && !flooded(t)
}

/// Flood: Flooded once, and Swamped too when the actor has the Swamp skin
pub(crate) fn flood_tile(g: &mut Game, actor: i32, c: Xy) -> Result<(), String> {
    let i = g.index(c).ok_or("flood tile")?;
    if !g.tiles[i].effects.contains(&FLOODED) {
        g.tiles[i].effects.try_push(FLOODED)?;
    }
    let p = player_index(g, actor)?;
    if g.players[p].skin == SWAMP_SKIN && !g.tiles[i].effects.contains(&SWAMPED) {
        g.tiles[i].effects.try_push(SWAMPED)?;
    }
    Ok(())
}

/// Drain: the first Algae and Flooded go, and Swamped on a Swamp-skin tile
pub(crate) fn drain_tile(g: &mut Game, c: Xy) -> Result<(), String> {
    let i = g.index(c).ok_or("drain tile")?;
    let swamp = g.tiles[i].skin == SWAMP_SKIN;
    let effects = &mut g.tiles[i].effects;
    for e in [ALGAE, FLOODED].into_iter().chain(swamp.then_some(SWAMPED)) {
        if let Some(k) = effects.iter().position(|&v| v == e) {
            effects.remove(k);
        }
    }
    Ok(())
}

fn has(t: &Tables, kind: i32, a: &str) -> bool {
    t.units
        .get(kind as usize)
        .is_some_and(|d| d.unit_abilities.has(a))
}

/// Combat Flood: the tile floods; an amphibious unit on it gains Bubble if the actor has the
/// Bubble ability; then connections and routes. The game removes a Flood whose tile is
/// not floodable before it runs.
pub(crate) fn flood(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    if !g.tile(c).is_some_and(floodable) {
        return Ok(());
    }
    flood_tile(g, actor, c)?;
    if let Some(n) = g.tile(c).and_then(|x| x.unit) {
        // the game's ability query refreshes the actor's tech cache
        crate::step::refresh_tech_cache(g, t, actor)?;
        let u = &g.units[n as usize];
        if crate::menu::player_ability(g, t, actor, "bubble")
            && has(t, u.kind, "amphibious")
            && !crate::menu::unit_effect_list(g, u.effects).contains(&BUBBLE)
        {
            out.push(Action::Combat(Combat::Bubble {
                player: crate::step::mover(g),
                unit: u.id,
                add: true,
            }));
        }
    }
    out.push(Action::UpdateCityConnections {
        actor,
        players: Seats::from_iter([actor]),
    });
    out.push(Action::UpdateRoutes { player: actor });
    Ok(())
}

/// Combat Bubble: the unit with id `unit`, wherever it stands, gains or loses Bubble
pub(crate) fn bubble(g: &mut Game, unit: u32, add: bool) -> Result<(), String> {
    let Some(n) = g
        .tiles
        .iter()
        .filter_map(|x| x.unit)
        .find(|&n| g.units[n as usize].id == unit)
    else {
        return Ok(());
    };
    let list = g.units[n as usize].effects as usize;
    let fx = g.effect_lists.get_mut(list).ok_or("effect list")?;
    match fx.iter().position(|&e| e == BUBBLE) {
        None if add => fx.try_push(BUBBLE)?,
        Some(k) if !add => {
            fx.remove(k);
        }
        _ => {}
    }
    Ok(())
}

/// A unit's Bubble loss when it is attacked
pub(crate) fn bubble_lost(g: &Game, u: &Unit) -> Option<Action> {
    crate::menu::unit_effect_list(g, u.effects)
        .contains(&BUBBLE)
        .then(|| {
            Action::Combat(Combat::Bubble {
                player: crate::step::mover(g),
                unit: u.id,
                add: false,
            })
        })
}

/// The ClearTileEffect entries of `u`'s unit menu: Algae, for a unit
/// that can attack or move and swims, flies or is amphibious; Flooded, for one that can attack
/// and move, when the mover's tribe does not flood, and that is amphibious or does not swim.
pub(crate) fn clear_options(
    g: &Game,
    t: &Tables,
    mover: i32,
    u: &Unit,
    tile: &Tile,
    can_move: bool,
    can_attack: bool,
) -> Result<[Option<i32>; 2], String> {
    let e = crate::menu::effects(tile);
    let algae = e.contains(&ALGAE)
        && (can_attack || can_move)
        && (has(t, u.kind, "amphibious") || has(t, u.kind, "swim") || has(t, u.kind, "fly"));
    let floods = g
        .player(mover)
        .and_then(|p| t.tribes.get(p.tribe as usize))
        .is_some_and(|d| d.tribe_abilities.has("flooded"));
    let flood = e.contains(&FLOODED)
        && can_attack
        && can_move
        && !floods
        && (has(t, u.kind, "amphibious") || !has(t, u.kind, "swim"));
    Ok([algae.then_some(ALGAE), flood.then_some(FLOODED)])
}

/// Clearing a tile effect: the first such effect goes (Foam with Algae), connections and
/// routes follow, and a unit that cleared it is spent
pub(crate) fn clear_tile_effect(
    g: &mut Game,
    p: i32,
    c: Xy,
    effect: i32,
    triggered: bool,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let i = g.index(c).ok_or("clear effect tile")?;
    let effects = &mut g.tiles[i].effects;
    if let Some(k) = effects.iter().position(|&e| e == effect) {
        effects.remove(k);
    }
    if effect == ALGAE {
        if let Some(k) = effects.iter().position(|&e| e == FOAM) {
            effects.remove(k);
        }
    }
    out.push(Action::UpdateCityConnections {
        actor: p,
        players: Seats::from_iter([p]),
    });
    out.push(Action::UpdateRoutes { player: p });
    if triggered {
        if let Some(n) = g.tiles[i].unit {
            crate::units::make_exhausted(g, n)?;
        }
    }
    Ok(())
}

/// The ClearTileEffect command: no command base; the queued action is marked as a unit's
pub(crate) fn clear_command(p: i32, c: Xy, effect: i32, out: &mut Vec<Action>) {
    out.push(Action::TurnEffect(TurnEffect::ClearTileEffect {
        player: p,
        coordinates: c,
        effect,
        triggered_by_unit: true,
    }));
}

const TENTACLE: i32 = 3;
const FROZEN: i64 = 0;

/// Tentacle update: every tile loses its first Tentacle effect, then each tile next
/// to an unfrozen Tentacles unit gains one
pub(crate) fn update_tentacles(g: &mut Game, t: &Tables) -> Result<(), String> {
    for x in &mut g.tiles {
        if let Some(k) = x.effects.iter().position(|&e| e == TENTACLE) {
            x.effects.remove(k);
        }
    }
    let mut affected = Vec::new();
    for x in &g.tiles {
        let Some(n) = x.unit else { continue };
        let u = &g.units[n as usize];
        if !has(t, u.kind, "tentacles")
            || crate::menu::unit_effect_list(g, u.effects).contains(&FROZEN)
        {
            continue;
        }
        let c = x.coordinates;
        for xx in c.x - 1..=c.x + 1 {
            for yy in c.y - 1..=c.y + 1 {
                let d = Xy { x: xx, y: yy };
                if d != c {
                    if let Some(i) = g.index(d) {
                        affected.push(i);
                    }
                }
            }
        }
    }
    for i in affected {
        if !g.tiles[i].effects.contains(&TENTACLE) {
            g.tiles[i].effects.try_push(TENTACLE)?;
        }
    }
    Ok(())
}

pub(crate) fn tentacle_tile(t: &Tile) -> bool {
    crate::menu::effects(t).contains(&TENTACLE)
}

/// Tentacle strike: a unit of `actor` that enters a Tentacle tile is hit by the healthiest
/// adjacent unfrozen Tentacles unit (the first on a tie) of another player not at peace with it
pub(crate) fn strike(
    g: &mut Game,
    t: &Tables,
    node: u32,
    actor: i32,
    target: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let defender = g.units[node as usize];
    let mut strongest: Option<(Xy, Unit)> = None;
    for x in target.x - 1..=target.x + 1 {
        for y in target.y - 1..=target.y + 1 {
            let origin = Xy { x, y };
            if origin == target {
                continue;
            }
            let Some(n) = g.tile(origin).and_then(|x| x.unit) else {
                continue;
            };
            let u = g.units[n as usize];
            if u.owner == 255 || u.owner == actor {
                continue;
            }
            let r = crate::step::relation_state(g, actor, u.owner)?;
            if r == crate::state::PEACE || r == crate::state::COOL_DOWN {
                continue;
            }
            if !has(t, u.kind, "tentacles")
                || crate::menu::unit_effect_list(g, u.effects).contains(&FROZEN)
            {
                continue;
            }
            if strongest.as_ref().is_none_or(|(_, o)| u.health > o.health) {
                strongest = Some((origin, u));
            }
        }
    }
    let Some((origin, attacker)) = strongest else {
        return Ok(());
    };
    let (damage, ..) = crate::attack::battle_full(g, t, &attacker, &defender)?;
    out.push(Action::Combat(Combat::Attack {
        player: attacker.owner,
        origin,
        target,
        damage,
        animation: 3,
        should_move_to_target: false,
        delay: 100,
    }));
    Ok(())
}
