//! The ordered v2 candidate menu, built family by family to match the game's bot menu.
//!
//! Order: a pending choice (city reward or peace reply) replaces everything; otherwise, for every tile
//! the mover has explored in row-major order: train, build, improvement abilities, unit actions, then
//! the own unit's moves and attacks; then research, diplomacy per known opponent, EndTurn, and in
//! two-player games Resign. Each family returns `None` until it is implemented, so the harness can
//! measure families separately; a position needing an unimplemented family is not served.

use crate::state::{Game, Xy};
use crate::state::{COOL_DOWN, PEACE};
use crate::tables::{unlocks, Tables};
use engine_fast_frame as nf;

/// Command type ids, as the frame's action.kind
pub mod kind {
    pub const CITY_REWARD: u64 = 11;
    pub const RESEARCH: u64 = 8;
    pub const END_TURN: u64 = 15;
    pub const RESIGN: u64 = 35;
    pub const PEACE_REPLY: u64 = 28;
    pub const END_MATCH: u64 = 22;
}

pub const STARTED: i64 = 2;
pub const FINAL_TURN: i64 = 3;
const CITY_LEVEL_UP: i64 = 1;
const PEACE_REQUEST: i64 = 2;
/// f32 bits of the literacy multiplier
const LITERACY: f32 = f32::from_bits(0x3f2aaa3b);

/// One menu candidate: the command and its details (the frame adds the id).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Cand {
    pub action: nf::Action,
    pub details: nf::Details,
}

/// A candidate of command `kind` with no other field; the methods below add fields.
pub(crate) fn cand(kind: u64) -> Cand {
    Cand {
        action: nf::Action {
            kind: kind as i32,
            ..Default::default()
        },
        details: nf::Details::default(),
    }
}

impl Cand {
    pub(crate) fn source(mut self, c: Xy) -> Self {
        self.action.source = Some(nf::Xy { x: c.x, y: c.y });
        self
    }
    pub(crate) fn target(mut self, c: Xy) -> Self {
        self.action.target = Some(nf::Xy { x: c.x, y: c.y });
        self
    }
    pub(crate) fn option(mut self, option: i32) -> Self {
        self.action.option = Some(option);
        self
    }
    pub(crate) fn opponent(mut self, opponent: i32) -> Self {
        self.action.opponent = Some(opponent);
        self
    }
    pub(crate) fn accept(mut self, accept: bool) -> Self {
        self.action.accept = Some(accept);
        self
    }
    pub(crate) fn price(mut self, price: i32) -> Self {
        self.details.price = Some(price);
        self
    }
}

/// The whole menu, or Err naming the first family this position needs that is not implemented.
pub fn menu(g: &Game, t: &Tables) -> Result<Vec<Cand>, String> {
    let _all = engine_fast_frame::timers::scope("menu");
    let mover = g.players[g.current_player_index as usize].action.id;
    let mut out = Vec::with_capacity(128);
    if let Some(trigger) = g.triggers.iter().rev().find(|x| x.player == mover) {
        out.extend(pending_choice(g, trigger)?);
        out.extend(resign(g));
        return Ok(out);
    }
    match g.current_state {
        STARTED => {}
        // FinalTurn without a pending reward: EndMatch alone, then the two-player Resign
        FINAL_TURN => return Ok(final_turn(g)),
        _ => return Err("phase other than Started/FinalTurn".into()),
    }
    let queued = !g.actions.is_empty() || g.current_command != g.commands;
    if queued {
        return Ok(Vec::new());
    }
    let mut unlocked: Option<Vec<usize>> = None;
    let mut builds: Vec<BuildOption> = Vec::new();
    let abilities = player_abilities(g, t, mover);
    // what the mover's units share in the move search, worked out at the first unit that moves
    let mut moving = None;
    for (i, tile) in g.tiles.iter().enumerate() {
        if !tile.explorers.contains(&mover) {
            continue;
        }
        train_into(g, t, mover, i, &mut out)?;
        // computed where the first tile's build family would compute it, then reused
        if unlocked.is_none() {
            let list = unlocked_improvements(g, t, mover)?;
            builds = build_options(g, t, mover, &list)?;
            unlocked = Some(list);
        }
        let known = unlocked.as_deref().unwrap_or_default();
        build_with(g, t, mover, i, &builds, abilities, &mut out)?;
        improvement_abilities_into(g, t, mover, i, abilities, &mut out);
        unit_actions_with(g, t, mover, i, Some(known), abilities, &mut out)?;
        crate::movement::moves_at(g, t, mover, i, &mut moving, &mut out)?;
        crate::combat::attacks_into(g, t, mover, i, &mut out)?;
    }
    {
        let _t = engine_fast_frame::timers::scope("menu.research and diplomacy");
        out.extend(research(g, t, mover)?);
        out.extend(diplomacy(g, t, mover)?);
    }
    once_each(&mut out);
    out.push(cand(kind::END_TURN));
    out.extend(resign(g));
    Ok(out)
}

/// Keep the first candidate of each distinct command, in order (the game adds each
/// command once: retained_ui_helpers `seen`).
fn once_each(out: &mut Vec<Cand>) {
    fn mix(h: u64, v: i64) -> u64 {
        (h ^ v as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .rotate_left(23)
    }
    fn hash(a: &nf::Action) -> u64 {
        let xy = |c: &Option<nf::Xy>| c.map_or(-1, |c| i64::from(c.x) << 16 ^ i64::from(c.y));
        let mut h = mix(0, i64::from(a.kind));
        h = mix(h, xy(&a.source));
        h = mix(h, xy(&a.target));
        h = mix(h, a.opponent.map_or(-1, i64::from));
        h = mix(h, a.option.map_or(-1, i64::from));
        mix(h, a.accept.map_or(-1, i64::from))
    }
    // open addressing over the kept candidates' positions; `EMPTY` marks a free slot
    const EMPTY: u32 = u32::MAX;
    let slots = (out.len() * 2).next_power_of_two();
    let mut table = vec![EMPTY; slots];
    let mut kept = 0;
    for i in 0..out.len() {
        let mut at = hash(&out[i].action) as usize & (slots - 1);
        let repeat = loop {
            match table[at] {
                EMPTY => break false,
                j if out[j as usize].action == out[i].action => break true,
                _ => at = (at + 1) & (slots - 1),
            }
        };
        if !repeat {
            table[at] = kept as u32;
            // everything between `kept` and `i` is a repeat, so the order of the kept is unchanged
            out.swap(kept, i);
            kept += 1;
        }
    }
    out.truncate(kept);
}

/// The final-turn menu: EndMatch, then Resign.
pub fn final_turn(g: &Game) -> Vec<Cand> {
    let mut out = vec![cand(kind::END_MATCH)];
    out.extend(resign(g));
    out
}

/// The game environment offers Resign only in a two-seat game, where it ends the match.
fn resign(g: &Game) -> Option<Cand> {
    (g.fixed.seats == 2).then(|| cand(kind::RESIGN))
}

/// City reward pair for a level-up, or accept-then-decline for a peace request.
pub fn pending_choice(g: &Game, trigger: &crate::state::Trigger) -> Result<Vec<Cand>, String> {
    let at = trigger.coordinates;
    match trigger.kind {
        CITY_LEVEL_UP => {
            let level = g
                .tile(at)
                .and_then(|t| t.improvement.as_ref())
                .filter(|i| i.kind == 1)
                .map(|i| i.level)
                .ok_or("city level-up trigger without a city")?;
            // City reward options: [Workshop, Explorer], [CityWall, Resources],
            // [PopulationGrowth, BorderGrowth], [Park, SuperUnit] for levels 2, 3, 4, >= 5
            let pairs = [[3, 4], [1, 7], [8, 5], [2, 6]];
            if level < 2 {
                return Err("city reward below level 2".into());
            }
            let pair = pairs[((level - 2) as usize).min(3)];
            Ok(pair
                .iter()
                .map(|&r| cand(kind::CITY_REWARD).target(at).option(r))
                .collect())
        }
        PEACE_REQUEST => {
            let opponent = trigger.opponent;
            Ok([true, false]
                .iter()
                .map(|&accept| cand(kind::PEACE_REPLY).opponent(opponent).accept(accept))
                .collect())
        }
        _ => Err("unknown pending trigger".into()),
    }
}

/// The tech tree's order for the tribe, with the tribe's overrides resolved.
pub(crate) fn tribe_tech(t: &Tables, tribe: i32) -> Result<&[usize], String> {
    t.tribe_tree(tribe)?.order.as_deref().map_err(Clone::clone)
}

/// Tech price: tier 0 is free, else ceil((cities - 1) * tier + tier + 4), times
/// the literacy multiplier
pub(crate) fn tech_price(g: &Game, t: &Tables, p: &crate::state::Player, tech: usize) -> i32 {
    let tier = t.techs.get(tech).map_or(0, |x| x.cost);
    if tier == 0 {
        return 0;
    }
    let mut x = (p.cities.wrapping_sub(1).wrapping_mul(tier)) as f32 + tier as f32 + 4.0;
    if player_ability(g, t, p.action.id, "literacy") {
        x *= LITERACY;
    }
    x.ceil() as i32
}

/// Unlockable techs: children of known techs, and an unknown parent of a known child,
/// in catalog tree order
pub(crate) fn unlockable_techs(g: &Game, t: &Tables, player: i32) -> Result<Vec<usize>, String> {
    let p = g.player(player).ok_or("missing player")?;
    let known = |id: usize| p.available_tech.contains(&(id as i32));
    let mut unlockable: Vec<usize> = Vec::new();
    let tree = t.tribe_tree(p.tribe)?;
    for &parent in tribe_tech(t, p.tribe)? {
        let children = tree.children[parent].as_deref().map_err(Clone::clone)?;
        if known(parent) {
            for &c in children {
                if !known(c) && !unlockable.contains(&c) {
                    unlockable.push(c);
                }
            }
        } else if children.iter().any(|&c| known(c)) && !unlockable.contains(&parent) {
            unlockable.push(parent);
        }
    }
    Ok(unlockable)
}

/// Research candidates: in `unlockable_techs` order (children of known parents, and an unknown
/// parent of a known child), filtered to what the mover can afford, each with its price.
pub fn research(g: &Game, t: &Tables, mover: i32) -> Result<Vec<Cand>, String> {
    let p = g.player(mover).ok_or("missing mover")?;
    let unlockable = unlockable_techs(g, t, mover)?;
    let mut out = Vec::new();
    for id in unlockable {
        let price = tech_price(g, t, p, id);
        if price <= p.action.currency {
            out.push(cand(kind::RESEARCH).option(id as i32).price(price));
        }
    }
    Ok(out)
}

pub mod kind_train {
    pub const TRAIN: u64 = 5;
}

/// Train candidates on one tile: an own city tile
/// without a unit; units unlocked by the mover's techs in their list order (duplicates kept), not hidden,
/// a `unique` unit only if none is owned, with its movement terrain next to the city; affordable, and the
/// ruling city below its unit support (units homed there <= city level).
pub fn train_at(g: &Game, t: &Tables, mover: i32, i: usize) -> Result<Vec<Cand>, String> {
    let mut out = Vec::new();
    train_into(g, t, mover, i, &mut out)?;
    Ok(out)
}

/// `train_at`, appending to `out`
fn train_into(
    g: &Game,
    t: &Tables,
    mover: i32,
    i: usize,
    out: &mut Vec<Cand>,
) -> Result<(), String> {
    let tile = &g.tiles[i];
    if tile.owner != mover
        || tile.unit.is_some()
        || !tile.improvement.as_ref().is_some_and(|x| x.kind == 1)
    {
        return Ok(());
    }
    let p = g.player(mover).ok_or("missing mover")?;
    if p.block_train_units {
        return Ok(());
    }
    let unit_of = |node: u32| g.units.get(node as usize);
    let on_board = || g.tiles.iter().filter_map(|t| t.unit).filter_map(unit_of);
    // support of the ruling city (the train check counts units homed at the city's coordinates)
    let home = g.tile(tile.ruling_city).and_then(|c| {
        c.improvement
            .as_ref()
            .filter(|x| x.kind == 1)
            .map(|x| (c.coordinates, x.level))
    });
    let tree = t.tribe_tree(p.tribe)?;
    for &tech in &p.available_tech {
        for &id in tree.units_of(tech)? {
            let u = t.units.get(id).unwrap();
            if u.hidden {
                continue;
            }
            if u.unit_abilities.has("unique")
                && on_board().any(|x| x.owner == mover && x.kind == id as i32)
            {
                continue;
            }
            if !u.movement_terrain.is_empty() {
                let allowed: Vec<i32> = u
                    .movement_terrain
                    .iter()
                    .map(|n| t.terrain.id(n).map(|x| x as i32))
                    .collect::<Result<_, _>>()?;
                // Terrain match: a neighbour of the listed terrain, wetland (Flooded or
                // Algae) counting as terrain 7
                let c = tile.coordinates;
                let near = (c.x - 1..=c.x + 1)
                    .flat_map(|x| (c.y - 1..=c.y + 1).map(move |y| Xy { x, y }))
                    .filter(|&xy| xy != c)
                    .filter_map(|xy| g.tile(xy))
                    .any(|n| {
                        allowed.contains(&n.terrain)
                            || allowed.contains(&7) && crate::aquarion::wetland(n)
                    });
                if !near {
                    continue;
                }
            }
            if p.action.currency < u.cost {
                continue;
            }
            let Some((at, level)) = home else { continue };
            let count = on_board().filter(|x| x.home == at).count() as i32;
            if count > level {
                continue;
            }
            out.push(
                cand(kind_train::TRAIN)
                    .target(tile.coordinates)
                    .option(id as i32)
                    .price(u.cost),
            );
        }
    }
    Ok(())
}

pub mod kind_build {
    pub const BUILD: u64 = 1;
}

const ROAD: usize = 3;
const BRIDGE: usize = 48;
const FOREST: i32 = 5;
const WATER: i32 = 1;
const OCEAN: i32 = 2;
const FLOODED: i32 = 1;
const ALGAE: i32 = 4;
const INVISIBLE: i64 = 3;

pub(crate) fn effects(tile: &crate::state::Tile) -> &[i32] {
    &tile.effects
}

pub(crate) fn relation(g: &Game, me: i32, other: i32) -> i32 {
    if other == 0 {
        return 0;
    }
    g.player(me)
        .and_then(|p| p.relation(other))
        .map_or(0, |r| r.state)
}

/// The unit on an explored tile, hidden units not included (the viewer's own view):
/// unless it is invisible, foreign, and not at peace with the viewer.
pub fn visible_unit<'a>(
    g: &'a Game,
    tile: &crate::state::Tile,
    viewer: i32,
) -> Option<&'a crate::state::Unit> {
    if !tile.explorers.contains(&viewer) {
        return None;
    }
    let u = g.units.get(tile.unit? as usize)?;
    let invisible = unit_effect_list(g, u.effects).contains(&INVISIBLE);
    (u.owner == viewer || !invisible || relation(g, viewer, u.owner) == PEACE).then_some(u)
}

/// The mover's unlocked improvements in the game's order: tech improvementUnlocks in unlocked-tech order
/// (not hidden), then completed tasks' unlocks. Duplicates are kept; the menu deduplicates.
fn unlocked_improvements(g: &Game, t: &Tables, mover: i32) -> Result<Vec<usize>, String> {
    let p = g.player(mover).ok_or("missing mover")?;
    let mut out = Vec::new();
    let tree = t.tribe_tree(p.tribe)?;
    for &tech in &p.available_tech {
        for &id in tree.improvements_of(tech)? {
            if !t.improvements.get(id).unwrap().hidden {
                out.push(id);
            }
        }
    }
    for task in &p.action.tasks {
        if task.flags.completed {
            let name = task.kind.name().to_lowercase();
            for u in &t.tasks.get(t.tasks.id(&name)?).unwrap().improvement_unlocks {
                out.push(t.improvements.id(u)?);
            }
        }
    }
    Ok(out)
}

/// Build candidates on one tile (unlocked improvements except `manual`,
/// each allowed by `can_build` and affordable).
pub fn build_at(g: &Game, t: &Tables, mover: i32, i: usize) -> Result<Vec<Cand>, String> {
    let options = build_options(g, t, mover, &unlocked_improvements(g, t, mover)?)?;
    let mut out = Vec::new();
    build_with(
        g,
        t,
        mover,
        i,
        &options,
        player_abilities(g, t, mover),
        &mut out,
    )?;
    Ok(out)
}

/// An improvement the mover could build somewhere this turn.
struct BuildOption {
    kind: usize,
    cost: i32,
    /// buildable outside the mover's territory
    freelance: bool,
}

/// The tile-independent part of the build family: the unlocked kinds in order, each once,
/// that are not unit-built (`manual`) or hidden and that the mover can pay for.
fn build_options(
    g: &Game,
    t: &Tables,
    mover: i32,
    unlocked: &[usize],
) -> Result<Vec<BuildOption>, String> {
    let stars = g.player(mover).ok_or("missing mover")?.action.currency;
    let mut out: Vec<BuildOption> = Vec::new();
    for &kind in unlocked {
        let d = t.improvements.get(kind).unwrap();
        // a catalog entry without cost reads as 0 (monuments: the game shows price 0)
        let cost = d.cost.unwrap_or(0);
        if d.improvement_abilities.has("manual")
            || d.hidden
            || stars < cost
            || out.iter().any(|o| o.kind == kind)
        {
            continue;
        }
        out.push(BuildOption {
            kind,
            cost,
            freelance: d.improvement_abilities.has("freelance"),
        });
    }
    Ok(out)
}

/// `build_at` given the mover's build options and abilities (the same for every tile),
/// appending to `out`
fn build_with(
    g: &Game,
    t: &Tables,
    mover: i32,
    i: usize,
    options: &[BuildOption],
    abilities: u128,
    out: &mut Vec<Cand>,
) -> Result<(), String> {
    let tile = &g.tiles[i];
    for o in options {
        // outside the mover's territory only freelance improvements pass `can_build`
        if tile.owner != mover && !o.freelance {
            continue;
        }
        if !can_build(g, t, mover, tile, o.kind, abilities)? {
            continue;
        }
        out.push(
            cand(kind_build::BUILD)
                .target(tile.coordinates)
                .option(o.kind as i32)
                .price(o.cost),
        );
    }
    Ok(())
}

/// Whether the player may build the improvement on the tile.
fn can_build(
    g: &Game,
    t: &Tables,
    mover: i32,
    tile: &crate::state::Tile,
    kind: usize,
    abilities: u128,
) -> Result<bool, String> {
    let d = t.improvements.get(kind).unwrap();
    let has = |a: &str| d.improvement_abilities.has(a);
    if d.hidden {
        return Ok(false);
    }
    if tile.owner != mover && (!has("freelance") || tile.owner != 0 && !has("manual")) {
        return Ok(false);
    }
    let fx = effects(tile);
    if kind == ROAD {
        let road_there = tile.has_road
            || tile
                .improvement
                .as_ref()
                .is_some_and(|x| x.kind == 1 || x.kind == BRIDGE as i32);
        if road_there {
            return Ok(false);
        }
    } else if tile.improvement.is_some() && !has("discrete") {
        return Ok(false);
    }
    if kind == BRIDGE {
        let anchor = |dx: i32, dy: i32| {
            g.tile(Xy {
                x: tile.coordinates.x + dx,
                y: tile.coordinates.y + dy,
            })
            .is_some_and(|n| (n.terrain as u32).wrapping_sub(3) < 0xffff_fffe)
        };
        let none = !(anchor(0, -1) && anchor(0, 1)) && !(anchor(1, 0) && anchor(-1, 0));
        if none || fx.contains(&FLOODED) {
            return Ok(false);
        }
    }
    let visible = visible_unit(g, tile, mover);
    match visible {
        Some(u) if u.owner != mover && relation(g, mover, u.owner) != PEACE => return Ok(false),
        None if has("manual") => return Ok(false),
        _ => {}
    }
    if has("limited") {
        let city = tile.ruling_city;
        // only tiles within two of the city can match
        let mut near = (city.y - 2..=city.y + 2)
            .flat_map(|y| (city.x - 2..=city.x + 2).map(move |x| Xy { x, y }))
            .filter_map(|c| g.tile(c));
        let taken = near.any(|x| {
            x.coordinates != city
                && x.ruling_city == city
                && x.improvement
                    .as_ref()
                    .is_some_and(|m| m.kind == kind as i32)
        });
        if taken {
            return Ok(false);
        }
    }
    if has("unique") {
        let built = g
            .player(mover)
            .is_some_and(|p| p.built_unique_improvements.contains(&(kind as i32)));
        if built {
            return Ok(false);
        }
    }
    let reqs = t.requirements(kind);
    for entry in &reqs.creates {
        let effect = match entry.effect.as_deref() {
            None => 0,
            Some("flooded") => FLOODED,
            Some("algae") => ALGAE,
            Some(e) => return Err(format!("creates effect {e} not implemented")),
        };
        let resource = entry.resource.clone().transpose()?;
        if resource.is_some_and(|r| r != 0 && tile.resource == Some(r)) && effect == 0
            || effect != 0 && fx.contains(&effect) && resource.is_none()
        {
            return Ok(false);
        }
    }
    // Flood (Canal): not on a flooded tile; Fill: something to drain, and no unit stranded by it
    if has("flood") && fx.contains(&FLOODED) {
        return Ok(false);
    }
    if has("fill") {
        if !fx.contains(&FLOODED) && !fx.contains(&ALGAE) {
            return Ok(false);
        }
        if let Some(u) = visible {
            let ua = |a: &str| {
                t.units
                    .get(u.kind as usize)
                    .is_some_and(|d| d.unit_abilities.has(a))
            };
            if ua("water") && fx.contains(&FLOODED) || ua("land") && fx.contains(&ALGAE) {
                return Ok(false);
            }
        }
    }
    let wetland = fx.contains(&FLOODED) || fx.contains(&ALGAE);
    let flooded = fx.contains(&FLOODED);
    let (mut has_res, mut res_ok, mut has_ter, mut ter_ok) = (false, false, false, false);
    for req in &reqs.terrain {
        if let Some(r) = &req.resource {
            let r = r.clone()?;
            if r != 0 {
                has_res = true;
                res_ok |= tile.resource == Some(r);
            }
        }
        if let Some(x) = &req.terrain {
            let x = x.clone()?;
            if x != 0 {
                has_ter = true;
                ter_ok |= tile.terrain == x
                    || wetland && tile.terrain != FOREST && x == 7
                    || flooded && tile.terrain == FOREST && x == 8;
                let ability = |name: &str| unlocks(abilities, name);
                if x == 3 && tile.terrain == FOREST {
                    ter_ok |= ability("treehouse");
                }
                if x == 3
                    && (tile.terrain == WATER || tile.terrain == OCEAN)
                    && ![3, 17, 52, 14].contains(&kind)
                {
                    ter_ok |= ability("pontoon");
                }
            }
        }
    }
    if has_res && !res_ok || has_ter && !ter_ok {
        return Ok(false);
    }
    adjacency(g, tile, &reqs.adjacency)
}

/// Whether the tile meets the improvement's adjacency requirement.
fn adjacency(
    g: &Game,
    tile: &crate::state::Tile,
    reqs: &[crate::tables::Names],
) -> Result<bool, String> {
    if reqs.is_empty() {
        return Ok(true);
    }
    let positive = reqs.iter().any(|e| e.positive);
    let c = tile.coordinates;
    for x in c.x - 1..=c.x + 1 {
        for y in c.y - 1..=c.y + 1 {
            if x == c.x && y == c.y {
                continue;
            }
            let Some(o) = g.tile(Xy { x, y }) else {
                continue;
            };
            for e in reqs {
                if let Some(no) = &e.not_improvement {
                    let no = no.clone()?;
                    if o.improvement.as_ref().is_some_and(|m| m.kind == no) {
                        return Ok(false);
                    }
                }
                if o.owner != tile.owner {
                    continue;
                }
                if let Some(m) = &e.improvement {
                    let m = m.clone()?;
                    if o.improvement.as_ref().is_some_and(|i| i.kind == m) {
                        return Ok(true);
                    }
                }
                if let Some(x) = &e.terrain {
                    let x = x.clone()?;
                    if x != 0 && o.terrain == x {
                        return Ok(true);
                    }
                }
                if let Some(r) = &e.resource {
                    let r = r.clone()?;
                    if r != 0 {
                        let actual = o
                            .resource
                            .ok_or("adjacency resource on a tile without one")?;
                        if actual == r {
                            return Ok(true);
                        }
                    }
                }
            }
        }
    }
    Ok(!positive)
}

/// Whether some known tech unlocks the player ability.
#[inline(always)]
pub(crate) fn player_ability(g: &Game, t: &Tables, player: i32, name: &str) -> bool {
    unlocks(player_abilities(g, t, player), name)
}

/// The player abilities `player`'s techs unlock, as a mask for `unlocks` (none for a missing player).
pub(crate) fn player_abilities(g: &Game, t: &Tables, player: i32) -> u128 {
    g.player(player)
        .map_or(0, |p| t.abilities(&p.available_tech))
}

pub mod kind_diplomacy {
    pub const PEACE_TREATY: u64 = 27;
    pub const BREAK_PEACE: u64 = 29;
    pub const ESTABLISH_EMBASSY: u64 = 30;
}

const PEACE_REQUEST_MESSAGE: i64 = 1;

/// Whether the player is alive under the match's death condition
pub(crate) fn alive(g: &Game, player: i32) -> Result<bool, String> {
    let cities = || {
        g.tiles
            .iter()
            .any(|x| x.owner == player && x.improvement.as_ref().is_some_and(|m| m.kind == CITY))
    };
    let units = || {
        g.tiles
            .iter()
            .filter_map(|x| x.unit)
            .any(|n| g.units.get(n as usize).is_some_and(|u| u.owner == player))
    };
    Ok(player == 255
        || match g.fixed.death_condition {
            0 => cities(),
            1 => cities() && units(),
            _ => true,
        })
}

fn embassy_level(p: &crate::state::Player, other: i32) -> i32 {
    p.relation(other).map_or(0, |r| r.embassy_level)
}

/// The diplomacy candidates (each command passing its validator): per
/// known, present, living opponent, PeaceTreaty or BreakPeace,
/// then EstablishEmbassy with its price. `mover` must be the current player with no pending trigger.
pub fn diplomacy(g: &Game, t: &Tables, mover: i32) -> Result<Vec<Cand>, String> {
    let me = g.player(mover).ok_or("missing mover")?;
    let mut out = Vec::new();
    for &opponent in &me.known_players {
        if opponent == mover || opponent == 0 || opponent == 255 {
            continue;
        }
        let Some(other) = g.player(opponent) else {
            continue;
        };
        if !alive(g, opponent)? {
            continue;
        }
        let peace = relation(g, mover, opponent) == PEACE;
        if !peace && player_ability(g, t, mover, "peacetreaty") {
            let requested = other
                .messages
                .iter()
                .any(|m| m.kind == PEACE_REQUEST_MESSAGE && m.sender == mover);
            if !requested {
                out.push(cand(kind_diplomacy::PEACE_TREATY).opponent(opponent));
            }
        } else if peace {
            out.push(cand(kind_diplomacy::BREAK_PEACE).opponent(opponent));
        }
        // the embassy goes to the opponent's original capital, which must hold an opponent city or unit
        let start = other.start_tile;
        let capital = g.tile(start).ok_or("missing original capital tile")?;
        if capital.owner != opponent
            || !player_ability(g, t, mover, "embassy")
            || !(capital.improvement.as_ref().is_some_and(|m| m.kind == CITY)
                || capital
                    .unit
                    .and_then(|n| g.units.get(n as usize))
                    .is_some_and(|u| u.owner == opponent))
        {
            continue;
        }
        let cost = g
            .players
            .iter()
            .filter(|p| embassy_level(p, mover) > 0)
            .count() as i32
            * 2
            + 4;
        if me.action.currency >= cost && embassy_level(other, mover) == 0 {
            out.push(
                cand(kind_diplomacy::ESTABLISH_EMBASSY)
                    .opponent(opponent)
                    .price(cost),
            );
        }
    }
    Ok(out)
}

pub mod kind_improvement {
    pub const DESTROY: u64 = 9;
    pub const DECOMPOSE: u64 = 26;
}

/// The improvement-ability query on one tile (each command
/// passing its validator): Destroy, then Decompose. `mover` must be the current player with no pending
/// trigger (as `menu` guarantees); the validators then only require an improvement, which this checks.
pub fn improvement_abilities_at(g: &Game, t: &Tables, mover: i32, i: usize) -> Vec<Cand> {
    let mut out = Vec::new();
    improvement_abilities_into(g, t, mover, i, player_abilities(g, t, mover), &mut out);
    out
}

/// `improvement_abilities_at` given the mover's abilities, appending to `out`
fn improvement_abilities_into(
    g: &Game,
    t: &Tables,
    mover: i32,
    i: usize,
    abilities: u128,
    out: &mut Vec<Cand>,
) {
    let tile = &g.tiles[i];
    let Some(imp) = tile.improvement.as_ref() else {
        return;
    };
    // Kinds up to 47 outside the literal bitset (City, Ruin, LightHouse) are not destroyable
    let kind = imp.kind as u32;
    if kind <= 47 && (0x7fff_ffff_fff9_u64 >> (kind & 63)) & 1 == 0 || tile.owner != mover {
        return;
    }
    if visible_unit(g, tile, mover)
        .is_some_and(|u| u.owner != mover && relation(g, mover, u.owner) != PEACE)
    {
        return;
    }
    let data = t.improvements.get(imp.kind as usize);
    if tile.unit.is_some() && data.is_some_and(|d| d.improvement_abilities.has("bridge")) {
        return;
    }
    if imp.kind == 0 || data.is_none() {
        return;
    }
    let at = tile.coordinates;
    if unlocks(abilities, "destroy") {
        out.push(cand(kind_improvement::DESTROY).target(at));
    }
    let decomposing = imp.effects.contains(&0);
    if !decomposing && unlocks(abilities, "decompose") {
        out.push(cand(kind_improvement::DECOMPOSE).target(at));
    }
}

pub mod kind_unit {
    pub const RECOVER: u64 = 3;
    pub const HEAL_OTHERS: u64 = 4;
    pub const CAPTURE: u64 = 7;
    pub const DISBAND: u64 = 10;
    pub const PROMOTE: u64 = 13;
    pub const EXAMINE_RUINS: u64 = 14;
    pub const UPGRADE: u64 = 16;
    pub const FREEZE_AREA: u64 = 17;
    pub const BREAK_ICE: u64 = 18;
    pub const CLEAR_TILE_EFFECT: u64 = 39;
    pub const EXPLODE: u64 = 24;
    pub const BOOST: u64 = 25;
    pub const SWARM: u64 = 38;
    pub const DISEMBARK: u64 = 36;
}

const FROZEN: i64 = 0;
const POISONED: i64 = 1;
const CITY: i32 = 1;

pub(crate) fn unit_effects<'a>(g: &'a Game, u: &crate::state::Unit) -> &'a [i64] {
    unit_effect_list(g, u.effects)
}

/// the unit-graph effect list at `index` (0 is the shared empty list)
pub(crate) fn unit_effect_list(g: &Game, index: u32) -> &[i64] {
    g.effect_lists.get(index as usize).map_or(&[], |e| &e[..])
}

/// The unit-action query on one tile (each command passing its
/// validator): Capture, ExamineRuins, Recover, HealOthers, Disembark, Promote, FreezeArea, BreakIce,
/// Upgrade, Disband, Boost, Swarm, Explode, `manual` builds, then ClearTileEffect.
/// `mover` must be the current player with no pending trigger (as `menu` guarantees): the validators'
/// current-player and trigger checks are not repeated here.
pub fn unit_actions_at(g: &Game, t: &Tables, mover: i32, i: usize) -> Result<Vec<Cand>, String> {
    let mut out = Vec::new();
    unit_actions_with(
        g,
        t,
        mover,
        i,
        None,
        player_abilities(g, t, mover),
        &mut out,
    )?;
    Ok(out)
}

/// `unit_actions_at`, appending to `out`; `unlocked` is the mover's unlocked improvements when
/// the caller has them, otherwise they are computed if a unit can build; `abilities` is the
/// mover's ability mask (`player_abilities`)
fn unit_actions_with(
    g: &Game,
    t: &Tables,
    mover: i32,
    i: usize,
    unlocked: Option<&[usize]>,
    abilities: u128,
    out: &mut Vec<Cand>,
) -> Result<(), String> {
    let tile = &g.tiles[i];
    let Some(node) = tile.unit else {
        return Ok(());
    };
    let unit_at = |n: u32| {
        g.units
            .get(n as usize)
            .ok_or_else(|| "unit node out of range".to_string())
    };
    let u = unit_at(node)?;
    if u.owner != mover {
        return Ok(());
    }
    let data = |u: &crate::state::Unit| {
        t.units
            .get(u.kind as usize)
            .ok_or_else(|| "unknown unit kind".to_string())
    };
    let has = |u: &crate::state::Unit, a: &str| -> Result<bool, String> {
        Ok(data(u)?.unit_abilities.has(a))
    };
    let c = tile.coordinates;
    let neighbours = || {
        (c.x - 1..=c.x + 1)
            .flat_map(move |x| (c.y - 1..=c.y + 1).map(move |y| Xy { x, y }))
            .filter(move |&xy| xy != c)
            .filter_map(|xy| g.tile(xy))
    };
    let ufx = unit_effects(g, u);
    let frozen = ufx.contains(&FROZEN);
    let can_move = !u.moved && !frozen;
    let can_attack = !u.attacked && !frozen && (!u.moved || has(u, "dash")?);
    let leader = u.leader == 0;
    let passenger = u.passenger.map(unit_at).transpose()?;
    // max health: a carrier reads its passenger unless the carrier has ability 39
    let max_health = |u: &crate::state::Unit| -> Result<i32, String> {
        let s = match u.passenger {
            Some(p) if !has(u, "protect")? => unit_at(p)?,
            _ => u,
        };
        Ok(data(s)?
            .health
            .wrapping_add(s.promotion_level.wrapping_mul(50)))
    };
    let action = |kind: u64| cand(kind).source(c);
    let improvement = tile.improvement.as_ref().map(|m| m.kind);
    let rel = relation(g, mover, tile.owner);
    if can_move
        && can_attack
        && leader
        && tile.owner != mover
        && improvement == Some(CITY)
        && rel != PEACE
        && rel != COOL_DOWN
    {
        out.push(action(kind_unit::CAPTURE));
    }
    if can_move && can_attack && leader && improvement == Some(t.improvements.id("ruin")? as i32) {
        out.push(action(kind_unit::EXAMINE_RUINS));
    }
    if leader
        && (u.health < max_health(u)? || ufx.contains(&POISONED))
        && !u.moved
        && !frozen
        && !u.attacked
    {
        out.push(action(kind_unit::RECOVER));
    }
    if has(u, "heal")? && can_move && !u.attacked {
        let mut hurt = false;
        for n in neighbours() {
            if let Some(o) = n
                .unit
                .map(unit_at)
                .transpose()?
                .filter(|o| o.owner == mover)
            {
                hurt |= max_health(o)? > o.health || unit_effects(g, o).contains(&POISONED);
            }
        }
        if hurt {
            out.push(action(kind_unit::HEAL_OTHERS));
        }
    }
    if passenger.is_some() {
        // Disembark: on wetland, or on an improvement with the Bridge ability
        let port = crate::aquarion::wetland(tile)
            || improvement
                .filter(|&k| k != 0)
                .and_then(|k| t.improvements.get(k as usize))
                .is_some_and(|d| d.improvement_abilities.has("bridge"));
        if port {
            out.push(action(kind_unit::DISEMBARK));
        }
    }
    let selected = passenger.unwrap_or(u);
    if !has(selected, "static")? && !data(selected)?.hidden {
        let discipline = if selected.owner == mover {
            unlocks(abilities, "discipline")
        } else {
            player_ability(g, t, selected.owner, "discipline")
        };
        let limit = 3 ^ i32::from(discipline);
        if limit > 0 && selected.xp >= limit && selected.promotion_level == 0 {
            out.push(action(kind_unit::PROMOTE));
        }
    }
    if crate::polaris::can_freeze_area(g, t, u)? {
        out.push(action(kind_unit::FREEZE_AREA));
    }
    if crate::polaris::can_break_ice(g, t, u) {
        // the game's candidate carries the command's `what` field, 0 until displayed
        out.push(action(kind_unit::BREAK_ICE).option(0));
    }
    if !has(u, "grow")? {
        let p = g.player(mover).ok_or("missing mover")?;
        let territory = tile.owner == mover || tile.owner != 0 && rel == PEACE;
        crate::units::upgradable(g, t, mover, u.kind, |id| {
            let d = t.units.get(id).unwrap();
            if territory && p.action.currency >= d.cost {
                out.push(action(kind_unit::UPGRADE).option(id as i32).price(d.cost));
            }
            true
        })?;
    }
    if can_move && !u.attacked && leader && unlocks(abilities, "disband") {
        out.push(action(kind_unit::DISBAND));
    }
    // Boost and Swarm: a unit that can move, with someone to boost
    if can_move && crate::step::mover(g) == mover {
        for (ability, kind) in [("boost", kind_unit::BOOST), ("swarm", kind_unit::SWARM)] {
            if has(u, ability)? && !crate::cymanti::boost_options(g, mover, c).is_empty() {
                out.push(action(kind));
            }
        }
    }
    if crate::cymanti::can_explode(g, t, u) {
        out.push(action(kind_unit::EXPLODE));
    }
    if can_attack && !u.moved && !frozen {
        let p = g.player(mover).ok_or("missing mover")?;
        let computed;
        let unlocked = match unlocked {
            Some(u) => u,
            None => {
                computed = unlocked_improvements(g, t, mover)?;
                &computed
            }
        };
        for &kind in unlocked {
            let d = t.improvements.get(kind).unwrap();
            if !d.improvement_abilities.has("manual")
                || !can_build(g, t, mover, tile, kind, abilities)?
            {
                continue;
            }
            let cost = d.cost.unwrap_or(0);
            if p.action.currency >= cost {
                out.push(
                    cand(kind_build::BUILD)
                        .target(c)
                        .option(kind as i32)
                        .price(cost),
                );
            }
        }
    }
    for effect in crate::aquarion::clear_options(g, t, mover, u, tile, can_move, can_attack)?
        .into_iter()
        .flatten()
    {
        out.push(action(kind_unit::CLEAR_TILE_EFFECT).option(effect));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_each_keeps_the_first_of_each_command_in_order() {
        let at = |x, y| Xy { x, y };
        let a = cand(6).source(at(1, 2)).target(at(3, 4));
        let b = cand(6).source(at(1, 2)).target(at(4, 3));
        let c = cand(1).option(5);
        // the same command with other details is still a repeat
        let a_again = a.clone().price(9);
        let mut list = vec![
            a.clone(),
            b.clone(),
            a_again,
            c.clone(),
            b.clone(),
            cand(1).option(6),
        ];
        once_each(&mut list);
        assert_eq!(list, [a, b, c, cand(1).option(6)]);
        let mut empty: Vec<Cand> = Vec::new();
        once_each(&mut empty);
        assert!(empty.is_empty());
    }
}
