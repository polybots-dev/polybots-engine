//! Rule queries: values a player reads off the screen that
//! need a rules computation. Each is one pass over the state for one viewer. It writes small
//! integers into the caller's buffers, one entry per tile at `y * width + x` (`UNKNOWN` where
//! the query has no answer for the viewer), and never writes to the game. A caller pays only
//! for the queries it asks for. The attack preview and the embark flag of a move are on the
//! legal list's entries (`menu::Cand::details`).
//!
//! Some queries show what the view hides, as the game's screen does; a caller that wants
//! none of it leaves them out:
//! - `shore_edges`, `border_edges`, `route_directions`, `territory`: the terrain, owner or
//!   route of an unexplored neighbour;
//! - `city_siege`, `city_work`, `income`, `city_units` and `tentacles` read the game as the
//!   viewer is shown it (`as_shown`): units hidden from the viewer are absent, so they show
//!   none of them;
//! - `can_act`: ice on an unexplored tile next to a unit, should a unit ever stand next to one
//!   (the game reveals the tiles around each unit), and an invisible unit of a player at peace
//!   on ice next to it, which makes the ice unbreakable;
//! - `income`: an embassy's income reads the foreign capital;
//! - `city_score`: a foreign city's population progress, which the view keeps to own cities;
//! - `unit_stats`: a foreign unit's effects, and its owner's techs through its defence bonus;
//! - `names`: the home city of a unit, which may be unexplored.

// a tile's index addresses the board and every output buffer alike
#![allow(clippy::needless_range_loop)]

use crate::state::{Game, Unit, Xy};
use crate::tables::Tables;
pub use crate::view::UNKNOWN;
use std::borrow::Cow;

const CITY: i32 = 1;
const BRIDGE: i32 = 48;
const FROZEN: i64 = 0;
const POISONED: i64 = 1;
const BOOSTED: i64 = 2;
const NATURE: i32 = 255;
/// the Tentacle tile effect
const TENTACLE: i32 = 3;

fn start(out: &mut Vec<i16>, g: &Game) {
    out.clear();
    out.resize(g.tiles.len(), UNKNOWN);
}

fn explored(g: &Game, i: usize, viewer: i32) -> bool {
    g.tiles[i].explorers.contains(&viewer)
}

fn unit_has(t: &Tables, kind: i32, a: &str) -> bool {
    t.units
        .get(kind as usize)
        .is_some_and(|d| d.unit_abilities.has(a))
}

/// The unit on tile `i` that has an info panel for `viewer`: one the viewer sees, alive and
/// not a temporary explorer.
pub(crate) fn panel_unit(g: &Game, i: usize, viewer: i32) -> Option<&Unit> {
    crate::menu::visible_unit(g, &g.tiles[i], viewer)
        .filter(|u| u.health != 0 && !u.is_temporary_explorer)
}

/// A unit's info panel: the displayed figures behind its statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Panel {
    /// health as the panel displays it
    pub health: i16,
    pub max_health: i16,
    /// added to the unit kind's attack (in the table's unit, tenths of the displayed figure)
    pub attack_modifier: i16,
    /// movement with the unit's effects applied
    pub movement: i16,
    /// the defence bonus in tenths (10: none)
    pub defence: i16,
}

/// The info panel of `u` (the unit kind's base figures are in the rule tables).
pub(crate) fn panel(g: &Game, t: &Tables, u: &Unit) -> Result<Panel, String> {
    let d = t.units.get(u.kind as usize).ok_or("unit kind")?;
    let max = crate::combat::max_health(g, t, u)?;
    let fx = crate::menu::unit_effect_list(g, u.effects);
    let mut movement = d.movement + i32::from(fx.contains(&BOOSTED));
    if fx.contains(&6) || fx.contains(&4) {
        movement += 1;
    }
    if fx.contains(&POISONED) && movement > 1 {
        movement -= 1;
    }
    Ok(Panel {
        health: (u.health as f32 * 0.1_f32).ceil() as u16 as i16,
        max_health: (max as f32 * 0.1_f32).ceil() as i32 as i16,
        attack_modifier: if fx.contains(&BOOSTED) { 50 } else { 0 },
        movement: movement as i16,
        defence: crate::combat::defence_bonus(g, t, u)? as i16,
    })
}

/// The planes `unit_stats` writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnitStats {
    pub health: Vec<i16>,
    pub max_health: Vec<i16>,
    pub attack_modifier: Vec<i16>,
    pub movement: Vec<i16>,
    pub defence: Vec<i16>,
}

/// `unit_stats`: the info panel of every unit the viewer sees that has one.
pub fn unit_stats(g: &Game, t: &Tables, viewer: i32, out: &mut UnitStats) -> Result<(), String> {
    for plane in [
        &mut out.health,
        &mut out.max_health,
        &mut out.attack_modifier,
        &mut out.movement,
        &mut out.defence,
    ] {
        start(plane, g);
    }
    for i in 0..g.tiles.len() {
        if let Some(u) = panel_unit(g, i, viewer) {
            let p = panel(g, t, u)?;
            out.health[i] = p.health;
            out.max_health[i] = p.max_health;
            out.attack_modifier[i] = p.attack_modifier;
            out.movement[i] = p.movement;
            out.defence[i] = p.defence;
        }
    }
    Ok(())
}

/// What `can_act` works out once per call and only when a unit needs it.
struct ActScan {
    /// the player abilities the viewer's techs unlock (`Tables::abilities`)
    abilities: u128,
}

/// Whether the turn-progress display counts own unit `n` as able to act
fn unit_can_act(g: &Game, t: &Tables, n: u32, scan: &mut ActScan) -> Result<bool, String> {
    let u = g.units[n as usize];
    let fx = crate::menu::unit_effect_list(g, u.effects);
    let can_move = !u.moved && !fx.contains(&FROZEN);
    let can_attack =
        !u.attacked && !fx.contains(&FROZEN) && (!u.moved || unit_has(t, u.kind, "dash"));
    if u.leader == 0
        && (u.health < crate::combat::max_health(g, t, &u)? || fx.contains(&POISONED))
        && can_move
        && can_attack
    {
        return Ok(true);
    }
    if unit_has(t, u.kind, "heal") && can_move && can_attack {
        let c = u.coordinates;
        for x in c.x - 1..=c.x + 1 {
            for y in c.y - 1..=c.y + 1 {
                let at = Xy { x, y };
                if at == c {
                    continue;
                }
                if let Some(m) = g.tile(at).and_then(|x| x.unit).map(|m| g.units[m as usize]) {
                    if m.owner == u.owner
                        && (crate::combat::max_health(g, t, &m)? > m.health
                            || crate::menu::unit_effect_list(g, m.effects).contains(&POISONED))
                    {
                        return Ok(true);
                    }
                }
            }
        }
    }
    let selected = u.passenger.map_or(u, |p| g.units[p as usize]);
    let sd = t.units.get(selected.kind as usize).ok_or("unit kind")?;
    if !unit_has(t, selected.kind, "static") && !sd.hidden {
        let discipline = selected.owner == u.owner
            && crate::tables::unlocks(scan.abilities, "discipline")
            || selected.owner != u.owner
                && crate::menu::player_ability(g, t, selected.owner, "discipline");
        let limit = 3 ^ i32::from(discipline);
        if limit > 0 && selected.xp >= limit && selected.promotion_level == 0 {
            return Ok(true);
        }
    }
    // the special tribes' abilities, in the game's order: FreezeArea, BreakIce, Explode
    if crate::polaris::can_freeze_area(g, t, &u)?
        || crate::polaris::can_break_ice(g, t, &u)
        || crate::cymanti::can_explode(g, t, &u)
    {
        return Ok(true);
    }
    if g.player(u.owner).is_some()
        && crate::tables::unlocks(scan.abilities, "disband")
        && !u.moved
        && !u.attacked
    {
        return Ok(true);
    }
    if unit_has(t, u.kind, "grow") {
        return Ok(false);
    }
    // the upgradable units (through the tribe's overrides): an affordable one
    let currency = g.player(u.owner).ok_or("upgrade player")?.action.currency;
    let mut affordable = false;
    crate::units::upgradable(g, t, u.owner, u.kind, |id| {
        affordable = t.units.get(id).is_some_and(|d| d.cost <= currency);
        !affordable
    })?;
    if affordable {
        return Ok(true);
    }
    // (a promotable unit already returned true above)
    let dash = !u.attacked && u.moved && unit_has(t, u.kind, "dash");
    if u.moved && !dash {
        return Ok(false);
    }
    let range = t.units.get(u.kind as usize).ok_or("unit kind")?.range;
    let targets = crate::combat::attack_options(g, t, &u, range, false)?;
    Ok(!u.moved || !targets.is_empty())
}

/// `can_act`: 1 or 0 for each of the viewer's own units, whether the game counts it as still
/// able to act this turn.
pub fn can_act(g: &Game, t: &Tables, viewer: i32, out: &mut Vec<i16>) -> Result<(), String> {
    start(out, g);
    // every unit asked about is the viewer's own
    let mut scan = ActScan {
        abilities: crate::menu::player_abilities(g, t, viewer),
    };
    for i in 0..g.tiles.len() {
        let tile = &g.tiles[i];
        if crate::menu::visible_unit(g, tile, viewer).is_none_or(|u| u.owner != viewer) {
            continue;
        }
        let node = tile.unit.ok_or("own unit missing")?;
        out[i] = i16::from(unit_can_act(g, t, node, &mut scan)?);
    }
    Ok(())
}

/// The game as `viewer` is shown it: every unit the viewer cannot see (`menu::visible_unit`:
/// on a tile it has not explored, or invisible and another player's not at peace with it)
/// taken off the board. Borrowed when there is none, which is most of the
/// time. For what the viewer is shown only; the rules read the real game.
pub fn as_shown(g: &Game, viewer: i32) -> Cow<'_, Game> {
    let hidden = |x: &crate::state::Tile| {
        x.unit.is_some() && crate::menu::visible_unit(g, x, viewer).is_none()
    };
    if !g.tiles.iter().any(hidden) {
        return Cow::Borrowed(g);
    }
    let mut shown = g.clone();
    for (x, y) in shown.tiles.iter_mut().zip(&g.tiles) {
        if hidden(y) {
            x.unit = None;
        }
    }
    Cow::Owned(shown)
}

/// The queries that read units, for the viewer: each runs on the game as the viewer is
/// shown it (`as_shown`), so no unit hidden from the viewer counts.
pub fn city_work(g: &Game, t: &Tables, viewer: i32, out: &mut Vec<i16>) -> Result<(), String> {
    city_work_on(&as_shown(g, viewer), t, viewer, out)
}

pub fn income(g: &Game, t: &Tables, viewer: i32) -> Result<i32, String> {
    income_on(&as_shown(g, viewer), t, viewer)
}

/// Another player's unit homed in the viewer's city counts, as the game's population slots
/// show it.
pub fn city_units(g: &Game, viewer: i32, out: &mut Vec<i16>) {
    city_units_on(&as_shown(g, viewer), viewer, out)
}

/// Only whether the city is under siege, not by whom.
pub fn city_siege(g: &Game, viewer: i32, out: &mut Vec<i16>) {
    city_siege_on(&as_shown(g, viewer), viewer, out, None)
}

/// The city on tile `i` as the viewer's board shows one.
fn shown_city(g: &Game, i: usize, viewer: i32) -> Option<&crate::state::Improvement> {
    g.tiles[i]
        .improvement
        .as_ref()
        .filter(|m| m.kind == CITY && explored(g, i, viewer))
}

/// `city_work`: the stars per turn each city the viewer sees gives the viewer, as the city
/// displays them (0: none shown). A city another player's unit stands in gives nothing.
fn city_work_on(g: &Game, t: &Tables, viewer: i32, out: &mut Vec<i16>) -> Result<(), String> {
    start(out, g);
    for i in 0..g.tiles.len() {
        let Some(m) = shown_city(g, i, viewer) else {
            continue;
        };
        out[i] = if m.effects.contains(&1) {
            0
        } else {
            crate::turns::player_work(g, t, g.tiles[i].coordinates, viewer, m.level)?.max(0) as i16
        };
    }
    Ok(())
}

/// `income`: the viewer's stars per turn (every improvement tile in map
/// order at its stored level). Not the turn-start income, which turns.rs computes.
fn income_on(g: &Game, t: &Tables, viewer: i32) -> Result<i32, String> {
    let mut income = 0;
    for x in &g.tiles {
        if let Some(m) = &x.improvement {
            income += crate::turns::player_work(g, t, x.coordinates, viewer, m.level)?;
        }
    }
    Ok(income)
}

/// `city_score`: the score of each city the viewer sees.
pub fn city_score(g: &Game, t: &Tables, viewer: i32, out: &mut Vec<i32>) {
    out.clear();
    out.resize(g.tiles.len(), i32::from(UNKNOWN));
    for (i, slot) in out.iter_mut().enumerate() {
        if shown_city(g, i, viewer).is_some() {
            *slot = crate::economy::improvement_score(t, g, i);
        }
    }
}

/// `city_units`: for each of the viewer's own cities, how many of its population slots
/// (level + 1) its units take, as its nameplate shows.
fn city_units_on(g: &Game, viewer: i32, out: &mut Vec<i16>) {
    start(out, g);
    for i in 0..g.tiles.len() {
        let Some(m) = shown_city(g, i, viewer).filter(|_| g.tiles[i].owner == viewer) else {
            continue;
        };
        let (c, slots) = (g.tiles[i].coordinates, m.level + 1);
        let mut used = 0;
        for n in g.tiles.iter().filter_map(|x| x.unit) {
            if g.units[n as usize].home == c {
                used += 1;
                if used == slots {
                    break;
                }
            }
        }
        out[i] = used as i16;
    }
}

/// `city_siege`: 1 on an explored, owned city another player's unit stands in (the game draws
/// it under siege even when the viewer cannot see the unit), else 0; `tribe`, if asked for,
/// gets the besieger's tribe (0: none, or a unit of no player).
fn city_siege_on(g: &Game, viewer: i32, out: &mut Vec<i16>, mut tribe: Option<&mut Vec<i16>>) {
    start(out, g);
    if let Some(tribe) = tribe.as_deref_mut() {
        start(tribe, g);
    }
    for (i, x) in g.tiles.iter().enumerate() {
        if !explored(g, i, viewer) {
            continue;
        }
        let owned = x.owner != 0 && x.owner != NATURE;
        let besieger = x
            .unit
            .filter(|_| owned && x.improvement.as_ref().is_some_and(|m| m.kind == CITY))
            .and_then(|n| g.units.get(n as usize))
            .filter(|u| u.owner != x.owner);
        out[i] = i16::from(besieger.is_some());
        if let Some(tribe) = tribe.as_deref_mut() {
            tribe[i] = besieger
                .and_then(|u| g.player(u.owner))
                .map_or(0, |p| p.tribe as i16);
        }
    }
}

/// bits of `shore_edges` and `border_edges`
pub const NORTH: i16 = 1;
pub const SOUTH: i16 = 2;
pub const EAST: i16 = 4;
pub const WEST: i16 = 8;
/// the four sides in bit order, north being y + 1
pub const SIDES: [(i16, i32, i32); 4] =
    [(NORTH, 0, 1), (SOUTH, 0, -1), (EAST, 1, 0), (WEST, -1, 0)];

/// `shore_edges`: the sides of each explored tile the game draws a shoreline on, as bits.
/// UNKNOWN on an unexplored tile and on one without a shoreline record.
pub fn shore_edges(g: &Game, viewer: i32, out: &mut Vec<i16>) {
    start(out, g);
    for i in (0..g.tiles.len()).filter(|&i| explored(g, i, viewer)) {
        if let Some(s) = &g.side.shorelines[i] {
            let sides = [
                (&s.north, NORTH),
                (&s.south, SOUTH),
                (&s.east, EAST),
                (&s.west, WEST),
            ];
            out[i] = sides
                .iter()
                .filter(|(side, _)| s.any && side.visible)
                .map(|&(_, bit)| bit)
                .sum();
        }
    }
}

/// `border_edges`: the sides of each explored tile where its owner's territory ends, as bits.
pub fn border_edges(g: &Game, viewer: i32, out: &mut Vec<i16>) {
    start(out, g);
    for (i, x) in g.tiles.iter().enumerate() {
        if !explored(g, i, viewer) {
            continue;
        }
        let (c, owned) = (x.coordinates, x.owner != 0 && x.owner != NATURE);
        out[i] = SIDES
            .iter()
            .filter(|&&(_, dx, dy)| {
                owned
                    && g.tile(Xy {
                        x: c.x + dx,
                        y: c.y + dy,
                    })
                    .is_none_or(|n| n.owner != x.owner)
            })
            .map(|&(bit, _, _)| bit)
            .sum();
    }
}

/// The eight directions of `route_directions`, by bit: west and south first, clockwise from
/// the south-west as the game numbers them.
pub const DIRECTIONS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
];

/// `route_directions`: the directions in which each explored tile's road or route is drawn
/// toward a neighbour, as bits by `DIRECTIONS` (a lone road is drawn toward direction 7).
pub fn route_directions(
    g: &Game,
    t: &Tables,
    viewer: i32,
    out: &mut Vec<i16>,
) -> Result<(), String> {
    start(out, g);
    let mut links = crate::routes::Links::new(g.tiles.len());
    for (i, x) in g.tiles.iter().enumerate() {
        if !explored(g, i, viewer) {
            continue;
        }
        let kind = x.improvement.as_ref().map(|m| m.kind);
        let opener = match kind {
            Some(k) => crate::routes::router(t, k)?.is_some_and(|r| r.terrain != 0),
            None => false,
        };
        let road = x.has_road || kind == Some(CITY) || kind == Some(BRIDGE);
        let mut shown = 0;
        if kind != Some(BRIDGE) && (x.has_route || road || opener) {
            for (d, (dx, dy)) in DIRECTIONS.iter().enumerate() {
                let to = g.index(Xy {
                    x: x.coordinates.x + dx,
                    y: x.coordinates.y + dy,
                });
                let Some(n) = to else { continue };
                // a tile with no road, route or improvement carries no connection
                let other = &g.tiles[n];
                if !other.has_road && !other.has_route && other.improvement.is_none() {
                    continue;
                }
                if links.open(g, t, i, n)? {
                    shown |= 1 << d;
                }
            }
        }
        if shown == 0 && road && kind != Some(CITY) && !opener {
            shown = 1 << 7;
        }
        out[i] = shown;
    }
    Ok(())
}

/// bits of `territory`'s edges
pub const EDGE_WEST: i16 = 1;
pub const EDGE_EAST: i16 = 2;
pub const EDGE_SOUTH: i16 = 4;
pub const EDGE_NORTH: i16 = 8;

/// `territory`: the border the game draws around each city the viewer sees whose owner is a
/// player. `city` gets, for each tile inside one, the city's tile; `edges` the sides of the
/// tile the border runs along. Tiles of the territory the viewer has not explored are
/// included.
pub fn territory(
    g: &Game,
    viewer: i32,
    city: &mut Vec<i16>,
    edges: &mut Vec<i16>,
) -> Result<(), String> {
    start(city, g);
    start(edges, g);
    for i in 0..g.tiles.len() {
        let Some(m) = shown_city(g, i, viewer) else {
            continue;
        };
        let (c, owner) = (g.tiles[i].coordinates, g.tiles[i].owner);
        if owner == 0 || g.player(owner).is_none() {
            continue;
        }
        let inside = |at: Xy| {
            g.tile(at)
                .is_some_and(|o| o.owner == owner && o.ruling_city == c)
        };
        // the square of the city's border size (the game walks it sorted by distance;
        // the order does not matter here)
        let radius = m.border_size;
        if radius > 5 {
            return Err("area radius above 5".into());
        }
        for y in c.y - radius..=c.y + radius {
            for x in c.x - radius..=c.x + radius {
                let Some(j) = g.index(Xy { x, y }).filter(|_| inside(Xy { x, y })) else {
                    continue;
                };
                city[j] = i as i16;
                edges[j] = [
                    (EDGE_WEST, -1, 0),
                    (EDGE_EAST, 1, 0),
                    (EDGE_SOUTH, 0, -1),
                    (EDGE_NORTH, 0, 1),
                ]
                .iter()
                .filter(|&&(_, dx, dy)| {
                    !inside(Xy {
                        x: x + dx,
                        y: y + dy,
                    })
                })
                .map(|&(bit, _, _)| bit)
                .sum();
            }
        }
    }
    Ok(())
}

/// The name of the city on tile `i`, if it has one.
pub(crate) fn city_name(g: &Game, i: usize) -> Option<&str> {
    g.side.names[i].as_deref().filter(|n| !n.is_empty())
}

/// The name of `u`'s home city, if its home is a city with a name.
pub(crate) fn home_name<'a>(g: &'a Game, u: &Unit) -> Option<&'a str> {
    g.index(u.home)
        .filter(|&h| {
            g.tiles[h]
                .improvement
                .as_ref()
                .is_some_and(|m| m.kind == CITY)
        })
        .and_then(|h| city_name(g, h))
}

/// `names`: the name of each city the viewer sees (`city`) and of the home city of each unit
/// with an info panel (`home`), as ids from 1 that are equal exactly when the text is; 0 is
/// no name. The ids mean nothing beyond one call.
pub fn names(g: &Game, viewer: i32, city: &mut Vec<i16>, home: &mut Vec<i16>) {
    start(city, g);
    start(home, g);
    fn id<'a>(seen: &mut Vec<&'a str>, name: Option<&'a str>) -> i16 {
        let Some(name) = name else { return 0 };
        let at = seen.iter().position(|n| *n == name).unwrap_or_else(|| {
            seen.push(name);
            seen.len() - 1
        });
        at as i16 + 1
    }
    let mut seen: Vec<&str> = Vec::new();
    for i in 0..g.tiles.len() {
        if shown_city(g, i, viewer).is_some() {
            city[i] = id(&mut seen, city_name(g, i));
        }
        if let Some(u) = panel_unit(g, i, viewer) {
            home[i] = id(&mut seen, home_name(g, u));
        }
    }
}

/// The neighbours `tentacles` reports, in the game's order: SW, W, NW, N, NE, E, SE, S.
pub const TENTACLE_DIRECTIONS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
];

/// `tentacles`: per explored tile, 1 in `zone` if it has the Tentacle effect, and in `from`
/// one bit per `TENTACLE_DIRECTIONS` entry whose tile holds a unit with Tentacles that is not
/// frozen. The neighbour's unit is read whether or not the viewer sees it;
/// the public query reads the game as the viewer is shown it (`as_shown`).
pub fn tentacles(g: &Game, t: &Tables, viewer: i32, zone: &mut Vec<i16>, from: &mut Vec<i16>) {
    tentacles_on(&as_shown(g, viewer), t, viewer, zone, from)
}

fn tentacles_on(g: &Game, t: &Tables, viewer: i32, zone: &mut Vec<i16>, from: &mut Vec<i16>) {
    start(zone, g);
    start(from, g);
    for i in 0..g.tiles.len() {
        if !explored(g, i, viewer) {
            continue;
        }
        let x = &g.tiles[i];
        let on = x.effects.contains(&TENTACLE);
        zone[i] = i16::from(on);
        from[i] = 0;
        if !on {
            continue;
        }
        for (slot, &(dx, dy)) in TENTACLE_DIRECTIONS.iter().enumerate() {
            let at = Xy {
                x: x.coordinates.x + dx,
                y: x.coordinates.y + dy,
            };
            let Some(n) = g.tile(at).and_then(|n| n.unit) else {
                continue;
            };
            let u = &g.units[n as usize];
            if unit_has(t, u.kind, "tentacles")
                && !crate::menu::unit_effect_list(g, u.effects).contains(&FROZEN)
            {
                from[i] |= 1 << slot;
            }
        }
    }
}

/// The techs of `tribe`'s tech tree, its overrides resolved, in the game's order (breadth-first;
/// from tech 0; the Genius task counts them).
pub fn tribe_tech(t: &Tables, tribe: i32) -> Result<&[usize], String> {
    crate::menu::tribe_tech(t, tribe)
}

/// The outcome of the game: the opponent of a resigned player (two-player games), or the
/// winner check's choice once the match has ended; None while the game goes on.
pub fn winner(g: &Game) -> Result<Option<i32>, String> {
    let playable = || {
        g.players
            .iter()
            .map(|p| p.action.id)
            .filter(|&p| p != NATURE)
    };
    if let Some(r) = g.players.iter().find(|p| p.resigned_at_command >= 0) {
        if playable().count() != 2 {
            return Err("resignation outcome beyond two players not implemented".into());
        }
        return Ok(playable().find(|&p| p != r.action.id));
    }
    if g.current_state == 4 {
        let (selected, _) = crate::turns::winner(&mut g.clone())?;
        return Ok(Some(selected));
    }
    Ok(None)
}
