//! Polaris's rules: freezing tiles and units, breaking ice, the ice bank's level and the
//! spreading climate.

use crate::action::{Action, Combat, Economy, MoveEffect, TurnEffect};
use crate::inline::Seats;
use crate::state::{Game, Unit, Xy, COOL_DOWN, PEACE};
use crate::step::{player_index, relation_state};
use crate::tables::Tables;

const WATER: i32 = 1;
const OCEAN: i32 = 2;
const FIELD: i32 = 3;
const MOUNTAIN: i32 = 4;
const FOREST: i32 = 5;
const ICE: i32 = 6;
const FLOODED: i32 = 1;
const SWAMPED: i32 = 2;
const ALGAE: i32 = 4;
const FROZEN: i64 = 0;
const INVISIBLE: i64 = 3;
const PETRIFIED: i64 = 5;
const BRIDGE: i32 = 48;
const ICE_BANK: i32 = 34;
const MAGMA_SKIN: i32 = 18;
const POLARIS: i32 = 16;
const NULL: Xy = Xy { x: -1, y: -1 };

fn unit_has(t: &Tables, kind: i32, a: &str) -> bool {
    t.units
        .get(kind as usize)
        .is_some_and(|d| d.unit_abilities.has(a))
}

/// Whether tribe `tribe` spreads its climate.
pub(crate) fn spreading(t: &Tables, tribe: i32) -> bool {
    t.tribes
        .get(tribe as usize)
        .is_some_and(|d| d.tribe_abilities.has("spread"))
}

/// A player's climate, with the game's lazy default: unset (0) reads as the tribe.
fn climate_of(p: &crate::state::Player) -> i32 {
    if p.climate == 0 {
        p.tribe
    } else {
        p.climate
    }
}

/// The lazy write of an unset climate: it becomes the tribe's.
fn settle_climate(g: &mut Game, i: usize) {
    if g.players[i].climate == 0 {
        g.players[i].climate = g.players[i].tribe;
    }
}

/// Active peace: the unit is the actor's, or its owner is at peace or cooling down
fn at_peace(g: &Game, actor: i32, u: &Unit) -> bool {
    u.owner == actor || {
        let r = crate::menu::relation(g, actor, u.owner);
        r == PEACE || r == COOL_DOWN
    }
}

/// Whether the tile is freezable for `actor` at `c`, without its lazy climate write. The second
/// value says whether the game reaches that write (it does whenever the answer is not
/// settled by ownership, exploration, water or flooding).
fn freezable(g: &Game, actor: i32, c: Xy, only_owned: bool) -> Result<(bool, bool), String> {
    let p = g.player(actor).ok_or("freeze actor missing")?;
    let tile = g.tile(c).ok_or("freeze tile off the board")?;
    if only_owned && tile.owner != actor {
        return Ok((false, false));
    }
    if actor != 255 && !tile.explorers.contains(&actor) {
        return Ok((false, false));
    }
    if tile.terrain == WATER
        || tile.terrain == OCEAN
        || crate::menu::effects(tile).contains(&FLOODED)
    {
        return Ok((true, false));
    }
    if tile.climate != climate_of(p) || tile.skin != p.skin {
        return Ok((true, true));
    }
    let open = crate::menu::visible_unit(g, tile, actor).is_some_and(|u| {
        !at_peace(g, actor, u) && !crate::menu::unit_effect_list(g, u.effects).contains(&FROZEN)
    });
    Ok((open, true))
}

/// `freezable` with the game's lazy climate write applied.
fn freezable_mut(g: &mut Game, actor: i32, c: Xy, only_owned: bool) -> Result<bool, String> {
    let (answer, writes) = freezable(g, actor, c, only_owned)?;
    if writes {
        let i = player_index(g, actor)?;
        settle_climate(g, i);
    }
    Ok(answer)
}

/// Whether `u` can freeze its area (radius 1), read-only: the menu's question.
pub(crate) fn can_freeze_area(g: &Game, t: &Tables, u: &Unit) -> Result<bool, String> {
    if u.moved || u.attacked || !unit_has(t, u.kind, "freezearea") || g.player(u.owner).is_none() {
        return Ok(false);
    }
    for c in area_without_centre(g, u.coordinates, 1) {
        if freezable(g, u.owner, c, false)?.0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `can_freeze_area` with the writes of the game's validator (the queried tiles' lazy
/// climate writes, up to the first freezable tile).
pub(crate) fn admit_freeze_area(g: &mut Game, t: &Tables, c: Xy) -> Result<(), String> {
    let n = g
        .tile(c)
        .and_then(|x| x.unit)
        .ok_or("freeze area with no unit")?;
    let u = g.units[n as usize];
    if u.moved || u.attacked || !unit_has(t, u.kind, "freezearea") || g.player(u.owner).is_none() {
        return Err("freeze area refused".into());
    }
    for x in area_without_centre(g, u.coordinates, 1) {
        if freezable_mut(g, u.owner, x, false)? {
            return Ok(());
        }
    }
    Err("freeze area refused".into())
}

/// The area around `c` (radius, diagonals, no centre) clipped to the board, x-major
fn area_without_centre(g: &Game, c: Xy, r: i32) -> Vec<Xy> {
    let h = g.tiles.len() as i32 / g.width;
    let mut out = Vec::new();
    for x in (c.x - r).max(0)..=(c.x + r).min(g.width - 1) {
        for y in (c.y - r).max(0)..=(c.y + r).min(h - 1) {
            if (Xy { x, y }) != c {
                out.push(Xy { x, y });
            }
        }
    }
    out
}

/// The FreezeArea command: the unit is spent and its area freezes.
pub(crate) fn freeze_area_command(
    g: &mut Game,
    p: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    crate::step::base(out, p, 17);
    spend(g, c);
    out.push(Action::Combat(Combat::FreezeArea {
        player: p,
        coordinates: c,
        radius: 1,
        freeze_units: true,
        only_owned: false,
    }));
    Ok(())
}

fn spend(g: &mut Game, c: Xy) -> Option<u32> {
    let n = g.tile(c)?.unit?;
    let u = &mut g.units[n as usize];
    u.moved = true;
    u.attacked = true;
    Some(n)
}

/// Freeze area: connections and routes first, then, from the far end of the sorted area,
/// FreezeTile where freezable and FreezeUnit on any unit (seen or not) not at peace and not frozen
pub(crate) fn freeze_area(
    g: &mut Game,
    actor: i32,
    center: Xy,
    radius: i32,
    freeze_units: bool,
    only_owned: bool,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let mut area = crate::explore::area_sorted(g, center, radius)?.to_vec();
    if area.is_empty() {
        return Ok(());
    }
    out.push(Action::UpdateCityConnections {
        actor,
        players: Seats::from_iter([actor]),
    });
    out.push(Action::UpdateRoutes { player: actor });
    player_index(g, actor)?;
    area.reverse();
    for i in area {
        let c = g.tiles[i].coordinates;
        if freezable_mut(g, actor, c, only_owned)? {
            out.push(Action::Combat(Combat::FreezeTile {
                player: actor,
                coordinates: c,
            }));
        }
        if freeze_units {
            if let Some(n) = g.tiles[i].unit {
                let u = g.units[n as usize];
                if !active_peace(g, actor, &u)?
                    && !crate::menu::unit_effect_list(g, u.effects).contains(&FROZEN)
                {
                    out.push(Action::Combat(Combat::FreezeUnit {
                        player: actor,
                        origin: NULL,
                        target: c,
                        damage: 0,
                    }));
                }
            }
        }
    }
    Ok(())
}

/// Active peace with its relation lookup's record write
fn active_peace(g: &mut Game, actor: i32, u: &Unit) -> Result<bool, String> {
    if u.owner == actor {
        return Ok(true);
    }
    let owner = u.owner;
    let r = relation_state(g, actor, owner)?;
    Ok(r == PEACE || r == COOL_DOWN)
}

/// Freeze a tile: water becomes ice, the tile takes the actor's climate and skin
/// (Swamped goes from a flooded tile), and the ice banks are levelled again
pub(crate) fn freeze_tile(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    // the game removes a FreezeTile whose tile is not freezable (its validity query keeps
    // the query's lazy writes) before it runs
    if !freezable_mut(g, actor, c, false)? {
        return Ok(());
    }
    {
        let i = g.index(c).ok_or("freeze tile")?;
        if [WATER, OCEAN].contains(&g.tiles[i].terrain) {
            g.tiles[i].terrain = ICE;
        }
        if g.tiles[i].effects.contains(&FLOODED) {
            if let Some(k) = g.tiles[i].effects.iter().position(|&e| e == SWAMPED) {
                g.tiles[i].effects.remove(k);
            }
        }
        let pi = player_index(g, actor)?;
        settle_climate(g, pi);
        g.tiles[i].climate = g.players[pi].climate;
        g.tiles[i].skin = g.players[pi].skin;
        ice_bank_levels(g, t, out)?;
    }
    crate::mapgen::candidate::generate_shorelines(g);
    Ok(())
}

/// Freeze a unit: the target is spent and Frozen (and Petrified on the Magma skin); a
/// freeze from an attacker deals its damage first
pub(crate) fn freeze_unit(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    origin: Xy,
    target: Xy,
    damage: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let n = g
        .tile(target)
        .and_then(|x| x.unit)
        .ok_or("freeze unit target")?;
    let owner = g.units[n as usize].owner;
    let ai = player_index(g, actor)?;
    player_index(g, owner)?;
    crate::step::attack_diplomacy(g, actor, owner, out)?;
    if origin != NULL {
        crate::attack::perform_attack(g, t, actor, origin, target, damage, out)?;
    }
    let u = &mut g.units[n as usize];
    u.moved = true;
    u.attacked = true;
    add_effect(g, n, FROZEN)?;
    if g.players[ai].skin == MAGMA_SKIN {
        add_effect(g, n, PETRIFIED)?;
    }
    crate::attack::reset_peace(g, ai);
    if g.fixed.achievements_active {
        return Err("achievement tracking not implemented".into());
    }
    if unit_has(t, g.units[n as usize].kind, "tentacles") {
        crate::aquarion::update_tentacles(g, t)?;
    }
    Ok(())
}

fn add_effect(g: &mut Game, n: u32, e: i64) -> Result<(), String> {
    let list = g.units[n as usize].effects as usize;
    let fx = g.effect_lists.get_mut(list).ok_or("effect list")?;
    if !fx.contains(&e) {
        fx.try_push(e)?;
    }
    Ok(())
}

/// Breakable ice: ice that is empty, holds a swimmer or flyer, a unit `viewer` cannot
/// see, algae or a bridge
fn breakable(g: &Game, t: &Tables, c: Xy, viewer: i32) -> bool {
    let Some(tile) = g.tile(c) else {
        return false;
    };
    if tile.terrain != ICE {
        return false;
    }
    let forced = crate::menu::effects(tile).contains(&ALGAE)
        || tile.improvement.as_ref().is_some_and(|m| m.kind == BRIDGE);
    let free = match tile.unit.and_then(|n| g.units.get(n as usize)) {
        None => true,
        Some(u) => {
            unit_has(t, u.kind, "swim")
                || unit_has(t, u.kind, "fly")
                || crate::menu::unit_effect_list(g, u.effects).contains(&INVISIBLE)
                    && u.owner != viewer
                    && crate::menu::relation(g, viewer, u.owner) != PEACE
        }
    };
    free || forced
}

fn neighbours(g: &Game, c: Xy) -> impl Iterator<Item = Xy> + '_ {
    (c.x - 1..=c.x + 1)
        .flat_map(move |x| (c.y - 1..=c.y + 1).map(move |y| Xy { x, y }))
        .filter(move |&xy| xy != c && g.tile(xy).is_some())
}

/// Can break ice: an unlinked unit that has not attacked (and has not moved, unless it
/// dashes), without AutoFreeze, next to breakable ice
pub(crate) fn can_break_ice(g: &Game, t: &Tables, u: &Unit) -> bool {
    if u.leader != 0
        || u.moved && !unit_has(t, u.kind, "dash")
        || u.attacked
        || unit_has(t, u.kind, "autofreeze")
    {
        return false;
    }
    neighbours(g, u.coordinates).any(|c| breakable(g, t, c, u.owner))
}

/// The BreakIce command: the unit is spent (an invisible one revealed) and the ice around it breaks.
pub(crate) fn break_ice_command(
    g: &mut Game,
    p: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    crate::step::base(out, p, 18);
    if let Some(n) = spend(g, c) {
        if crate::menu::unit_effect_list(g, g.units[n as usize].effects).contains(&INVISIBLE) {
            out.push(Action::MoveEffect(MoveEffect::Reveal {
                player: p,
                coordinates: c,
                origin: NULL,
                show_popup: false,
            }));
        }
    }
    out.push(Action::TurnEffect(TurnEffect::BreakIceArea {
        player: p,
        coordinates: c,
        radius: 1,
    }));
    Ok(())
}

pub(crate) fn break_ice_area(
    g: &Game,
    t: &Tables,
    p: i32,
    center: Xy,
    radius: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let area = crate::explore::area_sorted(g, center, radius)?.to_vec();
    if area.is_empty() {
        return Ok(());
    }
    out.push(Action::UpdateCityConnections {
        actor: p,
        players: Seats::from_iter([p]),
    });
    out.push(Action::UpdateRoutes { player: p });
    for &i in area.iter().rev() {
        let c = g.tiles[i].coordinates;
        if breakable(g, t, c, p) {
            out.push(Action::TurnEffect(TurnEffect::BreakIce {
                player: p,
                coordinates: c,
            }));
        }
    }
    Ok(())
}

/// Break ice: a hostile invisible unit in the way is revealed instead;
/// breakable ice becomes ocean, or water beside land, with the default skin
pub(crate) fn break_ice(
    g: &mut Game,
    t: &Tables,
    p: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let i = g.index(c).ok_or("break ice tile")?;
    if let Some(n) = g.tiles[i].unit {
        let u = g.units[n as usize];
        if crate::menu::unit_effect_list(g, u.effects).contains(&INVISIBLE) {
            player_index(g, p)?;
            if u.owner != p && relation_state(g, p, u.owner)? != PEACE {
                out.push(Action::MoveEffect(MoveEffect::Reveal {
                    player: p,
                    coordinates: c,
                    origin: NULL,
                    show_popup: true,
                }));
                return Ok(());
            }
        }
    }
    if breakable(g, t, c, p) {
        g.tiles[i].terrain = OCEAN;
        for d in [
            Xy { x: c.x - 1, y: c.y },
            Xy { x: c.x, y: c.y - 1 },
            Xy { x: c.x, y: c.y + 1 },
            Xy { x: c.x + 1, y: c.y },
        ] {
            if g.tile(d)
                .is_some_and(|x| [FIELD, MOUNTAIN, FOREST].contains(&x.terrain))
            {
                g.tiles[i].terrain = WATER;
                break;
            }
        }
        g.tiles[i].skin = 0;
    }
    ice_bank_levels(g, t, out)?;
    crate::mapgen::candidate::generate_shorelines(g);
    Ok(())
}

/// The ice bank level: one level per 20 tiles of ice or Polaris-climate land, up to the
/// ice bank's maxLevel
pub(crate) fn ice_bank_level(g: &Game, t: &Tables) -> Result<i32, String> {
    let cap = t
        .improvements
        .get(ICE_BANK as usize)
        .ok_or("no ice bank in the catalog")?
        .max_level
        .unwrap_or(0);
    let count = g
        .tiles
        .iter()
        .filter(|x| {
            x.terrain == ICE || ![WATER, OCEAN].contains(&x.terrain) && x.climate == POLARIS
        })
        .count();
    Ok(((count as f32 / 20.0_f32).floor() as i32).min(cap))
}

/// Ice bank levelling: every ice bank on the board moves to the current level, one
/// level action at a time, in tile order, for the tile's owner
pub(crate) fn ice_bank_levels(g: &Game, t: &Tables, out: &mut Vec<Action>) -> Result<(), String> {
    let banks: Vec<(Xy, i32, i32)> = g
        .tiles
        .iter()
        .filter_map(|x| {
            x.improvement
                .as_ref()
                .filter(|m| m.kind == ICE_BANK)
                .map(|m| (x.coordinates, x.owner, m.level))
        })
        .collect();
    if banks.is_empty() || t.improvements.get(ICE_BANK as usize).is_none() {
        return Ok(());
    }
    let level = ice_bank_level(g, t)?;
    for (target, player, current) in banks {
        if level < current {
            for _ in 0..current.wrapping_sub(level) {
                out.push(Action::Economy(Economy::ImprovementLevelDown {
                    player,
                    target,
                    amount: 1,
                }));
            }
        } else {
            for _ in 0..level.wrapping_sub(current) {
                out.push(Action::Economy(Economy::ImprovementLevelUp {
                    player,
                    target,
                }));
            }
        }
    }
    Ok(())
}

/// Restore climate: a tile of a spreading climate goes back to player `i`'s climate,
/// if it is land (not ice, unless `i`'s tribe spreads too, and then only on another skin)
pub(crate) fn restore_climate(
    g: &mut Game,
    t: &Tables,
    i: usize,
    c: Xy,
) -> Result<Option<Action>, String> {
    let tile = g.tile(c).ok_or("climate tile")?;
    let (climate, terrain, skin) = (tile.climate, tile.terrain, tile.skin);
    if !spreading(t, climate) {
        return Ok(None);
    }
    settle_climate(g, i);
    let mine = g.players[i].climate;
    let land = [FIELD, MOUNTAIN, FOREST, ICE].contains(&terrain);
    if !spreading(t, mine) {
        if !land || terrain == ICE {
            return Ok(None);
        }
    } else if !land || skin == g.players[i].skin {
        return Ok(None);
    }
    Ok(Some(Action::TurnEffect(TurnEffect::ChangeClimate {
        player: g.players[i].action.id,
        coordinates: c,
        climate: mine,
    })))
}

/// The climate step of a city area that `actor` (roster index `i`) takes over, per tile from the
/// far end of the sorted area: a spreading tribe freezes it
/// (Polaris) or changes its climate; any other restores a spread climate. The economy's
/// surrounding-area children follow each tile.
pub(crate) fn claim_area(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    i: usize,
    area: &[usize],
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let spreads = spreading(t, g.players[i].tribe);
    for &k in area {
        let c = g.tiles[k].coordinates;
        if spreads {
            if g.players[i].tribe == POLARIS {
                out.push(Action::Combat(Combat::FreezeTile {
                    player: actor,
                    coordinates: c,
                }));
            } else {
                settle_climate(g, i);
                out.push(Action::TurnEffect(TurnEffect::ChangeClimate {
                    player: actor,
                    coordinates: c,
                    climate: g.players[i].climate,
                }));
            }
        } else if spreading(t, g.tiles[k].climate) {
            if let Some(a) = restore_climate(g, t, i, c)? {
                out.push(a);
            }
        }
        crate::economy::surrounding(g, t, actor, c, out)?;
    }
    Ok(())
}

/// Climate change: the tile takes the climate and the player's skin (Swamped joins a
/// flooded tile on the Swamp skin)
pub(crate) fn change_climate(g: &mut Game, p: i32, c: Xy, climate: i32) -> Result<(), String> {
    let pi = player_index(g, p)?;
    let skin = g.players[pi].skin;
    let i = g.index(c).ok_or("climate change tile")?;
    let tile = &mut g.tiles[i];
    tile.climate = climate;
    tile.skin = skin;
    if tile.effects.contains(&FLOODED) && skin == 17 && !tile.effects.contains(&SWAMPED) {
        tile.effects.try_push(SWAMPED)?;
    }
    Ok(())
}
