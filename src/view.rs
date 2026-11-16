//! The player view: what one player may know of the game, as plain
//! integer planes. A pure function of the state and the viewer: it never writes to the game,
//! and it reads nothing from a tile the viewer has not explored except the ruin hint its
//! tribe ability grants. No scaling, no buckets, no display names.

use crate::state::{Game, PEACE};
use crate::tables::Tables;

/// "not known to this player"
pub const UNKNOWN: i16 = -1;

/// The per-tile planes, in order. Each has one entry per tile, at index `y * width + x`.
pub const TILE_PLANES: [&str; 29] = [
    "explored",
    "terrain",
    "climate",
    "resource",
    "improvement",
    "improvement_level",
    "owner",
    "capital_of",
    "ruling_city",
    "road",
    "route",
    "effects",
    "ruin_hint",
    "unit_kind",
    "unit_owner",
    "unit_health",
    "unit_veteran",
    "unit_effects",
    "unit_passenger",
    "unit_moved",
    "unit_attacked",
    "unit_kills",
    "unit_home",
    "city_population",
    "city_connected",
    "city_progress",
    "city_wall",
    "city_parks",
    "city_super_units",
];

/// The index of a tile plane by name (a preparation resolves this once).
pub fn plane(name: &str) -> Option<usize> {
    TILE_PLANES.iter().position(|p| *p == name)
}

/// One player's entry in the view: the viewer itself, or a player it has met.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerView {
    pub seat: i16,
    pub tribe: i16,
    /// relation state of the viewer toward this player (0 for the viewer itself)
    pub relation: i16,
    pub score: i64,
}

/// One of the viewer's tasks, as stored.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TaskView {
    /// the task's id (`state::TaskKind`)
    pub kind: i16,
    pub completed: bool,
    /// the counter the task keeps (peaceful turns, kills or conversions), UNKNOWN if it
    /// keeps none: the other tasks are counts over the view
    pub counter: i32,
}

/// A lighthouse on a board corner the viewer has explored.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LighthouseView {
    pub tile: i16,
    /// the seats that have explored it, in the order they did (the game draws it); 0 past
    /// the end
    pub explorers: [i16; 5],
}

/// The view of one player. `tiles` is plane-major: plane `p` is
/// `tiles[p * width * height..][..width * height]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub viewer: i16,
    pub turn: i32,
    pub width: i16,
    pub height: i16,
    pub rules: i16,
    pub mode: i16,
    pub tiles: Vec<i16>,
    /// the viewer's stars
    pub stars: i32,
    /// the viewer's techs, ascending
    pub tech: Vec<i16>,
    /// the resource kinds the viewer's tech shows, as bits by kind
    pub revealed_resources: u32,
    /// the viewer's start tile
    pub start_tile: i16,
    pub units_lost: i32,
    /// the monuments the viewer has built (improvement kinds), in order
    pub monuments: Vec<i16>,
    pub tasks: Vec<TaskView>,
    pub lighthouses: Vec<LighthouseView>,
    /// the viewer first, then the players it has met, by seat
    pub players: Vec<PlayerView>,
    /// every seat of the game with its colour (territory is drawn in it), nature included
    pub colors: Vec<(i16, i64)>,
}

impl View {
    /// One plane, by its index in `TILE_PLANES`.
    pub fn tile_plane(&self, p: usize) -> &[i16] {
        let n = self.width as usize * self.height as usize;
        &self.tiles[p * n..(p + 1) * n]
    }
}

const CITY: i32 = 1;
const RUIN: i32 = 2;
const INVISIBLE: i64 = 3;
const NATURE: i32 = 255;
const CITY_WALL: i32 = 1;
const PARK: i32 = 2;
const SUPER_UNIT: i32 = 6;

/// a list of small ids as a bit set (ids past 14 cannot be shown and refuse)
fn bits(ids: impl Iterator<Item = i64>) -> Result<i16, String> {
    let mut out = 0i16;
    for id in ids {
        if !(0..15).contains(&id) {
            return Err(format!("effect {id} does not fit the view's bit set"));
        }
        out |= 1 << id;
    }
    Ok(out)
}

/// Write `viewer`'s view of `g` into `out` (buffers are reused).
pub fn write(g: &Game, t: &Tables, viewer: i32, out: &mut View) -> Result<(), String> {
    let me = g.player(viewer).ok_or("view for a missing player")?;
    let n = g.tiles.len();
    let width = g.width as usize;
    out.viewer = viewer as i16;
    out.turn = g.current_turn;
    out.width = g.width as i16;
    out.height = (n / width) as i16;
    out.rules = g.fixed.version as i16;
    out.mode =
        i16::try_from(g.fixed.rules_mode).map_err(|_| "rules mode is not a small integer")?;
    out.stars = me.action.currency;
    out.tiles.clear();
    out.tiles.resize(TILE_PLANES.len() * n, UNKNOWN);
    // which resource kinds the viewer's tech shows, asked once
    let revealed = crate::infiltrate::revealed_kinds(g, t, viewer)?;
    let rainbow = t
        .tribes
        .get(me.tribe as usize)
        .is_some_and(|d| d.tribe_abilities.has("rainbowvision"));
    let tile_index = |c: crate::state::Xy| g.index(c).map_or(UNKNOWN, |i| i as i16);
    let mut planes: Vec<&mut [i16]> = out.tiles.chunks_mut(n).collect();
    let [explored, terrain, climate, resource, improvement, level, owner, capital, ruling, road, route, effects, ruin_hint, unit_kind, unit_owner, unit_health, unit_veteran, unit_effects, unit_passenger, unit_moved, unit_attacked, unit_kills, unit_home, city_population, city_connected, city_progress, city_wall, city_parks, city_super_units] =
        planes.as_mut_slice()
    else {
        return Err("view planes".into());
    };
    for (i, x) in g.tiles.iter().enumerate() {
        if !x.explorers.contains(&viewer) {
            explored[i] = 0;
            ruin_hint[i] =
                i16::from(rainbow && x.improvement.as_ref().is_some_and(|m| m.kind == RUIN));
            continue;
        }
        explored[i] = 1;
        ruin_hint[i] = 0;
        terrain[i] = x.terrain as i16;
        climate[i] = x.climate as i16;
        // a resource shows on a tile without an improvement, if the viewer's tech reveals its kind
        resource[i] = match x.resource.filter(|_| x.improvement.is_none()) {
            Some(kind) if (0..32).contains(&kind) && revealed >> kind & 1 == 1 => kind as i16,
            _ => 0,
        };
        owner[i] = x.owner as i16;
        capital[i] = x.capital_of as i16;
        ruling[i] = tile_index(x.ruling_city);
        road[i] = i16::from(x.has_road);
        route[i] = i16::from(x.has_route);
        effects[i] = bits(x.effects.iter().map(|&e| i64::from(e)))?;
        match &x.improvement {
            // a kind the catalog does not know is not shown (as the game's board)
            Some(m) if t.improvements.get(m.kind as usize).is_some() => {
                improvement[i] = m.kind as i16;
                level[i] = if m.kind == CITY { m.level as i16 } else { 0 };
                if m.kind == CITY {
                    city_parks[i] = m.rewards.count(PARK) as i16;
                    city_super_units[i] = m.rewards.count(SUPER_UNIT) as i16;
                }
                if m.kind == CITY && x.owner == viewer {
                    city_population[i] = m.population as i16;
                    city_progress[i] = m.xp as i16;
                    city_connected[i] = m.connected_to_capital_of_player as i16;
                    city_wall[i] = i16::from(m.rewards.contains(&CITY_WALL));
                }
            }
            _ => {
                improvement[i] = 0;
                level[i] = 0;
            }
        }
        unit_kind[i] = 0;
        unit_owner[i] = 0;
        let Some(u) = x.unit.and_then(|node| g.units.get(node as usize)) else {
            continue;
        };
        let fx = crate::menu::unit_effect_list(g, u.effects);
        // an invisible foreign unit shows only to a player at peace with its owner
        if u.owner != viewer
            && fx.contains(&INVISIBLE)
            && crate::menu::relation(g, viewer, u.owner) != PEACE
        {
            continue;
        }
        unit_kind[i] = u.kind as i16;
        unit_owner[i] = u.owner as i16;
        // health as the game displays it
        unit_health[i] = (u.health as u16).div_ceil(10) as i16;
        unit_veteran[i] = i16::from(u.promotion_level != 0);
        unit_kills[i] = u.xp as i16;
        if u.owner == viewer {
            unit_effects[i] = bits(fx.iter().copied())?;
            unit_passenger[i] = u
                .passenger
                .and_then(|p| g.units.get(p as usize))
                .map_or(0, |p| p.kind as i16);
            unit_moved[i] = i16::from(u.moved);
            unit_attacked[i] = i16::from(u.attacked);
            unit_home[i] = tile_index(u.home);
        }
    }
    out.tech.clear();
    out.tech.extend(me.available_tech.iter().map(|&k| k as i16));
    out.tech.sort_unstable();
    out.revealed_resources = revealed;
    out.start_tile = tile_index(me.start_tile);
    out.units_lost = me.casualties;
    out.monuments.clear();
    out.monuments
        .extend(me.built_unique_improvements.iter().map(|&k| k as i16));
    out.tasks.clear();
    for task in &me.action.tasks {
        let counter = match task.kind.name() {
            "Pacifist" => task.peaceful_turns,
            "Killer" => task.kills,
            "Convert" => task.conversions,
            _ => None,
        };
        out.tasks.push(TaskView {
            kind: i16::from(task.kind.0),
            completed: task.flags.completed,
            counter: match counter {
                Some(n) => i32::try_from(n).map_err(|_| "task counter is not a small integer")?,
                None => i32::from(UNKNOWN),
            },
        });
    }
    out.lighthouses.clear();
    let (w, h) = (g.width - 1, n as i32 / g.width - 1);
    for (cx, cy) in [(w, h), (0, 0), (0, h), (w, 0)] {
        let Some(i) = g.index(crate::state::Xy { x: cx, y: cy }) else {
            continue;
        };
        let x = &g.tiles[i];
        let lighthouse = x
            .improvement
            .as_ref()
            .is_some_and(|m| t.improvements.name(m.kind as usize).ok() == Some("lighthouse"));
        if !lighthouse || !x.explorers.contains(&viewer) {
            continue;
        }
        let mut explorers = [0; 5];
        let seats = x.explorers.iter().filter(|&&p| p != NATURE);
        for (slot, &p) in explorers.iter_mut().zip(seats) {
            *slot = p as i16;
        }
        out.lighthouses.push(LighthouseView {
            tile: i as i16,
            explorers,
        });
    }
    out.colors.clear();
    out.colors
        .extend(g.players.iter().map(|p| (p.action.id as i16, p.color)));
    out.players.clear();
    let entry = |p: &crate::state::Player| PlayerView {
        seat: p.action.id as i16,
        tribe: p.tribe as i16,
        relation: me.relation(p.action.id).map_or(0, |r| r.state as i16),
        score: p.score,
    };
    out.players.push(entry(me));
    let mut met: Vec<PlayerView> = g
        .players
        .iter()
        .filter(|p| {
            p.action.id != viewer
                && p.action.id != NATURE
                && me.known_players.contains(&p.action.id)
        })
        .map(entry)
        .collect();
    met.sort_by_key(|p| p.seat);
    out.players.extend(met);
    Ok(())
}
