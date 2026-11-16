//! Improvements and city growth: the Build and DestroyImprovement bodies, improvement levels
//! and the surrounding-area check, resources and roads, and the City actions (population and level-up
//! with its trigger).

use crate::action::{Action, City, Combat, Economy, MoveEffect, Payload};
use crate::inline::Seats;
use crate::state::{Game, Xy};
use crate::step::{player_index, task_notification};
use crate::tables::{ImprovementData, Tables};
use serde_json::Value;

const CITY: i32 = 1;
const ROAD: i32 = 3;
const NULL: Xy = Xy { x: -1, y: -1 };
const METROPOLIS_TASK: usize = 6;
const CITY_LEVEL_UP: i64 = 1;

fn data(t: &Tables, kind: i32) -> Option<&ImprovementData> {
    (kind != 0)
        .then(|| t.improvements.get(kind as usize))
        .flatten()
}

fn has(d: &ImprovementData, ability: &str) -> bool {
    d.improvement_abilities.has(ability)
}

fn field(v: &Value, k: &str) -> i32 {
    v[k].as_i64().unwrap_or(0) as i32
}

/// The score of the improvement on tile `i`
pub(crate) fn improvement_score(t: &Tables, g: &Game, i: usize) -> i32 {
    let x = &g.tiles[i];
    let Some(m) = &x.improvement else { return 0 };
    if x.owner == 0 || x.owner == 255 {
        return 0;
    }
    let Some(d) = t.improvements.get(m.kind as usize) else {
        return 0;
    };
    let mut score = m.base_score;
    if m.kind == CITY {
        score = score
            .wrapping_add((m.level + 1).wrapping_mul(50))
            .wrapping_add(m.xp.wrapping_mul(5));
        if m.xp < 0 {
            score = score.wrapping_add(m.xp.wrapping_mul(100)).max(0);
        }
    }
    if has(d, "patina") {
        let per: i32 = d.growth_rewards.iter().map(|r| field(r, "score")).sum();
        score = score.wrapping_add(per.wrapping_mul(m.level));
    }
    score
}

fn adjacency_bonus(g: &Game, t: &Tables, i: usize, d: &ImprovementData) -> Result<i32, String> {
    let (unique, per_tile) = (has(d, "picky"), has(d, "simple"));
    let c = g.tiles[i].coordinates;
    let owner = g.tiles[i].owner;
    let (mut seen, mut total) = (Vec::new(), 0i32);
    for x in c.x - 1..=c.x + 1 {
        for y in c.y - 1..=c.y + 1 {
            if (x, y) == (c.x, c.y) {
                continue;
            }
            let Some(o) = g.index(Xy { x, y }).map(|j| &g.tiles[j]) else {
                continue;
            };
            if o.owner != owner {
                continue;
            }
            for e in &d.adjacency_improvements {
                let mut add = |id: i32, level: i32| {
                    if unique {
                        if !seen.contains(&id) {
                            seen.push(id);
                            total += 1;
                        }
                    } else {
                        total += if per_tile { 1 } else { level.max(1) };
                    }
                };
                if let Some(n) = e["improvement"].as_str() {
                    let id = t.improvements.id(n)? as i32;
                    if let Some(m) = &o.improvement {
                        if m.kind != 0 && m.kind == id {
                            add(id, m.level);
                        }
                    }
                }
                if let Some(n) = e["resources"].as_str() {
                    let id = t.resources.id(n)? as i32;
                    if o.improvement.is_none() && o.resource.is_some_and(|r| r != 0 && r == id) {
                        add(id, 1);
                    }
                }
            }
        }
    }
    Ok(total)
}

/// The improvement level calculation (>=104)
fn level_for(g: &Game, t: &Tables, i: usize) -> Result<i32, String> {
    let x = &g.tiles[i];
    let Some(m) = &x.improvement else {
        return Ok(1);
    };
    let name = t.improvements.name(m.kind as usize).unwrap_or("");
    let mut level = if name == "lighthouse" {
        x.explorers.len() as i32 + 1
    } else {
        m.level
    };
    let Some(d) = data(t, m.kind) else {
        return Ok(1);
    };
    let raw_cap = d.max_level.unwrap_or(0);
    // Fungi go one level higher for a tile owner with Microbes
    let cap = raw_cap
        + i32::from(name == "fungi" && crate::menu::player_ability(g, t, x.owner, "microbes"));
    if has(d, "patina") {
        let rate = d.growth_rate.unwrap_or(0);
        level = (g.current_turn - m.founded)
            .checked_div(rate)
            .ok_or("growth rate 0")?;
    } else if !d.adjacency_improvements.is_empty() {
        level = adjacency_bonus(g, t, i, d)?;
    }
    if name == "icebank" {
        return crate::polaris::ice_bank_level(g, t);
    }
    Ok(if raw_cap > 0 { level.min(cap) } else { level })
}

/// Surrounding check: level actions for adjacency improvements around `c`, center included
pub(crate) fn surrounding(
    g: &Game,
    t: &Tables,
    player: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    g.index(c).ok_or("surrounding tile")?;
    for x in c.x - 1..=c.x + 1 {
        for y in c.y - 1..=c.y + 1 {
            let at_c = Xy { x, y };
            let Some(i) = g.index(at_c) else { continue };
            let Some(m) = &g.tiles[i].improvement else {
                continue;
            };
            let Some(d) = t.improvements.get(m.kind as usize) else {
                continue;
            };
            if d.adjacency_improvements.is_empty() {
                continue;
            }
            update_level(g, t, player, at_c, out)?;
        }
    }
    Ok(())
}

/// Level update: the calculated level; one level action per step to it is queued
pub(crate) fn update_level(
    g: &Game,
    t: &Tables,
    player: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<i32, String> {
    let i = g.index(c).ok_or("level tile")?;
    let level = level_for(g, t, i)?;
    if let Some(m) = &g.tiles[i].improvement {
        if level < m.level {
            for _ in 0..m.level - level {
                out.push(Action::Economy(Economy::ImprovementLevelDown {
                    player,
                    target: c,
                    amount: 1,
                }));
            }
        } else {
            for _ in 0..level - m.level {
                out.push(Action::Economy(Economy::ImprovementLevelUp {
                    player,
                    target: c,
                }));
            }
        }
    }
    Ok(level)
}

fn score_child(player: i32, amount: i32, c: Xy) -> Action {
    Action::Payload(Payload::IncreaseScore {
        player,
        amount,
        source: c,
        delay: 0,
    })
}

/// Build: direct surrounding-area children first, then the buffered ones reversed
pub fn build(
    g: &mut Game,
    t: &Tables,
    player: i32,
    kind: i32,
    c: Xy,
    pay: bool,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let Some(i) = g.index(c) else {
        return Ok(());
    };
    let Some(d) = data(t, kind) else {
        return Ok(());
    };
    let Ok(p) = player_index(g, player) else {
        return Ok(());
    };
    let name = t.improvements.name(kind as usize)?;
    let (consumed, temporary) = (has(d, "consumed"), has(d, "discrete"));
    let mut changes_route = false;
    for e in &d.creates {
        changes_route |= e["terrain"].is_string() || e["effect"].is_string();
    }
    let (mut currency, mut population, mut score) = (0, 0, 0);
    for r in &d.rewards {
        currency += field(r, "currency");
        population += field(r, "population");
        score += field(r, "score").max(0);
    }
    // the children pushed from here are buffered: moved behind the surrounding-area ones at the end
    let start = out.len();
    if kind != ROAD && !consumed && !temporary {
        g.tiles[i].improvement = Some(crate::state::Improvement {
            kind,
            owner: 0,
            founder: player,
            level: 0,
            founded: g.current_turn,
            xp: 0,
            population: 0,
            production: 1,
            base_score: score,
            border_size: d.border_size.unwrap_or(0),
            upgrade: 0,
            connected_to_capital_of_player: 0,
            named: false,
            rewards: Default::default(),
            effects: Default::default(),
        });
        if g.side.names[i].is_some() {
            std::sync::Arc::make_mut(&mut g.side).names[i] = None;
        }
        if has(d, "patina") {
            let level = level_for(g, t, i)?;
            g.tiles[i].improvement.as_mut().unwrap().level = level;
        }
        let amount = improvement_score(t, g, i);
        if amount > 0 {
            out.push(score_child(player, amount, c));
        }
    }
    if name == "bridge" {
        // The bridge orientation is stored in the level: 1 when the anchors are east
        // and west only (an anchor is any tile that is not Water or Ocean)
        let anchor = |dx: i32, dy: i32| {
            g.index(Xy {
                x: c.x + dx,
                y: c.y + dy,
            })
            .is_some_and(|j| ![1, 2].contains(&g.tiles[j].terrain))
        };
        let horizontal = !(anchor(0, -1) && anchor(0, 1)) && anchor(1, 0) && anchor(-1, 0);
        g.tiles[i]
            .improvement
            .as_mut()
            .ok_or("bridge without its improvement")?
            .level = i32::from(horizontal);
    }
    if consumed && kind != 52 && g.tiles[i].resource.is_some() {
        out.push(Action::Economy(Economy::DestroyResource {
            player,
            target: c,
        }));
    }
    if kind == ROAD {
        out.push(Action::Economy(Economy::BuildRoad { player, target: c }));
    }
    if kind == ROAD || !d.routes.is_empty() || has(d, "bridge") || has(d, "fill") || changes_route {
        if kind != ROAD {
            out.push(Action::UpdateRoutes { player });
        }
        out.push(Action::UpdateCityConnections {
            actor: player,
            players: Seats::from_iter([player]),
        });
    }
    for _ in 0..currency {
        out.push(Action::Payload(Payload::IncreaseCurrency {
            player,
            source: c,
            delay: 40,
            amount: 1,
        }));
    }
    if pay {
        let cur = &mut g.players[p].action.currency;
        *cur = cur.wrapping_sub(d.cost.unwrap_or(0)).max(0);
    }
    let ruling = g.tiles[i].ruling_city;
    if g.index(ruling).is_some() {
        for _ in 0..population {
            out.push(Action::City(City::IncreasePopulation {
                player,
                source: c,
                target: ruling,
                delay: 60,
            }));
        }
    }
    if has(d, "flood") {
        out.push(Action::Combat(Combat::Flood {
            player,
            coordinates: c,
        }));
    }
    if has(d, "fill") {
        crate::aquarion::drain_tile(g, c)?;
    }
    for e in &d.creates {
        if let Some(n) = e["terrain"].as_str() {
            g.tiles[i].terrain = t.terrain.id(n)? as i32;
            g.tiles[i].resource = None;
        }
        if let Some(n) = e["resource"].as_str() {
            // created unless the player already sees a resource there
            if crate::infiltrate::visible_resource(g, t, player, c)?.is_none() {
                out.push(Action::Economy(Economy::CreateResource {
                    player,
                    kind: t.resources.id(n)? as i32,
                    target: c,
                    reason: 0,
                }));
            }
        }
        // EnchantAnimal: a free unit of the named kind (not resolved through the tribe)
        if let Some(n) = e["unit"].as_str() {
            out.push(Action::Economy(Economy::Train {
                player,
                kind: t.units.id(n)? as i32,
                target: c,
                cost: 0,
                direction_target: NULL,
                train_action_aborted: false,
            }));
        }
        // the Algae build's effect; Foam joins Algae on the Cute game skin
        if let Some(n) = e["effect"].as_str() {
            let effect = match n {
                "algae" => 4,
                "flooded" => 1,
                other => return Err(format!("creates effect {other} not implemented")),
            };
            let cute = crate::cymanti::effective_skin(g, g.players[p].skin) == 19;
            crate::cymanti::create_effect(g, i, effect, cute)?;
        }
    }
    if has(d, "manual") {
        if let Some(n) = g.tiles[i].unit {
            // the builder is spent; an Invisible one is revealed
            if crate::menu::unit_effect_list(g, g.units[n as usize].effects).contains(&3) {
                out.push(Action::MoveEffect(MoveEffect::Reveal {
                    player,
                    coordinates: c,
                    origin: Xy { x: -1, y: -1 },
                    show_popup: false,
                }));
            }
            let u = &mut g.units[n as usize];
            u.moved = true;
            u.attacked = true;
        }
    }
    let buffered = out.len();
    surrounding(g, t, player, c, out)?;
    if name == "icebank" {
        crate::polaris::ice_bank_levels(g, t, out)?;
    }
    if has(d, "unique") {
        g.players[p].built_unique_improvements.try_push(kind)?;
    }
    crate::step::buffered_last(out, start, buffered);
    Ok(())
}

/// Population at a level: rewards plus growth population per level when maxLevel > 0
pub(crate) fn population_at_level(d: &ImprovementData, level: i32) -> i32 {
    let mut v: i32 = d.rewards.iter().map(|r| field(r, "population")).sum();
    if d.max_level.unwrap_or(0) > 0 {
        for r in &d.growth_rewards {
            v = v.wrapping_add(field(r, "population").wrapping_mul(level));
        }
    }
    v
}

pub fn destroy(
    g: &mut Game,
    t: &Tables,
    player: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let i = g.index(c).ok_or("destroy tile")?;
    let score = improvement_score(t, g, i);
    out.push(Action::Combat(Combat::DecreaseScore {
        player,
        amount: score,
    }));
    let m = g.tiles[i]
        .improvement
        .ok_or("destroy without an improvement")?;
    // the children pushed from here are buffered: moved behind the surrounding-area ones at the end
    let start = out.len();
    if let Some(d) = data(t, m.kind) {
        let ruling = g.tiles[i].ruling_city;
        for _ in 0..population_at_level(d, m.level).max(0) {
            out.push(Action::City(City::DecreasePopulation {
                player,
                target: ruling,
                delay: 200,
            }));
        }
        for r in &d.terrain_requirements {
            if let Some(n) = r["resource"].as_str() {
                g.tiles[i].resource = Some(t.resources.id(n)? as i32);
            }
        }
        if !d.routes.is_empty() || has(d, "bridge") {
            out.push(Action::UpdateRoutes { player });
            out.push(Action::UpdateCityConnections {
                actor: player,
                players: Seats::from_iter([player]),
            });
            g.tiles[i].has_road = false;
        }
    }
    g.tiles[i].improvement = None;
    let buffered = out.len();
    surrounding(g, t, player, c, out)?;
    crate::step::buffered_last(out, start, buffered);
    Ok(())
}

/// The Economy actions ported so far. An invalid action, which the game removes, queues
/// nothing.
pub fn economy_action(
    g: &mut Game,
    t: &Tables,
    e: &Economy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    // (variant, player, target, kind, pay, amount)
    let (inner, player, c, kind, pay, amount) = match *e {
        Economy::Build {
            player,
            kind,
            target,
            pay,
        } => ("Build", player, target, kind, pay, 0),
        Economy::BuildRoad { player, target } => ("BuildRoad", player, target, 0, false, 0),
        Economy::DestroyResource { player, target } => {
            ("DestroyResource", player, target, 0, false, 0)
        }
        Economy::CreateResource {
            player,
            kind,
            target,
            ..
        } => ("CreateResource", player, target, kind, false, 0),
        Economy::ImprovementLevelUp { player, target } => {
            ("ImprovementLevelUp", player, target, 0, false, 0)
        }
        Economy::ImprovementLevelDown {
            player,
            target,
            amount,
        } => ("ImprovementLevelDown", player, target, 0, false, amount),
        _ => return Err("not an improvement action".into()),
    };
    match inner {
        "Build" => build(g, t, player, kind, c, pay, out),
        "BuildRoad" => {
            let i = g.index(c).ok_or("road tile")?;
            g.tiles[i].has_road = true;
            Ok(())
        }
        "DestroyResource" => {
            let i = g.index(c).ok_or("resource tile")?;
            if g.tiles[i].resource.is_none() {
                return Ok(());
            }
            surrounding(g, t, player, c, out)?;
            g.tiles[i].resource = None;
            Ok(())
        }
        "CreateResource" => {
            let Some(i) = g.index(c) else {
                return Ok(());
            };
            g.tiles[i].resource = Some(kind);
            surrounding(g, t, player, c, out)
        }
        "ImprovementLevelUp" | "ImprovementLevelDown" => {
            let down = inner == "ImprovementLevelDown";
            let i = g.index(c).ok_or("level tile")?;
            let Some(m) = g.tiles[i].improvement.as_ref().filter(|m| m.kind != 0) else {
                return Ok(());
            };
            let (current, calculated) = (m.level, level_for(g, t, i)?);
            if down && calculated >= current || !down && current >= calculated {
                return Ok(());
            }
            let amount = if down { amount } else { 1 };
            let m = g.tiles[i].improvement.as_mut().unwrap();
            if down && m.level == 0 {
                return Ok(());
            }
            m.level = i32::from(if down {
                (m.level as u16).wrapping_sub(amount as u16)
            } else {
                (m.level as u16).wrapping_add(1)
            });
            let kind = m.kind;
            let ruling = g.tiles[i].ruling_city;
            if let Some(d) = t.improvements.get(kind as usize) {
                for r in &d.growth_rewards {
                    let population = field(r, "population");
                    if population > 0 && (down || ruling != NULL) {
                        for _ in 0..population {
                            out.push(if down {
                                Action::City(City::DecreasePopulation {
                                    player,
                                    target: ruling,
                                    delay: 200,
                                })
                            } else {
                                Action::City(City::IncreasePopulation {
                                    player,
                                    source: c,
                                    target: ruling,
                                    delay: 60,
                                })
                            });
                        }
                    }
                    if !down && field(r, "score") > 0 {
                        out.push(score_child(player, field(r, "score"), c));
                    }
                }
            }
            surrounding(g, t, player, c, out)?;
            Ok(())
        }
        other => Err(format!("economy action {other} not implemented")),
    }
}

/// The City actions: population changes and the level-up with its trigger.
pub fn city_action(
    g: &mut Game,
    t: &Tables,
    inner: &str,
    player: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let i = g.index(c).ok_or("city action with no tile")?;
    let owner = g.tiles[i].owner;
    let is_city = g.tiles[i]
        .improvement
        .as_ref()
        .is_some_and(|m| m.kind == CITY);
    if is_city && inner != "IncreasePopulation" && player_index(g, owner).is_err() {
        return Err("city action on a city with no owner player".into());
    }
    if !is_city {
        return Ok(());
    }
    match inner {
        "IncreasePopulation" | "DecreasePopulation" => {
            let up = inner == "IncreasePopulation";
            let before = improvement_score(t, g, i);
            let m = g.tiles[i].improvement.as_mut().unwrap();
            m.xp += if up { 1 } else { -1 };
            m.population += if up { 1 } else { -1 };
            let after = improvement_score(t, g, i);
            if up {
                out.push(score_child(player, after.wrapping_sub(before), c));
                let m = g.tiles[i].improvement.as_ref().unwrap();
                let current = g.players[g.current_player_index as usize].action.id;
                if m.level < m.xp && owner == current {
                    out.push(Action::City(City::LevelUp { player, target: c }));
                }
            } else {
                let p = player_index(g, owner)?;
                g.players[p].score = i64::from(
                    (g.players[p].score as u32).wrapping_sub(before.wrapping_sub(after) as u32),
                );
            }
        }
        "LevelUp" => {
            let m = g.tiles[i].improvement.as_mut().unwrap();
            if m.level >= m.xp {
                return Ok(());
            }
            m.level += 1;
            m.xp -= m.level;
            // upgrade is a u16 in the game and wraps below zero
            m.upgrade = i32::from((m.upgrade as u16).wrapping_sub(1));
            out.push(score_child(owner, 50 - m.level * 5, c));
            let p = player_index(g, owner)?;
            if g.players[p].task(6).is_none() {
                out.push(Action::EnableTask {
                    player: owner,
                    task: METROPOLIS_TASK as u32,
                });
            }
            g.triggers.push(crate::state::Trigger {
                player: owner,
                opponent: 0,
                kind: CITY_LEVEL_UP,
                coordinates: c,
            });
            out.extend(task_notification(g, t, p, METROPOLIS_TASK)?);
        }
        other => return Err(format!("city action {other} not implemented")),
    }
    Ok(())
}
