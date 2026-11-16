//! Cymanti's rules: Swarm and Boost, Decompose, poison and spores, healing improvements and
//! Explode (the swarm, decompose, heal options, end-of-turn, Poison, spore and explode rules).
//! turn-end, Poison, spore and explode rules).

use crate::action::{Action, Combat, Economy, MoveEffect, Payload, TurnEffect};
use crate::state::{Game, Unit, Xy, COOL_DOWN, PEACE};
use crate::step::{player_index, relation_state};
use crate::tables::Tables;

const WATER: i32 = 1;
const OCEAN: i32 = 2;
const POISONED: i64 = 1;
const BOOSTED: i64 = 2;
const SWIFT: i64 = 6;
const CHARMED: i64 = 8;
const CUTE_SKIN: i32 = 19;
/// The improvement effect "decomposing"
const DECOMPOSING: i32 = 0;
const SPORES: &str = "spores";
const ALGAE_123: &str = "algae";
const ALGAE_124: &str = "algaespawn";

fn unit_has(t: &Tables, kind: i32, a: &str) -> bool {
    t.units
        .get(kind as usize)
        .is_some_and(|d| d.unit_abilities.has(a))
}

fn effects_mut(g: &mut Game, n: u32) -> Result<&mut crate::inline::Inline<i64, 9>, String> {
    let list = g.units[n as usize].effects as usize;
    g.effect_lists
        .get_mut(list)
        .ok_or_else(|| "effect list".into())
}

fn add_effect(g: &mut Game, n: u32, e: i64) -> Result<(), String> {
    let fx = effects_mut(g, n)?;
    if !fx.contains(&e) {
        fx.try_push(e)?;
    }
    Ok(())
}

fn remove_first(g: &mut Game, list: u32, e: i64) -> Result<(), String> {
    let fx = g.effect_lists.get_mut(list as usize).ok_or("effect list")?;
    if let Some(k) = fx.iter().position(|&v| v == e) {
        fx.remove(k);
    }
    Ok(())
}

/// The boost options: own units around `c` (x-major, not the centre) that are neither
/// Boosted nor Swift and lead no chain
pub(crate) fn boost_options(g: &Game, p: i32, c: Xy) -> Vec<Xy> {
    let mut out = Vec::new();
    for x in c.x - 1..=c.x + 1 {
        for y in c.y - 1..=c.y + 1 {
            let d = Xy { x, y };
            if d == c {
                continue;
            }
            let Some(u) = g.tile(d).and_then(|t| t.unit).map(|n| &g.units[n as usize]) else {
                continue;
            };
            let fx = crate::menu::unit_effect_list(g, u.effects);
            if u.owner != p || fx.contains(&BOOSTED) || fx.contains(&SWIFT) || u.leader != 0 {
                continue;
            }
            out.push(d);
        }
    }
    out
}

/// The Swarm (and Boost) command: the unit is spent and BoostOthers is queued; no command base
pub(crate) fn swarm_command(g: &mut Game, p: i32, c: Xy, out: &mut Vec<Action>) {
    if let Some(n) = g.tile(c).and_then(|t| t.unit) {
        g.units[n as usize].moved = true;
        g.units[n as usize].attacked = true;
    }
    out.push(Action::TurnEffect(TurnEffect::BoostOthers {
        player: p,
        coordinates: c,
    }));
}

/// BoostOthers: one Boost per boost option, in order
pub(crate) fn boost_others(g: &Game, p: i32, c: Xy, out: &mut Vec<Action>) {
    for coordinates in boost_options(g, p, c) {
        out.push(Action::TurnEffect(TurnEffect::Boost {
            player: p,
            coordinates,
        }));
    }
}

/// Boost: the unit there, if any, becomes Swift
pub(crate) fn boost(g: &mut Game, c: Xy) -> Result<(), String> {
    if let Some(n) = g.tile(c).and_then(|t| t.unit) {
        add_effect(g, n, SWIFT)?;
    }
    Ok(())
}

/// The Decompose command: queued only on an own improvement the catalog knows; no command base
pub(crate) fn decompose_command(
    g: &Game,
    t: &Tables,
    p: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let tile = g.tile(c).ok_or("decompose tile")?;
    let Some(m) = &tile.improvement else {
        return Ok(());
    };
    if tile.owner != p || m.kind == 0 || t.improvements.get(m.kind as usize).is_none() {
        return Ok(());
    }
    out.push(Action::TurnEffect(TurnEffect::Decompose {
        player: p,
        coordinates: c,
    }));
    Ok(())
}

/// Decomposing: the improvement is marked decomposing
pub(crate) fn decompose(g: &mut Game, c: Xy) -> Result<(), String> {
    let i = g.index(c).ok_or("decompose tile")?;
    let m = g.tiles[i]
        .improvement
        .as_mut()
        .ok_or("decompose without an improvement")?;
    if !m.effects.contains(&DECOMPOSING) {
        m.effects.try_push(DECOMPOSING)?;
    }
    Ok(())
}

/// The heal options: own units around `c` (and on it when `centre`) that are hurt or poisoned
pub(crate) fn heal_options(
    g: &Game,
    t: &Tables,
    c: Xy,
    p: i32,
    centre: bool,
) -> Result<Vec<Xy>, String> {
    let mut out = Vec::new();
    for x in c.x - 1..=c.x + 1 {
        for y in c.y - 1..=c.y + 1 {
            let d = Xy { x, y };
            if !centre && d == c {
                continue;
            }
            let Some(n) = g.tile(d).and_then(|x| x.unit) else {
                continue;
            };
            let u = &g.units[n as usize];
            if u.owner != p {
                continue;
            }
            let max = crate::combat::max_health(g, t, u)?;
            if max > u.health || crate::menu::unit_effect_list(g, u.effects).contains(&POISONED) {
                out.push(d);
            }
        }
    }
    Ok(out)
}

/// The end-of-turn upkeep of an own improvement: a Heal
/// improvement (the mycelium) heals the unit on it and its neighbours; a decomposing one pays
/// its cost back one star at a time and is destroyed, unless a unit stands on a bridge
pub(crate) fn improvement_upkeep(
    g: &Game,
    t: &Tables,
    p: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let tile = g.tile(c).ok_or("upkeep tile")?;
    let Some(m) = &tile.improvement else {
        return Ok(());
    };
    let d = t
        .improvements
        .get(m.kind as usize)
        .ok_or("end-command improvement data")?;
    if d.improvement_abilities.has("heal") {
        let options = heal_options(g, t, c, p, true)?;
        if options.contains(&c) {
            out.push(Action::TurnEffect(TurnEffect::Heal {
                player: p,
                coordinates: c,
                fixed_heal: 40,
            }));
        }
        if !options.is_empty() {
            out.push(Action::MoveEffect(MoveEffect::HealOthers {
                player: p,
                coordinates: c,
            }));
        }
    }
    if m.effects.contains(&DECOMPOSING)
        && (tile.unit.is_none() || !d.improvement_abilities.has("bridge"))
    {
        for _ in 0..d.cost.unwrap_or(0).max(0) {
            out.push(Action::Payload(Payload::IncreaseCurrency {
                player: p,
                delay: 40,
                source: c,
                amount: 1,
            }));
        }
        out.push(Action::MoveEffect(MoveEffect::DestroyImprovement {
            player: p,
            coordinates: c,
        }));
    }
    Ok(())
}

/// A poisoned unit is healed of the poison (and Charmed) instead of hit points; the passenger's
/// list is cleared too, which is the same list when they share it
pub(crate) fn cure(g: &mut Game, n: u32) -> Result<(), String> {
    let u = g.units[n as usize];
    let lists = [
        Some(u.effects),
        u.passenger.map(|p| g.units[p as usize].effects),
    ];
    for list in lists.into_iter().flatten() {
        remove_first(g, list, POISONED)?;
        remove_first(g, list, CHARMED)?;
    }
    Ok(())
}

/// Combat Poison: the unit on `target` (and its passenger) is Poisoned; Charmed too when the
/// actor has the Cute skin (on the carrier, as the game writes it)
pub(crate) fn poison(g: &mut Game, actor: i32, target: Xy) -> Result<(), String> {
    let Some(n) = g.tile(target).and_then(|x| x.unit) else {
        return Ok(());
    };
    add_effect(g, n, POISONED)?;
    let ai = player_index(g, actor)?;
    let cute = g.players[ai].skin == CUTE_SKIN;
    if cute {
        add_effect(g, n, CHARMED)?;
    }
    if let Some(p) = g.units[n as usize].passenger {
        add_effect(g, p, POISONED)?;
        if cute {
            add_effect(g, n, CHARMED)?;
        }
    }
    Ok(())
}

/// Spawning spores: spores for the current player, and on water an algae build
/// (the Algae improvement in rules123, AlgaeSpawn in rules124)
pub(crate) fn spores(g: &Game, t: &Tables, c: Xy, out: &mut Vec<Action>) -> Result<(), String> {
    let tile = g.tile(c).ok_or("spore tile")?;
    let actor = crate::step::mover(g);
    out.push(Action::Economy(Economy::CreateResource {
        player: actor,
        kind: t.resources.id(SPORES)? as i32,
        target: c,
        reason: 0,
    }));
    if tile.terrain == WATER || tile.terrain == OCEAN {
        let name = if g.fixed.version == 123 {
            ALGAE_123
        } else {
            ALGAE_124
        };
        out.push(Action::Economy(Economy::Build {
            player: actor,
            kind: t.improvements.id(name)? as i32,
            target: c,
            pay: false,
        }));
    }
    Ok(())
}

/// A unit stepping or trained onto a poison improvement is poisoned by the tile's owner,
/// unless its tribe resists poison or it flies
pub(crate) fn poison_tile(g: &Game, t: &Tables, n: u32, c: Xy) -> Option<Action> {
    let tile = g.tile(c)?;
    let m = tile.improvement.as_ref()?;
    if !t
        .improvements
        .get(m.kind as usize)
        .is_some_and(|d| d.improvement_abilities.has("poison"))
    {
        return None;
    }
    let u = &g.units[n as usize];
    let resists = g
        .player(u.owner)
        .and_then(|p| t.tribes.get(p.tribe as usize))
        .is_some_and(|d| d.tribe_abilities.has("poisonresist"));
    (g.player(u.owner).is_some() && !resists && !unit_has(t, u.kind, "fly")).then_some({
        Action::Combat(Combat::Poison {
            player: tile.owner,
            origin: c,
            target: c,
        })
    })
}

/// Exploding a unit: the carrier's passenger (or the unit) explodes; its follower explodes
/// first and its leader lets go; spores; every unit around not at peace takes half the blast,
/// is poisoned when it survives, and the actor's diplomacy and Pacifist task react; the actor's
/// own unit costs it score; the tile is emptied
pub(crate) fn explode(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let i = g.index(c).ok_or("explode tile")?;
    let Some(carrier) = g.tiles[i].unit else {
        return Ok(());
    };
    let selected = if unit_has(t, g.units[carrier as usize].kind, "carry") {
        g.units[carrier as usize]
            .passenger
            .ok_or("explode missing passenger")?
    } else {
        carrier
    };
    let u: Unit = g.units[selected as usize];
    if u.follower != 0 {
        if let Some(f) = crate::linked::find(g, u.follower) {
            out.push(Action::Combat(Combat::Explode {
                player: actor,
                coordinates: g.units[f as usize].coordinates,
                home_coordinates: Xy { x: 0, y: 0 },
            }));
        }
    }
    if u.leader != 0 {
        if let Some(l) = crate::linked::find(g, u.leader) {
            g.units[l as usize].follower = 0;
        }
    }
    spores(g, t, c, out)?;
    let area = crate::explore::area_sorted(g, c, 1)?.to_vec();
    let ai = player_index(g, actor)?;
    for k in area {
        let target = g.tiles[k].coordinates;
        let Some(n) = g.tiles[k].unit else { continue };
        let d = g.units[n as usize];
        let peace = d.owner == actor || {
            let r = relation_state(g, actor, d.owner)?;
            r == PEACE || r == COOL_DOWN
        };
        if peace {
            continue;
        }
        let dealt = crate::attack::battle_full(g, t, &u, &d)?.0;
        let damage = crate::attack::half_damage(dealt);
        if damage < d.health {
            out.push(Action::Combat(Combat::Poison {
                player: actor,
                origin: target,
                target,
            }));
        }
        out.push(Action::Combat(Combat::Attack {
            player: actor,
            origin: target,
            target,
            damage,
            animation: 2,
            should_move_to_target: false,
            delay: 100,
        }));
        player_index(g, d.owner)?;
        crate::step::attack_diplomacy(g, actor, d.owner, out)?;
        if g.fixed.achievements_active {
            return Err("achievement tracking not implemented".into());
        }
        crate::attack::reset_peace(g, ai);
    }
    if actor == u.owner {
        let amount = crate::units::unit_score(g, t, &u);
        out.push(Action::Combat(Combat::DecreaseScore {
            player: actor,
            amount,
        }));
    }
    g.tiles[i].unit = None;
    Ok(())
}

/// Whether an Explode unit can explode: not frozen, not trained this turn unless following a leader
pub(crate) fn can_explode(g: &Game, t: &Tables, u: &Unit) -> bool {
    unit_has(t, u.kind, "explode")
        && (g.current_turn != u.created_turn || u.leader != 0)
        && !crate::menu::unit_effect_list(g, u.effects).contains(&0)
}

/// Whether a build creates tile effect `e` (Algae, with Foam on the Cute game skin)
pub(crate) fn create_effect(g: &mut Game, i: usize, e: i32, cute: bool) -> Result<(), String> {
    let effects = &mut g.tiles[i].effects;
    if !effects.contains(&e) {
        effects.try_push(e)?;
    }
    const ALGAE: i32 = 4;
    const FOAM: i32 = 5;
    if e == ALGAE && cute && !effects.contains(&FOAM) {
        effects.try_push(FOAM)?;
    }
    Ok(())
}

/// The effective skin: the game's skin, or the player's when the game has none
pub(crate) fn effective_skin(g: &Game, player_skin: i32) -> i32 {
    let game = g.game_skin();
    if game == 0 {
        player_skin
    } else {
        game
    }
}
