//! Command execution: one decision's command applied to the state, as the game command manager
//! runs it.
//!
//! The command's wire form is appended to `game.commands`; the manager then alternates between the
//! action stack (last entry first, children appended while the parent is still present, the parent
//! removed at its saved index) and the next unexecuted command, until both are empty or the current
//! player has a pending trigger. Queued actions keep the game's JSON form, because the ones still
//! queued at a decision boundary are part of the state.
//!
//! Exact-or-refuse: a command, action or branch not ported yet returns Err and leaves the caller to
//! discard the state; nothing is approximated.

use crate::action::{
    Action, City, Combat, Command, Diplomacy, Economy, MoveEffect, Payload, TurnEffect,
};
use crate::inline::Seats;
use crate::menu::{tech_price, tribe_tech};
use crate::state::{Game, Relation, Xy, COOL_DOWN, PEACE};
use crate::tables::Tables;

const PEACE_REQUEST_MESSAGE: i64 = 1;
const PEACE_REQUEST_TRIGGER: i64 = 2;
const POISONED: i64 = 1;
const NATURE: i32 = 255;
const FROZEN: i64 = 0;
const STARTED: i64 = 2;
const CITY: i32 = 1;
const WATER: i32 = 1;
const ICE: i32 = 6;
const BUBBLE: i64 = 4;
const OCEAN: i32 = 2;
const FOREST: i32 = 5;

pub(crate) fn player_index(g: &Game, id: i32) -> Result<usize, String> {
    g.players
        .iter()
        .position(|p| p.action.id == id)
        .ok_or_else(|| format!("missing player {id}"))
}

pub(crate) fn mover(g: &Game) -> i32 {
    g.players[g.current_player_index as usize].action.id
}

/// The gameplay hash, materialized from the seed on first use
pub(crate) fn random_hash(g: &mut Game) -> Result<crate::random::HashFunction, String> {
    let seed = g.seeds.gameplay_seed;
    let hash = g
        .seeds
        .random_hash
        .get_or_insert(crate::state::HashSeed { seed });
    Ok(crate::random::HashFunction(hash.seed as i32))
}

/// A stale unlocked-tech cache is refilled with the known techs, as an ability lookup does
pub(crate) fn refresh_tech_cache(g: &mut Game, t: &Tables, id: i32) -> Result<(), String> {
    let i = player_index(g, id)?;
    let p = &mut g.players[i];
    let cache = p
        .unlocked_tech_cache
        .as_ref()
        .ok_or("null unlocked tech cache")?;
    if *cache != p.available_tech {
        let fresh = p
            .available_tech
            .iter()
            .copied()
            .filter(|&k| t.techs.get(k as usize).is_some())
            .collect();
        p.unlocked_tech_cache = Some(fresh);
    }
    Ok(())
}

/// player `i`'s unlocked-tech cache as stored (the game's lazily refreshed copy)
pub(crate) fn tech_cache(g: &Game, i: usize) -> Result<&[i32], String> {
    g.players[i]
        .unlocked_tech_cache
        .as_deref()
        .ok_or_else(|| "null unlocked tech cache".into())
}

/// Relation lookup that inserts a default record for a missing opponent
pub(crate) fn relation_entry(g: &mut Game, i: usize, other: i32) -> Result<&mut Relation, String> {
    // the game never stores a relation toward player 0 (it returns a fresh default instead)
    if other == 0 {
        return Err("relation toward player 0".into());
    }
    let list = &mut g.players[i].relations;
    let n = match list.iter().position(|(id, _)| *id == other) {
        Some(n) => n,
        None => {
            list.push((other, Relation::DEFAULT));
            list.len() - 1
        }
    };
    Ok(&mut list[n].1)
}

/// a relation read through the game's inserting lookup; player 0 gives the default record
pub(crate) fn relation_of(g: &mut Game, i: usize, other: i32) -> Result<Relation, String> {
    if other == 0 {
        return Ok(Relation::DEFAULT);
    }
    relation_entry(g, i, other).copied()
}

/// player `p`'s relation state toward `q`, through the inserting lookup
pub(crate) fn relation_state(g: &mut Game, p: i32, q: i32) -> Result<i32, String> {
    if q == 0 {
        return Ok(Relation::DEFAULT.state);
    }
    let i = player_index(g, p)?;
    Ok(relation_of(g, i, q)?.state)
}

/// The current capital: the original capital if still owned, else the first owned city
pub(crate) fn current_capital(g: &Game, id: i32) -> Result<Xy, String> {
    let p = g.player(id).ok_or("missing player")?;
    let start = p.start_tile;
    if g.tile(start).ok_or("missing original capital tile")?.owner == id {
        return Ok(start);
    }
    Ok(g.tiles
        .iter()
        .find(|x| x.owner == id && x.improvement.as_ref().is_some_and(|m| m.kind == 1))
        .map_or(Xy { x: -1, y: -1 }, |x| x.coordinates))
}

/// Route children of a peace change: connections for both players, then both players' routes
fn route_children(out: &mut Vec<Action>, p: i32, q: i32) {
    out.extend([
        Action::UpdateCityConnections {
            actor: p,
            players: Seats::from_iter([p, q]),
        },
        Action::UpdateRoutes { player: p },
        Action::UpdateRoutes { player: q },
    ]);
}

pub(crate) fn base(out: &mut Vec<Action>, player: i32, command: u64) {
    out.extend([
        Action::Payload(Payload::EndCommand {
            player: player,
            command: command,
        }),
        Action::Payload(Payload::RunDeferred { player: player }),
    ]);
}

/// The state writes of validating a build: a relation record for a foreign unit on the tile
/// (the visible unit and the peace check), and the tech-cache refresh of the two terrain-requirement
/// ability queries (a Field requirement on Forest, or on water for most improvements).
fn build_validation_writes(
    g: &mut Game,
    t: &Tables,
    p: i32,
    kind: i32,
    c: Xy,
) -> Result<(), String> {
    let tile = g.tile(c).ok_or("build tile")?;
    let (terrain, explored) = (tile.terrain, tile.explorers.contains(&p));
    let occupant = tile
        .unit
        .and_then(|n| g.units.get(n as usize))
        .map(|u| u.owner)
        .filter(|&o| explored && o != p);
    if let Some(o) = occupant {
        let i = player_index(g, p)?;
        relation_entry(g, i, o)?;
    }
    let d = t
        .improvements
        .get(kind as usize)
        .ok_or("unknown improvement")?;
    for r in &d.terrain_requirements {
        if r["terrain"] == "field"
            && (terrain == FOREST
                || (terrain == WATER || terrain == OCEAN) && ![3, 17, 52, 14].contains(&kind))
        {
            refresh_tech_cache(g, t, p)?;
        }
    }
    Ok(())
}

/// Embassy cost: 4 + 2 per player (self and Nature included) with an embassy toward `p`; the
/// reverse-relation lookups insert default records
fn embassy_cost(g: &mut Game, p: i32) -> Result<i32, String> {
    let mut count = 0;
    for i in 0..g.players.len() {
        count += i32::from(relation_entry(g, i, p)?.embassy_level > 0);
    }
    Ok(count * 2 + 4)
}

/// Attack diplomacy: record the attack turn on both relations and queue removal of
/// both embassies
pub(crate) fn attack_diplomacy(
    g: &mut Game,
    actor: i32,
    opponent: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    if opponent == 0 {
        return Ok(());
    }
    let turn = g.current_turn;
    let a = player_index(g, actor)?;
    let r = relation_entry(g, a, opponent)?;
    r.previous_attack_turn = r.last_attack_turn;
    r.last_attack_turn = turn;
    let o = player_index(g, opponent)?;
    relation_entry(g, o, actor)?.previous_attack_turn = turn;
    out.extend([
        Action::DestroyEmbassy {
            player: actor,
            opponent: opponent,
        },
        Action::DestroyEmbassy {
            player: opponent,
            opponent: actor,
        },
    ]);
    Ok(())
}

/// The unit-graph node of the board unit with id `id`.
fn find_unit(g: &Game, id: u64) -> Option<u32> {
    g.tiles.iter().filter_map(|x| x.unit).find(|&n| {
        g.units
            .get(n as usize)
            .is_some_and(|u| u64::from(u.id) == id)
    })
}

/// Facing direction: the exact vector when adjacent (or NONE), else the weighted best of eight
pub(crate) fn facing_direction(from: Xy, to: Xy) -> i32 {
    let v = [
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
        (1, 0),
        (1, -1),
        (0, -1),
    ];
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    if dx.abs().max(dy.abs()) <= 1 {
        return v
            .iter()
            .position(|&d| d == (dx, dy))
            .map_or(8, |i| i as i32);
    }
    let (mut best, mut score) = (0, 0);
    for (i, &(x, y)) in v.iter().enumerate() {
        let weight = if x * x + y * y == 1 { 1393 } else { 985 };
        let candidate = (x * dx + y * dy) * weight;
        if score < candidate {
            score = candidate;
            best = i;
        }
    }
    best as i32
}

const LAND: [i32; 4] = [3, 4, 5, 6];

/// A Move action for the Command reason
fn move_action(
    g: &mut Game,
    t: &Tables,
    p: i32,
    unit: u32,
    path: &[Xy],
    reason: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    // reason 0 is a command, 1 the advance after a kill, 2 a push out of the way
    if !(0..=2).contains(&reason) {
        return Err("unknown move reason".into());
    }
    let Some(node) = find_unit(g, u64::from(unit)) else {
        return Ok(());
    };
    if player_index(g, p).is_err() {
        return Ok(());
    }
    let unit = g.units[node as usize].clone();
    let Some(data) = t.units.get(unit.kind as usize) else {
        return Ok(());
    };
    let (dest, origin) = (
        *path.first().ok_or("empty move path")?,
        *path.last().unwrap(),
    );
    let has = |a: &str| data.unit_abilities.has(a);
    if unit.kind == 13 {
        return Err("moving a Bunny not implemented".into());
    }
    // moved |= (reason != Push and, for an attack advance, the unit lacks Escape)
    if reason == 0 || reason == 1 && !has("escape") {
        g.units[node as usize].moved = true;
    }
    // a skater that ends off the ice is spent
    if has("skate") && reason != 2 && g.tile(dest).ok_or("move destination")?.terrain != ICE {
        g.units[node as usize].moved = true;
        g.units[node as usize].attacked = true;
    }
    let (from_i, to_i) = (
        g.index(origin).ok_or("move origin")?,
        g.index(dest).ok_or("move destination")?,
    );
    g.tiles[from_i].unit = None;
    g.tiles[to_i].unit = Some(node);
    g.units[node as usize].coordinates = dest;
    if path.len() > 1 {
        let d = facing_direction(path[1], dest);
        g.units[node as usize].direction = d;
        g.units[node as usize].flipped = d < 4;
    }
    out.extend(crate::cymanti::poison_tile(g, t, node, dest));
    refresh_tech_cache(g, t, p)?; // the player-ability query for the bubble check
    let bubbled = |g: &Game| {
        crate::menu::unit_effect_list(g, g.units[node as usize].effects).contains(&BUBBLE)
    };
    // an amphibious unit of a Bubble player gains Bubble on a flooded tile
    if crate::menu::player_ability(g, t, p, "bubble")
        && crate::aquarion::flooded(&g.tiles[to_i])
        && has("amphibious")
        && !bubbled(g)
    {
        out.push(Action::Combat(Combat::Bubble {
            player: mover(g),
            unit: unit.id,
            add: true,
        }));
    }
    if crate::aquarion::tentacle_tile(&g.tiles[to_i]) {
        crate::aquarion::strike(g, t, node, p, dest, out)?;
    }
    if reason != 2 && has("autofreeze") {
        out.push(Action::Combat(Combat::FreezeArea {
            player: p,
            coordinates: dest,
            radius: 1,
            freeze_units: true,
            only_owned: false,
        }));
    }
    if reason != 2 && (has("stomp") || has("tentacles")) {
        crate::attack::area_damage(g, t, node, dest, out)?;
    }
    if reason != 2 && has("autoheal") {
        out.push(Action::MoveEffect(MoveEffect::HealOthers {
            player: p,
            coordinates: dest,
        }));
    }
    if reason != 2 && has("autoflood") {
        out.push(Action::Combat(Combat::Flood {
            player: p,
            coordinates: dest,
        }));
    }
    // the Island leaves algae on the water it crosses (AlgaeSpawn at both rules versions)
    if reason != 2 && has("algae") {
        let spawn = t.improvements.id("algaespawn")? as i32;
        for &c in path {
            let x = g.tile(c).ok_or("algae path tile")?;
            if (x.terrain == WATER || x.terrain == OCEAN) && !crate::menu::effects(x).contains(&4) {
                out.push(Action::Economy(Economy::Build {
                    player: g.units[node as usize].owner,
                    kind: spawn,
                    target: c,
                    pay: false,
                }));
            }
        }
    }
    // ... and loses it on dry land
    let amphibious = has("swim") && data.movement_terrain.is_empty() && !has("carry");
    if bubbled(g)
        && LAND.contains(&g.tiles[to_i].terrain)
        && !crate::aquarion::wetland(&g.tiles[to_i])
        && amphibious
    {
        out.push(Action::Combat(Combat::Bubble {
            player: mover(g),
            unit: unit.id,
            add: false,
        }));
    }
    if has("tentacles") {
        crate::aquarion::update_tentacles(g, t)?;
    }
    let sight = 1 + i32::from(has("scout"));
    for &c in path {
        crate::explore::explore_from(g, p, c, sight, out)?;
    }
    // whether the unit embarks or disembarks at the destination
    let target = g.tiles[to_i].clone();
    let water =
        target.terrain == WATER || target.terrain == OCEAN || crate::aquarion::wetland(&target);
    let land_unit = !has("swim") && !has("fly") && unit.leader == 0 && unit.follower == 0;
    let mut embark = false;
    if land_unit
        && water
        && target.improvement.as_ref().is_some_and(|m| {
            t.improvements
                .get(m.kind as usize)
                .is_some_and(|d| d.improvement_abilities.has("embark"))
        })
        && crate::routes::can_embark(g, t, unit.owner)?
    {
        embark = target.owner == unit.owner
            || target.owner != 0 && {
                let i = player_index(g, unit.owner)?;
                relation_entry(g, i, target.owner)?.state == PEACE
            };
    }
    if embark {
        out.push(Action::MoveEffect(MoveEffect::Embark {
            player: p,
            coordinates: dest,
        }));
    } else if has("carry")
        && (target.improvement.as_ref().is_some_and(|m| m.kind == CITY)
            || LAND.contains(&target.terrain) && !crate::aquarion::wetland(&target))
    {
        out.push(Action::MoveEffect(MoveEffect::Disembark {
            player: p,
            coordinates: dest,
        }));
    }
    // a follower two or more tiles behind steps into the head's old tile
    let follower = g.units[node as usize].follower;
    if follower != 0 {
        if let Some(f) = crate::linked::find(g, follower) {
            let at = g.units[f as usize].coordinates;
            if (at.x - dest.x).abs().max((at.y - dest.y).abs()) >= 2 {
                out.push(Action::MoveEffect(MoveEffect::Move {
                    player: p,
                    unit: follower,
                    path: crate::action::Path::from_iter([origin, at]),
                    should_animate: true,
                    reason: 0,
                }));
            }
        }
    }
    if target.improvement.as_ref().is_some_and(|m| m.kind == CITY)
        && target.owner != 0
        && target.owner != p
    {
        attack_diplomacy(g, p, target.owner, out)?;
    }
    Ok(())
}

/// The chosen candidate as a command, with the game validator's state writes applied.
fn admit(g: &mut Game, t: &Tables, action: &engine_fast_frame::Action) -> Result<Command, String> {
    let p = mover(g);
    let place = |c: Option<engine_fast_frame::Xy>, what: &str| {
        c.map(|c| Xy { x: c.x, y: c.y })
            .ok_or_else(|| format!("action has no {what}"))
    };
    let source = || place(action.source, "source");
    let target = || place(action.target, "target");
    let option = || action.option.ok_or("action has no option");
    let opponent = || action.opponent.ok_or("action has no opponent");
    Ok(match action.kind {
        8 => {
            let technology = i64::from(option()?);
            if t.techs.get(technology as usize).map_or(0, |x| x.cost) != 0 {
                refresh_tech_cache(g, t, p)?;
            }
            Command::Research { technology }
        }
        27 => {
            let opponent = opponent()?;
            refresh_tech_cache(g, t, p)?;
            if opponent != 0 {
                let i = player_index(g, p)?;
                relation_entry(g, i, opponent)?;
            }
            // the game names the opponent's capital in the command; it must exist
            current_capital(g, opponent)?;
            Command::PeaceTreaty { opponent }
        }
        1 => {
            let (kind, target) = (option()?, target()?);
            build_validation_writes(g, t, p, kind, target)?;
            Command::Build { kind, target }
        }
        9 => Command::Destroy {
            coordinates: target()?,
        },
        29 => {
            let opponent = opponent()?;
            if opponent != 0 {
                let i = player_index(g, p)?;
                relation_entry(g, i, opponent)?;
            }
            current_capital(g, opponent)?;
            Command::BreakPeace { opponent }
        }
        28 => Command::PeaceRequestResponse {
            opponent: opponent()?,
            accepted: action.accept == Some(true),
        },
        6 => {
            let (from, to) = (source()?, target()?);
            let node = g
                .tile(from)
                .and_then(|x| x.unit)
                .ok_or("move with no unit")?;
            let unit = g.units[node as usize].clone();
            // Move validity: the unit can move and has data; the
            // path is checked where the Move searches it (`execute_command`)
            if unit.moved || crate::menu::unit_effect_list(g, unit.effects).contains(&FROZEN) {
                return Err("a Move by a unit that cannot move: not a legal move".into());
            }
            if t.units.get(unit.kind as usize).is_none() {
                return Err("a Move by a unit without unit data: not a legal move".into());
            }
            refresh_tech_cache(g, t, p)?;
            Command::Move {
                unit: unit.id,
                from,
                to,
            }
        }
        5 => {
            refresh_tech_cache(g, t, p)?;
            Command::Train {
                kind: option()?,
                target: target()?,
            }
        }
        36 => Command::Disembark {
            coordinates: source()?,
        },
        16 => {
            refresh_tech_cache(g, t, p)?;
            Command::Upgrade {
                kind: option()?,
                coordinates: source()?,
            }
        }
        30 => {
            let opponent = opponent()?;
            current_capital(g, opponent)?;
            embassy_cost(g, p)?;
            refresh_tech_cache(g, t, p)?;
            let o = player_index(g, opponent)?;
            relation_entry(g, o, p)?;
            Command::EstablishEmbassy { opponent }
        }
        2 => {
            let (origin, target) = (source()?, target()?);
            let node = g
                .tile(origin)
                .and_then(|x| x.unit)
                .ok_or("attack with no unit")?;
            let unit = g.units[node as usize].clone();
            crate::attack::admit_writes(g, t, &unit)?;
            Command::Attack {
                unit: unit.id,
                origin,
                target,
            }
        }
        7 => {
            let coordinates = source()?;
            let node = g
                .tile(coordinates)
                .and_then(|x| x.unit)
                .ok_or("capture with no unit")?;
            let unit = g.units[node as usize].id;
            crate::capture::admit_writes(g, p, coordinates)?;
            Command::Capture { unit, coordinates }
        }
        11 => Command::CityReward {
            reward: option()?,
            target: target()?,
        },
        14 => Command::ExamineRuins {
            coordinates: source()?,
        },
        4 => Command::HealOthers {
            coordinates: source()?,
        },
        13 => {
            let coordinates = source()?;
            let n = g
                .tile(coordinates)
                .and_then(|x| x.unit)
                .ok_or("promote with no unit")?;
            let owner = g.units[g.units[n as usize].passenger.unwrap_or(n) as usize].owner;
            refresh_tech_cache(g, t, owner)?;
            Command::Promote { coordinates }
        }
        // the menu's Resign is the mover's own; a recorded one names who resigned
        // (`opponent`) and, for a kick, who kicked (`option`) and that it was one (`accept`)
        35 => Command::Resign {
            resigned_player: action.opponent.unwrap_or(p),
            kicker_player: action.option.unwrap_or(0),
            was_kicked: action.accept.unwrap_or(false),
        },
        15 => Command::EndTurn,
        20 => Command::StartMatch,
        22 => Command::EndMatch,
        3 => Command::Recover {
            coordinates: source()?,
        },
        10 => Command::Disband {
            coordinates: source()?,
        },
        17 => {
            let coordinates = source()?;
            crate::polaris::admit_freeze_area(g, t, coordinates)?;
            Command::FreezeArea { coordinates }
        }
        18 => Command::BreakIce {
            coordinates: source()?,
        },
        38 => Command::Swarm {
            coordinates: source()?,
        },
        25 => Command::Boost {
            coordinates: source()?,
        },
        37 => Command::Flood {
            coordinates: source()?,
        },
        24 => Command::Explode {
            coordinates: source()?,
        },
        26 => Command::Decompose {
            coordinates: target()?,
        },
        39 => Command::ClearTileEffect {
            coordinates: source()?,
            effect: option()?,
        },
        k => return Err(format!("command kind {k} not implemented")),
    })
}

/// The command's own execution: its state writes and the actions it queues, in queue order.
/// `p` is the player who submitted it; `history` the number of commands so far, this one included.
fn execute_command(
    g: &mut Game,
    t: &Tables,
    p: i32,
    c: &Command,
    history: usize,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    match *c {
        Command::Research { technology } => {
            let tech = technology as usize;
            base(out, p, 8);
            if g.player(p).is_some() && t.techs.get(tech).is_some() {
                if t.techs.get(tech).unwrap().cost != 0 {
                    refresh_tech_cache(g, t, p)?;
                }
                let cost = tech_price(g, t, g.player(p).unwrap(), tech);
                out.push(Action::Research {
                    player: p,
                    technology,
                    cost,
                });
            }
        }
        Command::Build { kind, target } => {
            base(out, p, 1);
            out.push(Action::Economy(Economy::Build {
                player: p,
                kind,
                target,
                pay: true,
            }));
        }
        Command::Destroy { coordinates } => {
            base(out, p, 9);
            out.push(Action::MoveEffect(MoveEffect::DestroyImprovement {
                player: p,
                coordinates,
            }));
        }
        Command::BreakPeace { opponent } => {
            route_children(out, p, opponent);
            out.push(Action::Diplomacy(Diplomacy::BreakPeace {
                player: p,
                opponent,
            }));
        }
        Command::PeaceRequestResponse { opponent, accepted } => {
            let n = g
                .triggers
                .iter()
                .rposition(|x| x.player == p && x.kind == PEACE_REQUEST_TRIGGER)
                .ok_or("peace response without its trigger")?;
            g.triggers.remove(n);
            if accepted {
                route_children(out, p, opponent);
            }
            out.push(Action::Diplomacy(Diplomacy::PeaceResponse {
                player: p,
                opponent,
                accepted,
            }));
        }
        Command::Attack {
            unit,
            origin,
            target,
        } => crate::attack::attack_command(g, t, p, unit, origin, target, out)?,
        Command::HealOthers { coordinates } => {
            base(out, p, 4);
            if let Some(n) = g.tile(coordinates).ok_or("heal-others tile")?.unit {
                g.units[n as usize].moved = true;
                g.units[n as usize].attacked = true;
            }
            out.push(Action::TurnEffect(TurnEffect::HealOthers {
                player: p,
                coordinates,
            }));
        }
        Command::Promote { coordinates } => {
            // the catalog parser admits no promotesTo field, so promotion is always the Promote action
            base(out, p, 13);
            out.push(Action::Combat(Combat::Promote {
                player: p,
                coordinates,
            }));
        }
        Command::Resign {
            resigned_player,
            kicker_player,
            was_kicked,
        } => {
            base(out, p, 35);
            let Ok(i) = player_index(g, resigned_player) else {
                return Ok(());
            };
            let turn = g.current_turn;
            let resigned = &mut g.players[i];
            resigned.auto_play = true;
            resigned.resigned_at_command = history as i64 - 1;
            resigned.resigned_turn = i64::from(turn);
            if g.fixed.game_type == 1 || g.fixed.game_type == 4 {
                g.players[i].handicap = 1;
            }
            let pending_triggers = std::mem::take(&mut g.triggers);
            out.push(Action::TurnEffect(TurnEffect::Resign {
                player: p,
                resigned_player,
                kicker_player,
                was_kicked,
                pending_triggers,
            }));
            let (selected, completed) = crate::turns::winner(g)?;
            if completed {
                out.push(Action::GameOver {
                    player: p,
                    winner: selected,
                });
            }
        }
        Command::Capture { unit, coordinates } => {
            crate::capture::capture_command(g, p, unit, coordinates, out)?
        }
        Command::CityReward { reward, target } => {
            crate::reward::reward_command(g, p, reward, target, out)?
        }
        Command::ExamineRuins { coordinates } => {
            crate::ruins::examine_command(g, t, p, coordinates, out)?
        }
        Command::EndTurn => crate::turns::end_command(g, t, p, out)?,
        Command::EndMatch => crate::turns::end_match(g, p, out),
        Command::StartMatch => {
            base(out, p, 20);
            out.push(Action::StartMatch { player: p });
        }
        Command::Move { unit: id, from, to } => {
            base(out, p, 6);
            let Some(node) = find_unit(g, u64::from(id)) else {
                return Ok(());
            };
            let kind = g.units[node as usize].kind;
            if t.units.get(kind as usize).is_none() {
                return Ok(());
            }
            let occupied = g.tile(to).ok_or("move destination")?.unit.is_some();
            // the path first, before anything is written: a Move with none is refused as
            // the game refuses it before it runs (MovementError::NoPath), with the
            // game unchanged; the search does not read `moved`
            let path =
                if occupied {
                    None
                } else {
                    refresh_tech_cache(g, t, p)?;
                    let unit = g.units[node as usize].clone();
                    let path = crate::movement::unit_path(g, t, &unit, to)?;
                    Some(path.filter(|p| p.len() >= 2).ok_or(
                        "a Move with no path within the unit's movement: not a legal move",
                    )?)
                };
            g.units[node as usize].moved = true;
            out.push(Action::MoveEffect(MoveEffect::Reselect {
                player: p,
                unit: id,
            }));
            let Some(path) = path else {
                // a hidden enemy is there: the move becomes a Reveal
                g.units[node as usize].moved = false;
                let origin = g.units[node as usize].coordinates;
                out.push(Action::MoveEffect(MoveEffect::Reveal {
                    player: p,
                    coordinates: to,
                    origin,
                    show_popup: true,
                }));
                return Ok(());
            };
            let unit = g.units[node as usize].clone();
            out.push(Action::MoveEffect(MoveEffect::Move {
                player: p,
                unit: id,
                path,
                should_animate: true,
                reason: 0,
            }));
            let hide = t
                .units
                .get(kind as usize)
                .unwrap()
                .unit_abilities
                .has("hide");
            if hide && !crate::menu::unit_effect_list(g, unit.effects).contains(&3) {
                out.push(Action::MoveEffect(MoveEffect::Hide {
                    player: p,
                    coordinates: from,
                }));
            }
        }
        Command::Train { kind, target } => {
            base(out, p, 5);
            if let Some(d) = t.units.get(kind as usize) {
                out.push(Action::Economy(Economy::Train {
                    player: p,
                    kind,
                    target,
                    cost: d.cost,
                    direction_target: Xy { x: -1, y: -1 },
                    train_action_aborted: false,
                }));
            }
        }
        Command::EstablishEmbassy { opponent } => {
            out.push(Action::Diplomacy(Diplomacy::EstablishEmbassy {
                player: p,
                opponent,
            }));
            if let Some(other) = g.player(opponent) {
                out.push(Action::Meet {
                    player: opponent,
                    other: p,
                    coordinates: other.start_tile,
                    technology: 0,
                });
            }
        }
        Command::Upgrade { kind, coordinates } => {
            base(out, p, 16);
            if let Some(d) = t.units.get(kind as usize).filter(|_| kind != 0) {
                out.push(Action::TurnEffect(TurnEffect::Upgrade {
                    player: p,
                    coordinates,
                    unit: kind,
                    cost: d.cost,
                }));
            }
        }
        Command::Disembark { coordinates } => out.push(Action::MoveEffect(MoveEffect::Disembark {
            player: p,
            coordinates,
        })),
        Command::PeaceTreaty { opponent } => out.push(Action::Diplomacy(Diplomacy::PeaceTreaty {
            player: p,
            opponent,
        })),
        Command::Recover { coordinates } => {
            base(out, p, 3);
            out.push(Action::TurnEffect(TurnEffect::Recover {
                player: p,
                coordinates,
            }));
            out.push(Action::TurnEffect(TurnEffect::Heal {
                player: p,
                coordinates,
                fixed_heal: 0,
            }));
        }
        Command::FreezeArea { coordinates } => {
            crate::polaris::freeze_area_command(g, p, coordinates, out)?
        }
        Command::ClearTileEffect {
            coordinates,
            effect,
        } => crate::aquarion::clear_command(p, coordinates, effect, out),
        Command::Swarm { coordinates } | Command::Boost { coordinates } => {
            crate::cymanti::swarm_command(g, p, coordinates, out)
        }
        // rules124 runs the command base first; rules123 queues the explosion alone
        Command::Explode { coordinates } => {
            if g.fixed.version >= 124 {
                base(out, p, 24);
            }
            out.push(Action::Combat(Combat::Explode {
                player: p,
                coordinates,
                home_coordinates: Xy { x: 0, y: 0 },
            }));
        }
        // never offered by the menu, but admitted: the command base, the unit spent, the flood
        Command::Flood { coordinates } => {
            base(out, p, 37);
            if let Some(n) = g.tile(coordinates).and_then(|x| x.unit) {
                g.units[n as usize].moved = true;
                g.units[n as usize].attacked = true;
            }
            out.push(Action::Combat(Combat::Flood {
                player: p,
                coordinates,
            }));
        }
        Command::Decompose { coordinates } => {
            crate::cymanti::decompose_command(g, t, p, coordinates, out)?
        }
        Command::BreakIce { coordinates } => {
            crate::polaris::break_ice_command(g, p, coordinates, out)?
        }
        Command::Disband { coordinates } => {
            base(out, p, 10);
            out.push(Action::DisbandUnit {
                player: p,
                coordinates,
                home_coordinates: Xy { x: 0, y: 0 },
                get_paid: true,
            });
        }
    }
    Ok(())
}

/// Reorder the children pushed since `start`: those pushed before `mid` go behind the later
/// ones, back to front (the game buffers them and appends the buffer reversed).
pub(crate) fn buffered_last(out: &mut [Action], start: usize, mid: usize) {
    out[start..mid].reverse();
    out[start..].rotate_left(mid - start);
}

fn pending_trigger(g: &Game) -> bool {
    let p = mover(g);
    g.triggers.iter().any(|x| x.player == p)
}

pub(crate) const TASKS: [&str; 10] = [
    "None",
    "Pacifist",
    "Genius",
    "Network",
    "Wealth",
    "Killer",
    "Metropolis",
    "Explorer",
    "ExploreLighthouses",
    "Convert",
];

/// Whether the task is completed, for the predicates handled so far; also writes the completed flag
fn task_completed(g: &mut Game, t: &Tables, i: usize, n: usize) -> Result<bool, String> {
    let id = g.players[i].action.id;
    let task = g.players[i].action.tasks[n];
    if task.flags.completed {
        return Ok(false);
    }
    let done = match task.kind.name() {
        "Wealth" => g.players[i].action.currency >= 100,
        "Genius" => tribe_tech(t, g.players[i].tribe)?
            .iter()
            .all(|&k| g.players[i].available_tech.contains(&(k as i32))),
        "Pacifist" => task.peaceful_turns.ok_or("peaceful turns")? >= 5,
        "Killer" => task.kills.ok_or("kills")? >= 10,
        "Convert" => task.conversions.ok_or("conversions")? >= 3,
        "Metropolis" => g.tiles.iter().any(|x| {
            x.owner == id
                && x.improvement
                    .as_ref()
                    .is_some_and(|m| m.kind == CITY && m.level > 4)
        }),
        "Network" => {
            let start = g.players[i].start_tile;
            g.tiles
                .iter()
                .filter(|x| {
                    x.owner == id
                        && x.coordinates != start
                        && x.improvement.as_ref().is_some_and(|m| {
                            m.kind == CITY && m.connected_to_capital_of_player == id
                        })
                })
                .count()
                >= 5
        }
        "Explorer" => {
            let explored = g
                .tiles
                .iter()
                .filter(|x| id == NATURE || x.explorers.contains(&id))
                .count();
            g.players[i].action.tasks[n].explored_tiles = Some(explored as i64);
            explored >= g.tiles.len()
        }
        "ExploreLighthouses" => {
            let (w, h) = (g.width - 1, g.tiles.len() as i32 / g.width - 1);
            [(w, h), (0, 0), (0, h), (w, 0)]
                .iter()
                .filter(|&&(x, y)| {
                    g.tile(Xy { x, y })
                        .is_some_and(|c| id == NATURE || c.explorers.contains(&id))
                })
                .count()
                == 4
        }
        _ => return Err("task type None".into()),
    };
    g.players[i].action.tasks[n].flags.completed = done;
    Ok(done)
}

/// Task check and notification: evaluate the player's task of this kind (if any), returning
/// TaskCompleted when it completes now
pub(crate) fn task_notification(
    g: &mut Game,
    t: &Tables,
    i: usize,
    kind: usize,
) -> Result<Option<Action>, String> {
    let tasks = &g.players[i].action.tasks;
    let Some(n) = tasks.iter().position(|x| x.kind.0 as usize == kind) else {
        return Ok(None);
    };
    Ok(task_completed(g, t, i, n)?.then(|| Action::TaskCompleted {
        player: g.players[i].action.id,
        task: kind as u32,
    }))
}

fn check_task(
    g: &mut Game,
    t: &Tables,
    i: usize,
    kind: usize,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    out.extend(task_notification(g, t, i, kind)?);
    Ok(())
}

fn new_task(kind: usize) -> Result<crate::state::Task, String> {
    use crate::state::{Task, TaskFlags, TaskKind};
    let name = TASKS.get(kind).copied().filter(|name| *name != "None");
    let name = name.ok_or_else(|| format!("task type {kind}"))?;
    let counter = |of: &str, start: i64| (name == of).then_some(start);
    Ok(Task {
        kind: TaskKind(kind as u8),
        flags: TaskFlags {
            started: true,
            completed: false,
        },
        peaceful_turns: counter("Pacifist", 0),
        kills: counter("Killer", 1),
        conversions: counter("Convert", 0),
        explored_tiles: counter("Explorer", 0),
    })
}

/// Add to the score: the score, and at the score limit, if the match is decided,
/// GameOver for the scorer after it
pub(crate) fn add_score(
    g: &mut Game,
    id: i32,
    amount: i64,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let Ok(i) = player_index(g, id) else {
        return Ok(());
    };
    let score = (g.players[i].score as u32).wrapping_add(amount as u32);
    g.players[i].score = i64::from(score);
    let limit = g.fixed.score_limit;
    if limit > 0 && i64::from(score) >= limit && g.current_state == STARTED {
        let (winner, completed) = crate::turns::winner(g)?;
        if completed {
            out.push(Action::GameOver { player: id, winner });
        }
    }
    Ok(())
}

fn heal(g: &mut Game, t: &Tables, c: Xy, fixed: i64) -> Result<(), String> {
    let Some(tile) = g.tile(c) else { return Ok(()) };
    let Some(node) = tile.unit else { return Ok(()) };
    let owner = tile.owner;
    let unit = g
        .units
        .get(node as usize)
        .ok_or("unit node out of range")?
        .clone();
    if crate::menu::unit_effect_list(g, unit.effects).contains(&POISONED) {
        return crate::cymanti::cure(g, node);
    }
    let max = crate::combat::max_health(g, t, &unit)? as u16;
    let health = unit.health as u16;
    let amount = if fixed != 0 {
        fixed as u16
    } else {
        let mut amount = 20u16;
        if let Ok(i) = player_index(g, unit.owner) {
            let peace = owner != unit.owner && owner != 0 && {
                relation_entry(g, i, owner)?.state == PEACE
            };
            if owner == unit.owner || peace {
                amount = 40;
            }
        }
        amount.min(max.wrapping_sub(health))
    };
    g.units[node as usize].health = i32::from(health.wrapping_add(amount).min(max));
    Ok(())
}

/// Disband: removes the tile's unit reference (the node stays), refunds half the cost
/// one star at a time when paid, and takes back the unit's score
fn disband(
    g: &mut Game,
    t: &Tables,
    player: i32,
    c: Xy,
    paid: bool,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let i = g
        .tiles
        .iter()
        .position(|x| x.coordinates == c)
        .ok_or("disband tile")?;
    let Some(node) = g.tiles[i].unit else {
        return Ok(());
    };
    let unit = g
        .units
        .get(node as usize)
        .ok_or("unit node out of range")?
        .clone();
    // the follower goes too (paid), and the leader lets go
    if unit.follower != 0 {
        if let Some(f) = crate::linked::find(g, unit.follower) {
            out.push(Action::DisbandUnit {
                player,
                coordinates: g.units[f as usize].coordinates,
                home_coordinates: Xy { x: 0, y: 0 },
                get_paid: true,
            });
        }
    }
    if unit.leader != 0 {
        if let Some(l) = crate::linked::find(g, unit.leader) {
            g.units[l as usize].follower = 0;
        }
    }
    if let Some(data) = t.units.get(unit.kind as usize) {
        let passenger_cost = unit
            .passenger
            .and_then(|n| g.units.get(n as usize))
            .and_then(|p| t.units.get(p.kind as usize))
            .map_or(0, |d| d.cost);
        let cost = data.cost.wrapping_add(passenger_cost);
        if paid && cost > 1 {
            for _ in 0..cost / 2 {
                out.push(Action::Payload(Payload::IncreaseCurrency {
                    player: player,
                    source: c,
                    amount: 1,
                    delay: 40,
                }));
            }
        }
        out.push(Action::Combat(Combat::DecreaseScore {
            player: unit.owner,
            amount: cost.wrapping_mul(5),
        }));
    }
    g.tiles[i].unit = None;
    if t.units
        .get(unit.kind as usize)
        .is_some_and(|d| d.unit_abilities.has("tentacles"))
    {
        crate::aquarion::update_tentacles(g, t)?;
    }
    Ok(())
}

/// Run one queued action, pushing its children onto `out` (none when the game removes
/// it as invalid).
fn execute_action(
    g: &mut Game,
    t: &Tables,
    a: &Action,
    history: usize,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    match *a {
        Action::Payload(Payload::EndCommand { .. }) => {
            for p in &mut g.players {
                p.action.opinions_dirty = true;
            }
        }
        Action::Payload(Payload::RunDeferred { player }) => {
            if !g.needs_routes {
                return Ok(());
            }
            out.push(Action::UpdateCityConnections {
                actor: player,
                players: Seats::from_iter([player]),
            });
            out.push(Action::UpdateRoutes { player });
            g.needs_routes = false;
        }
        Action::Payload(Payload::IncreaseScore { player, amount, .. }) => {
            add_score(g, player, i64::from(amount), out)?
        }
        Action::Payload(Payload::IncreaseCurrency { player, amount, .. }) => {
            if let Ok(i) = player_index(g, player) {
                let c = &mut g.players[i].action.currency;
                *c = c.wrapping_add(amount).max(0);
                check_task(g, t, i, 4, out)?;
            }
        }
        Action::Combat(Combat::FreezeArea {
            player,
            coordinates,
            radius,
            freeze_units,
            only_owned,
        })
        | Action::Economy(Economy::FreezeArea {
            player,
            target: coordinates,
            radius,
            freeze_units,
            only_owned_tiles: only_owned,
        }) => crate::polaris::freeze_area(
            g,
            player,
            coordinates,
            radius,
            freeze_units,
            only_owned,
            out,
        )?,
        Action::Combat(Combat::Flood {
            player,
            coordinates,
        }) => crate::aquarion::flood(g, t, player, coordinates, out)?,
        Action::Combat(Combat::Bubble { unit, add, .. }) => crate::aquarion::bubble(g, unit, add)?,
        Action::Combat(Combat::Eat {
            player,
            unit,
            coordinates,
        }) => crate::linked::eat(g, t, player, unit, coordinates, out)?,
        Action::Combat(Combat::Upgrade {
            player,
            kind,
            coordinates,
            cost,
        }) => crate::units::upgrade(g, t, player, coordinates, kind, cost, out)?,
        Action::Combat(Combat::Poison { player, target, .. }) => {
            crate::cymanti::poison(g, player, target)?
        }
        Action::Combat(Combat::Explode {
            player,
            coordinates,
            ..
        }) => crate::cymanti::explode(g, t, player, coordinates, out)?,
        Action::TurnEffect(TurnEffect::BoostOthers {
            player,
            coordinates,
        }) => crate::cymanti::boost_others(g, player, coordinates, out),
        Action::TurnEffect(TurnEffect::Boost { coordinates, .. }) => {
            crate::cymanti::boost(g, coordinates)?
        }
        Action::TurnEffect(TurnEffect::Decompose { coordinates, .. }) => {
            crate::cymanti::decompose(g, coordinates)?
        }
        Action::MoveEffect(MoveEffect::HealOthers {
            player,
            coordinates,
        }) => {
            for c in crate::cymanti::heal_options(g, t, coordinates, player, false)? {
                out.push(Action::TurnEffect(TurnEffect::Heal {
                    player,
                    coordinates: c,
                    fixed_heal: 40,
                }));
            }
        }
        Action::TurnEffect(TurnEffect::ClearTileEffect {
            player,
            coordinates,
            effect,
            triggered_by_unit,
        }) => crate::aquarion::clear_tile_effect(
            g,
            player,
            coordinates,
            effect,
            triggered_by_unit,
            out,
        )?,
        Action::Combat(Combat::FreezeTile {
            player,
            coordinates,
        }) => crate::polaris::freeze_tile(g, t, player, coordinates, out)?,
        Action::Combat(Combat::FreezeUnit {
            player,
            origin,
            target,
            damage,
        }) => crate::polaris::freeze_unit(g, t, player, origin, target, damage, out)?,
        Action::TurnEffect(TurnEffect::BreakIceArea {
            player,
            coordinates,
            radius,
        }) => crate::polaris::break_ice_area(g, t, player, coordinates, radius, out)?,
        Action::TurnEffect(TurnEffect::BreakIce {
            player,
            coordinates,
        }) => crate::polaris::break_ice(g, t, player, coordinates, out)?,
        Action::TurnEffect(TurnEffect::ChangeClimate {
            player,
            coordinates,
            climate,
        }) => crate::polaris::change_climate(g, player, coordinates, climate)?,
        Action::Combat(Combat::DecreaseScore { player, amount }) => {
            if let Ok(i) = player_index(g, player) {
                g.players[i].score =
                    i64::from((g.players[i].score as u32).wrapping_sub(amount as u32));
            }
        }
        Action::EnableTask { player, task } => {
            let kind = task as usize;
            if let Ok(i) = player_index(g, player) {
                if g.players[i].task(kind).is_none() {
                    g.players[i].action.tasks.push(new_task(kind)?);
                    check_task(g, t, i, kind, out)?;
                }
            }
        }
        Action::TaskCompleted { .. } => {}
        Action::TurnEffect(TurnEffect::Upgrade {
            player,
            coordinates,
            unit,
            cost,
        }) => crate::units::upgrade(g, t, player, coordinates, unit, cost, out)?,
        Action::DestroyEmbassy {
            player: p,
            opponent: q,
        } => {
            // the opponent's relation toward the player, inserted if missing
            let Ok(o) = player_index(g, q) else {
                return Ok(());
            };
            if p == 0 {
                return Ok(());
            }
            let r = relation_entry(g, o, p)?;
            if r.embassy_level <= 0 {
                return Ok(());
            }
            r.embassy_level = 0;
        }
        Action::Combat(Combat::Attack {
            player,
            origin,
            target,
            damage,
            ..
        }) => crate::attack::perform_attack(g, t, player, origin, target, damage, out)?,
        Action::Combat(Combat::Kill { coordinates, .. }) => {
            crate::attack::kill(g, t, coordinates, out)?
        }
        Action::Combat(Combat::CaptureCity {
            player,
            coordinates,
            ..
        }) => crate::capture::capture_city(g, t, player, coordinates, out)?,
        Action::Combat(Combat::Infiltrate {
            player,
            origin,
            target,
        }) => crate::infiltrate::infiltrate(g, t, player, origin, target, out)?,
        Action::Combat(Combat::InfiltrationReward {
            player,
            reward,
            coordinates,
            kind,
        }) => crate::infiltrate::reward(g, t, player, reward, coordinates, kind, out)?,
        Action::Combat(Combat::Convert {
            player,
            origin,
            target,
        }) => crate::attack::convert(g, t, player, origin, target, out)?,
        Action::TurnEffect(TurnEffect::EndTurn { player }) => {
            crate::turns::end_turn(g, player, out)?
        }
        Action::TurnEffect(TurnEffect::StartTurn { player }) => {
            crate::turns::start_turn(g, t, player, out)?
        }
        Action::TurnEffect(TurnEffect::PassPlayer { .. })
        | Action::TurnEffect(TurnEffect::EndMatch { .. }) => {}
        Action::TurnEffect(TurnEffect::ReceiveDiplomacyMessage {
            player,
            sender,
            kind,
        }) => crate::turns::receive_message(g, player, sender, kind)?,
        Action::GameOver { .. } => crate::turns::game_over(g),
        Action::Economy(Economy::CityReward {
            player,
            reward,
            target,
        }) => crate::reward::city_reward(g, t, player, i64::from(reward), target, out)?,
        Action::Economy(ref scout @ Economy::Scout { .. }) => {
            crate::reward::scout(g, t, scout, out)?
        }
        Action::Economy(Economy::ExpandCity { player, target }) => {
            crate::reward::expand_city(g, t, player, target, out)?
        }
        Action::Economy(Economy::ModifyProduction { amount, target, .. }) => {
            crate::reward::modify(g, "ModifyProduction", target, amount)?
        }
        Action::Economy(Economy::ModifyScore { amount, target, .. }) => {
            crate::reward::modify(g, "ModifyScore", target, amount)?
        }
        Action::Ruins {
            player,
            reward,
            coordinates,
        } => crate::ruins::ruins(g, t, player, reward, coordinates, out)?,
        Action::Combat(Combat::Promote { coordinates, .. }) => {
            crate::units::promote(g, t, coordinates)?
        }
        Action::Economy(Economy::Houdini { .. }) => {}
        Action::WipePlayer { player: p, target } => {
            // wipe a player: record the elimination, disband the victim's units, end every embassy
            let k = player_index(g, p)?;
            g.players[k].wipe_outs += 1;
            let v = player_index(g, target)?;
            let turn = g.current_turn;
            let victim = &mut g.players[v];
            victim.killed_turn = i64::from(turn);
            victim.killer_id = i64::from(p);
            victim.wiped_at_command = history as i64 - 1;
            out.push(Action::WipePlayerEnd { player: p, target });
            for x in &g.tiles {
                if x.unit.is_some_and(|n| g.units[n as usize].owner == target) {
                    out.push(Action::DisbandUnit {
                        player: target,
                        coordinates: x.coordinates,
                        home_coordinates: Xy { x: 0, y: 0 },
                        get_paid: true,
                    });
                }
            }
            for q in g.players.iter().map(|q| q.action.id) {
                out.push(Action::DestroyEmbassy {
                    player: target,
                    opponent: q,
                });
                out.push(Action::DestroyEmbassy {
                    player: q,
                    opponent: target,
                });
            }
        }
        Action::WipePlayerEnd { .. } => {}
        Action::TurnEffect(TurnEffect::HealOthers {
            player: p,
            coordinates: c,
        }) => {
            // heal options: every own wounded or poisoned neighbour (not the healer's tile)
            g.tile(c).ok_or("heal-others tile")?;
            for xx in c.x - 1..=c.x + 1 {
                for yy in c.y - 1..=c.y + 1 {
                    let target = Xy { x: xx, y: yy };
                    if target == c {
                        continue;
                    }
                    let Some(n) = g.tile(target).and_then(|x| x.unit) else {
                        continue;
                    };
                    let u = g.units[n as usize].clone();
                    if u.owner != p {
                        continue;
                    }
                    let max = crate::combat::max_health(g, t, &u)?;
                    if max > u.health
                        || crate::menu::unit_effect_list(g, u.effects).contains(&POISONED)
                    {
                        out.push(Action::TurnEffect(TurnEffect::Heal {
                            player: p,
                            coordinates: target,
                            fixed_heal: 40,
                        }));
                    }
                }
            }
        }
        Action::TurnEffect(TurnEffect::Resign {
            ref pending_triggers,
            ..
        }) => g.triggers.extend(pending_triggers.iter().copied()),
        Action::Economy(Economy::Train {
            player,
            kind,
            target,
            cost,
            direction_target,
            ..
        }) => crate::units::train(g, t, player, kind, target, cost, direction_target, out)?,
        Action::MoveEffect(MoveEffect::Move {
            player,
            unit,
            ref path,
            reason,
            ..
        }) => move_action(g, t, player, unit, path, reason, out)?,
        Action::MoveEffect(MoveEffect::Reselect { .. }) => {}
        Action::MoveEffect(MoveEffect::Reveal {
            player: p,
            coordinates: c,
            origin,
            ..
        }) => {
            if let Some(n) = g.tile(c).and_then(|t| t.unit) {
                let list = g.units[n as usize].effects as usize;
                let effects = g.effect_lists.get_mut(list).ok_or("effect list")?;
                if let Some(k) = effects.iter().position(|e| *e == 3) {
                    effects.remove(k);
                }
                if let Some(from) = g.tile(origin).and_then(|t| t.unit) {
                    let d = facing_direction(origin, c);
                    g.units[from as usize].direction = d;
                    g.units[from as usize].flipped = d < 4;
                    let target_owner = g.units[n as usize].owner;
                    let other = if target_owner == p {
                        g.units[from as usize].owner
                    } else {
                        target_owner
                    };
                    out.push(Action::Meet {
                        player: p,
                        other,
                        coordinates: c,
                        technology: 0,
                    });
                }
            }
        }
        Action::MoveEffect(MoveEffect::Hide { coordinates, .. }) => {
            if let Some(n) = g.tile(coordinates).and_then(|c| c.unit) {
                g.units[n as usize].attacked = true;
                let list = g.units[n as usize].effects as usize;
                let effects = g.effect_lists.get_mut(list).ok_or("effect list")?;
                if !effects.contains(&3) {
                    effects.push(3);
                }
            }
        }
        Action::MoveEffect(MoveEffect::Embark {
            player,
            coordinates,
        }) => crate::units::embark(g, t, player, coordinates, out)?,
        Action::MoveEffect(MoveEffect::Disembark {
            player,
            coordinates,
        }) => crate::units::disembark(g, t, player, coordinates, out)?,
        Action::Explore {
            player,
            coordinates,
        } => crate::explore::explore(g, t, player, coordinates, out)?,
        Action::Meet {
            player,
            other,
            coordinates,
            ..
        } => crate::explore::meet_action(g, t, player, other, coordinates, out)?,
        Action::Economy(ref e) => crate::economy::economy_action(g, t, e, out)?,
        Action::City(City::IncreasePopulation { player, target, .. }) => {
            crate::economy::city_action(g, t, "IncreasePopulation", player, target, out)?
        }
        Action::City(City::DecreasePopulation { player, target, .. }) => {
            crate::economy::city_action(g, t, "DecreasePopulation", player, target, out)?
        }
        Action::City(City::LevelUp { player, target }) => {
            crate::economy::city_action(g, t, "LevelUp", player, target, out)?
        }
        Action::MoveEffect(MoveEffect::DestroyImprovement {
            player,
            coordinates,
        }) => crate::economy::destroy(g, t, player, coordinates, out)?,
        Action::UpdateRoutes { .. } => crate::routes::update_routes(g, t)?,
        Action::UpdateCityConnections { ref players, .. } => {
            crate::routes::update_city_connections(g, t, players, out)?;
        }
        Action::CityConnection(ref c) => crate::routes::city_connection(g, t, c, out)?,
        Action::Diplomacy(Diplomacy::BreakPeace {
            player: p,
            opponent: q,
        }) => {
            let (other, actor) = (player_index(g, q)?, player_index(g, p)?);
            let turn = g.current_turn;
            for (i, o) in [(actor, q), (other, p)] {
                let r = relation_entry(g, i, o)?;
                r.state = COOL_DOWN;
                r.last_peace_broken_turn = turn;
            }
            // the loop writes only unit flags, so the tiles can be read as it goes
            for i in 0..g.tiles.len() {
                let Some(n) = g.tiles[i].unit else { continue };
                if g.units.get(n as usize).is_none_or(|u| u.owner != p) {
                    continue;
                }
                let c = g.units[n as usize].coordinates;
                if g.tile(c).ok_or("unit off the board")?.owner == q {
                    out.push(Action::DisbandUnit {
                        player: p,
                        coordinates: c,
                        home_coordinates: Xy { x: 0, y: 0 },
                        get_paid: false,
                    });
                } else {
                    g.units[n as usize].moved = true;
                    g.units[n as usize].attacked = true;
                }
            }
        }
        Action::Diplomacy(Diplomacy::EstablishEmbassy {
            player: p,
            opponent: q,
        }) => {
            let (other, actor) = (player_index(g, q)?, player_index(g, p)?);
            let cost = embassy_cost(g, p)?;
            let c = &mut g.players[actor].action.currency;
            *c = c.wrapping_sub(cost).max(0);
            let turn = g.current_turn;
            let r = relation_entry(g, other, p)?;
            r.embassy_level = 1;
            r.embassy_build_turn = turn;
            let center = g.players[other].start_tile;
            for x in center.x - 1..=center.x + 1 {
                for y in center.y - 1..=center.y + 1 {
                    let c = Xy { x, y };
                    if c != center && g.tile(c).is_some_and(|t| !t.explorers.contains(&p)) {
                        out.push(Action::Explore {
                            player: p,
                            coordinates: c,
                        });
                    }
                }
            }
        }
        Action::Diplomacy(Diplomacy::PeaceResponse {
            player: p,
            opponent: q,
            accepted,
        }) => {
            if accepted {
                let (actor, other) = (player_index(g, p)?, player_index(g, q)?);
                relation_entry(g, actor, q)?.state = PEACE;
                relation_entry(g, other, p)?.state = PEACE;
            }
        }
        Action::StartMatch { .. } => crate::reset::start_match(g, t, out)?,
        Action::Research {
            player: p,
            technology,
            cost,
        } => {
            let tech = technology as i32;
            let Ok(i) = player_index(g, p) else {
                return Ok(());
            };
            if g.players[i].available_tech.contains(&tech) {
                return Ok(());
            }
            let data = t.techs.get(tech as usize).ok_or("unknown tech")?;
            let vision = data.ability_unlocks.has("capitalvision");
            let start = g.players[i].start_tile;
            g.players[i].available_tech.push(tech);
            let c = &mut g.players[i].action.currency;
            *c = c.wrapping_sub(cost).max(0);
            out.push(Action::Payload(Payload::IncreaseScore {
                player: p,
                amount: data.cost * 100,
                delay: 0,
                source: start,
            }));
            for name in &data.task_unlocks {
                let kind = t.tasks.id(name)?;
                if g.players[i].task(kind).is_none() {
                    out.push(Action::EnableTask {
                        player: p,
                        task: kind as u32,
                    });
                }
            }
            if vision {
                // reveal all capitals: Explore each known player's unexplored capital
                refresh_tech_cache(g, t, p)?;
                if crate::menu::player_ability(g, t, p, "capitalvision") {
                    for o in 0..g.players.len() {
                        let other = g.players[o].action.id;
                        let known = other == 255
                            || other == p
                            || g.players[i].known_players.contains(&other);
                        if other == 255 || !known {
                            continue;
                        }
                        let start = g.players[o].start_tile;
                        if !g
                            .tile(start)
                            .ok_or("capital off the board")?
                            .explorers
                            .contains(&p)
                        {
                            out.push(Action::Explore {
                                player: p,
                                coordinates: start,
                            });
                        }
                    }
                }
            }
            check_task(g, t, i, 2, out)?;
        }
        Action::Diplomacy(Diplomacy::PeaceTreaty { player, opponent }) => {
            let i = player_index(g, opponent)?;
            g.players[i].messages.try_push(crate::state::Message {
                kind: PEACE_REQUEST_MESSAGE,
                sender: player,
            })?;
        }
        Action::TurnEffect(TurnEffect::Recover { coordinates, .. }) => {
            if let Some(tile) = g.tile(coordinates) {
                let node = tile.unit.ok_or("Recover with no unit")?;
                let u = g
                    .units
                    .get_mut(node as usize)
                    .ok_or("unit node out of range")?;
                u.moved = true;
                u.attacked = true;
            }
        }
        Action::TurnEffect(TurnEffect::Heal {
            coordinates,
            fixed_heal,
            ..
        }) => heal(g, t, coordinates, i64::from(fixed_heal))?,
        Action::DisbandUnit {
            player,
            coordinates,
            get_paid,
            ..
        } => disband(g, t, player, coordinates, get_paid, out)?,
    }
    Ok(())
}

thread_local! {
    /// The action stack of the command being run (`submit`), kept for its storage.
    static STACK: std::cell::Cell<Vec<Action>> = const { std::cell::Cell::new(Vec::new()) };
}

/// Count the command `player` submits and run the command manager until it is idle or a
/// pending choice stops it. No log is kept: the rules read only how many commands there
/// have been.
pub(crate) fn submit(
    g: &mut Game,
    t: &Tables,
    player: i32,
    command: Command,
) -> Result<(), String> {
    submit_seen(g, t, player, command, |_| {})
}

/// `submit`, showing `seen` every queued action as it is taken off the stack to run.
fn submit_seen(
    g: &mut Game,
    t: &Tables,
    player: i32,
    command: Command,
    mut seen: impl FnMut(&Action),
) -> Result<(), String> {
    g.commands += 1;
    let history = g.commands as usize;
    // The stack runs outside the game, so the executors can borrow the game, and in a list
    // kept per thread: the game's own stack is empty between decisions unless a pending
    // choice interrupted a command, so a game (or a fresh copy of one) need not grow it.
    let mut actions = STACK.take();
    actions.append(&mut g.actions);
    let mut command = Some(command);
    loop {
        if !actions.is_empty() && !pending_trigger(g) {
            // the newest action runs; its children go on top in its place
            let a = actions.pop().expect("checked not empty");
            seen(&a);
            execute_action(g, t, &a, history, &mut actions)?;
            continue;
        }
        if g.current_command >= g.commands {
            break;
        }
        // between decisions every counted command has run, so the one left is this one
        let c = command
            .take()
            .ok_or("the state has commands that never ran")?;
        g.current_command += 1;
        execute_command(g, t, player, &c, history, &mut actions)?;
    }
    g.actions.append(&mut actions);
    STACK.set(actions);
    Ok(())
}

/// One recorded command as the game's command manager takes it, without what the game
/// environment adds around a decision (`advance`): Nature's turn is not played for it and
/// the decision count does not move. For replays, whose command lists hold every player's
/// commands. `seen` is shown each queued action the command runs, for coverage counts.
pub fn command(
    g: &mut Game,
    t: &Tables,
    action: &engine_fast_frame::Action,
    seen: impl FnMut(&Action),
) -> Result<(), String> {
    let player = mover(g);
    let command = admit(g, t, action)?;
    submit_seen(g, t, player, command, seen)
}

/// One decision as the game environment makes it: `advance`, then the next viewer's
/// memory for input preparation (`viewer::refresh`).
pub fn step(g: &mut Game, t: &Tables, action: &engine_fast_frame::Action) -> Result<u8, String> {
    let elapsed = advance(g, t, action)?;
    let _t = engine_fast_frame::timers::scope("step.viewer memory");
    // a new frame refreshes the next viewer's memory, unless the game just ended (resignation or End)
    if action.kind != 35 && g.current_state != 4 {
        crate::viewer::refresh(g)?;
    }
    Ok(elapsed)
}

/// The engine's own step: admit and run `action` (the chosen candidate's action), then
/// Nature's continuation. Returns the game's elapsed-turns flag: 1 if the command itself
/// changed the current player or turn. On Err the state is unspecified.
pub fn advance(g: &mut Game, t: &Tables, action: &engine_fast_frame::Action) -> Result<u8, String> {
    let mut _t = engine_fast_frame::timers::scope("step.admit");
    let player = mover(g);
    let command = admit(g, t, action)?;
    let before = (mover(g), g.current_turn);
    _t = engine_fast_frame::timers::scope("step.command");
    submit(g, t, player, command)?;
    _t = engine_fast_frame::timers::scope("step.nature turn");
    let elapsed = u8::from((mover(g), g.current_turn) != before);
    // Nature's turn ends at once (EndMatch in the final turn), at most 4 times
    for _ in 0..4 {
        let state = g.current_state;
        if state == 4 || mover(g) != NATURE {
            break;
        }
        let command = match state {
            STARTED => Command::EndTurn,
            3 => Command::EndMatch,
            _ => return Err("unexpected Nature phase".into()),
        };
        submit(g, t, NATURE, command)?;
    }
    if mover(g) == NATURE && g.current_state != 4 {
        return Err("Nature continuation did not settle".into());
    }
    g.decisions += 1;
    Ok(elapsed)
}
