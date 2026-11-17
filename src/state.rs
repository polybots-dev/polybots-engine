//! engine-fast game state and its conversion to and from the interchange checkpoint JSON.
//!
//! The state the rules read is flat: tiles, units and players are `Copy` (short lists are stored
//! in place, `inline.rs`), and the game holds them in arrays beside a few scalars, the action
//! stack and the pending choices. What no rule reads sits behind shared pointers: `Fixed` (the
//! settings and the checkpoint's constant parts), `Side` (names, shorelines, display data) and
//! `Memory` (what each viewer remembers, for input preparation). JSON exists only at the two
//! ends: `import` reads a checkpoint, `export` writes one, and `roundtrip` checks that the pair
//! agrees with the checkpoint under the behaviour contract.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::inline::{Counts, Inline};

/// An `Option` field that must be present (null allowed);
/// serde would otherwise read a missing key as `None`.
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Xy {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Improvement {
    pub kind: i32,
    pub owner: i32,
    pub founder: i32,
    pub level: i32,
    pub founded: i32,
    pub xp: i32,
    pub population: i32,
    pub production: i32,
    pub base_score: i32,
    pub border_size: i32,
    pub upgrade: i32,
    pub connected_to_capital_of_player: i32,
    /// whether the city has a name; the text is in `Side::names`
    #[serde(skip)]
    pub named: bool,
    /// city rewards taken, counted by kind (no rule reads their order)
    pub rewards: Counts<14>,
    pub effects: Inline<i32, 2>,
}

/// One side of a tile's shoreline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shoreline {
    pub visible: bool,
    #[serde(deserialize_with = "required")]
    pub sprite_extension: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shorelines {
    pub any: bool,
    pub north: Shoreline,
    pub south: Shoreline,
    pub east: Shoreline,
    pub west: Shoreline,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tile {
    pub coordinates: Xy,
    pub terrain: i32,
    pub climate: i32,
    pub altitude: i32,
    pub owner: i32,
    pub capital_of: i32,
    /// player ids in the order they explored the tile (a lighthouse shows the order); the
    /// capacity is four seats and Nature
    pub explorers: Inline<i32, 5>,
    pub ruling_city: Xy,
    #[serde(deserialize_with = "required")]
    pub improvement: Option<Improvement>,
    #[serde(deserialize_with = "required")]
    pub resource: Option<i32>,
    /// index into `units` (the unit graph node), not a unit id
    #[serde(deserialize_with = "required")]
    pub unit: Option<u32>,
    pub has_road: bool,
    pub has_route: bool,
    #[serde(deserialize_with = "required")]
    pub continent: Option<i32>,
    pub had_route: bool,
    pub last_population_check: i32,
    pub available_population: i32,
    pub skin: i32,
    pub effects: Inline<i32, 5>,
}

/// What no rule reads, kept beside the flat state for display and for export, one entry per
/// tile. Shared between copies of a game until one of them writes to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Side {
    /// city names
    pub names: Vec<Option<String>>,
    /// drawn at map generation
    pub shorelines: Vec<Option<Shorelines>>,
    pub upgrade_tech: Vec<Value>,
    /// per player, in `players` order
    pub players: Vec<PlayerSide>,
}

/// A player's display data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerSide {
    pub user_name: Option<String>,
    pub account_id: Value,
    pub aggressions: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub id: u32,
    pub leader: u32,
    pub follower: u32,
    pub owner: i32,
    pub kind: i32,
    pub previous_turn_end_coordinates: Xy,
    pub coordinates: Xy,
    pub home: Xy,
    /// index into `units` of the carried unit (a unit-graph node, like `Tile::unit`)
    #[serde(deserialize_with = "required")]
    pub passenger: Option<u32>,
    pub health: i32,
    pub promotion_level: i32,
    pub xp: i32,
    pub moved: bool,
    pub attacked: bool,
    pub direction: i32,
    pub flipped: bool,
    pub created_turn: i32,
    /// index into unit_graph.effect_lists (0 = none)
    pub effects: u32,
    pub is_temporary_explorer: bool,
    pub birth_climate: i32,
    pub birth_climate_skin: i32,
}

/// A task by its index in `step::TASKS`; written by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TaskKind(pub u8);

impl TaskKind {
    pub fn name(self) -> &'static str {
        crate::step::TASKS[self.0 as usize]
    }
    pub fn named(name: &str) -> Option<TaskKind> {
        let n = crate::step::TASKS.iter().position(|t| *t == name)?;
        Some(TaskKind(n as u8))
    }
}

impl Serialize for TaskKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for TaskKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        TaskKind::named(&name).ok_or_else(|| serde::de::Error::custom(format!("task {name}")))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TaskFlags {
    pub started: bool,
    pub completed: bool,
}

/// A player's progress on one task; each kind carries at most one counter.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Task {
    #[serde(rename = "type")]
    pub kind: TaskKind,
    pub flags: TaskFlags,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peaceful_turns: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kills: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversions: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explored_tiles: Option<i64>,
}

/// A diplomacy message waiting for its receiver's turn.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub kind: i64,
    pub sender: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerAction {
    pub id: i32,
    pub currency: i32,
    pub tasks: Inline<Task, 10>,
    /// the game's user-interface flag; no rule reads it
    pub opinions_dirty: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Player {
    pub action: PlayerAction,
    pub tribe: i32,
    pub available_tech: Inline<i32, 56>,
    /// capacity: four seats and Nature, as for the lists below
    pub known_players: Inline<i32, 5>,
    pub score: i64,
    pub cities: i32,
    pub kills: i32,
    pub casualties: i32,
    /// diplomacy toward each other player, in the game's insertion order (never toward player 0)
    pub relations: Inline<(i32, Relation), 5>,
    /// the game's lazily refreshed copy of `available_tech` (null before its first use)
    #[serde(deserialize_with = "required")]
    pub unlocked_tech_cache: Option<Inline<i32, 56>>,
    pub start_tile: Xy,
    pub handicap: i32,
    pub auto_play: bool,
    /// the command count when the player resigned or was wiped out, -1 if not
    pub resigned_at_command: i64,
    pub wiped_at_command: i64,
    /// at most one peace request from each other player
    pub messages: Inline<Message, 5>,
    /// monuments built, in the order built (the order is only displayed)
    pub built_unique_improvements: Inline<i32, 8>,
    pub block_train_units: bool,
    pub skin: i32,
    pub end_score: i64,
    pub color: i64,
    // carried for the checkpoint; no rule reads these
    pub tribe_mix: i64,
    pub climate: i32,
    pub has_chosen_tribe: bool,
    pub resigned_turn: i64,
    pub wipe_outs: i64,
    pub killer_id: i64,
    pub killed_turn: i64,
    pub capital_count_cache: i64,
}

/// Diplomacy relation states.
pub const NEUTRAL: i32 = 0;
pub const PEACE: i32 = 1;
pub const COOL_DOWN: i32 = 3;

/// One player's relation toward another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub state: i32,
    pub last_attack_turn: i32,
    pub embassy_level: i32,
    pub last_peace_broken_turn: i32,
    pub first_meet: i32,
    pub embassy_build_turn: i32,
    pub previous_attack_turn: i32,
}

impl Relation {
    /// the record the game inserts (or returns, toward player 0) for an unmet player
    pub const DEFAULT: Relation = Relation {
        state: NEUTRAL,
        last_attack_turn: -100,
        embassy_level: 0,
        last_peace_broken_turn: -100,
        first_meet: -100,
        embassy_build_turn: -100,
        previous_attack_turn: -100,
    };
}

impl Player {
    /// the player's task of this kind (an index in `step::TASKS`), if it has been enabled
    pub fn task(&self, kind: usize) -> Option<&Task> {
        self.action.tasks.iter().find(|t| t.kind.0 as usize == kind)
    }
    pub fn task_mut(&mut self, kind: usize) -> Option<&mut Task> {
        self.action
            .tasks
            .iter_mut()
            .find(|t| t.kind.0 as usize == kind)
    }
    /// the stored relation toward `other`, without the game's default insertion
    pub fn relation(&self, other: i32) -> Option<&Relation> {
        self.relations
            .iter()
            .find(|(id, _)| *id == other)
            .map(|(_, r)| r)
    }
}

/// A choice a player must make before anything else (a city reward, a peace request).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    pub player: i32,
    pub opponent: i32,
    pub kind: i64,
    pub coordinates: Xy,
}

/// What a viewer remembers beside its sightings (checkpoint `history`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    /// tile indices, ascending
    pub ruins_seen: Vec<i64>,
    #[serde(deserialize_with = "required")]
    pub snapshot_turn: Option<i32>,
    /// per opponent: its score at the first snapshot, at the latest, and at the one before
    pub scores: BTreeMap<i32, (i64, i64, Option<i64>)>,
}

/// What each viewer remembers, for input preparation (`viewer.rs`); no rule reads it. Shared
/// between copies of a game until one of them is refreshed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Memory {
    /// per viewer: the enemy units it last saw
    pub sightings: Option<BTreeMap<i32, Vec<engine_fast_frame::RememberedUnit>>>,
    pub history: Option<BTreeMap<i32, History>>,
}

/// A whole game: typed hot parts plus the rest of the checkpoint with those parts taken out.
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub width: i32,
    /// written through `Game::tile_mut` and the other accessors in board.rs
    pub tiles: crate::board::Tiles,
    pub units: crate::board::Written<Unit>,
    pub players: Vec<Player>,
    pub current_turn: i32,
    pub current_player_index: i32,
    pub current_unit_id: u32,
    /// the match phase
    pub current_state: i64,
    /// how many logged commands have run; equal to `commands.len()` between decisions
    pub current_command: u64,
    /// an exploration since the last route update
    pub needs_routes: bool,
    /// decisions taken so far (the environment's counter)
    pub decisions: u64,
    /// how many commands have been submitted (the game keeps them in a log; the rules
    /// read only its length)
    pub commands: u64,
    /// the stack of queued actions, newest last
    pub actions: Vec<crate::action::Action>,
    /// pending choices, oldest first
    pub triggers: Vec<Trigger>,
    /// unit_graph.effect_lists: a unit's effects by its `effects` index (list 0 is the empty one)
    pub effect_lists: crate::board::Written<Inline<i64, 9>>,
    /// per viewer, the enemy units it last saw, by tile index (absent in a checkpoint without the key)
    pub memory: Arc<Memory>,
    pub seeds: Seeds,
    /// the default-Random seeds names still have to draw, next first
    pub naming_seeds: std::collections::VecDeque<i64>,
    /// what no step changes (shared between clones)
    pub fixed: Arc<Fixed>,
    /// display and export data the rules never read
    pub side: Arc<Side>,
}

/// One of the two hash records; the game creates it from its seed on first use.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HashSeed {
    pub seed: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seeds {
    pub gameplay_seed: i64,
    pub village_name_seed: i64,
    #[serde(deserialize_with = "required")]
    pub random_hash: Option<HashSeed>,
    #[serde(deserialize_with = "required")]
    pub village_name_hash: Option<HashSeed>,
}

/// The settings a match keeps from reset to end, read once at import.
#[derive(Clone, Debug, PartialEq)]
pub struct Fixed {
    pub version: i32,
    pub rules_mode: i64,
    pub game_type: i64,
    pub turn_limit: i64,
    pub score_limit: i64,
    pub win_by_capital: bool,
    pub win_by_extermination: bool,
    pub death_condition: i64,
    pub achievements_active: bool,
    pub naming_case: String,
    /// seats in the match configuration
    pub seats: usize,
    pub action_limit: u64,
    /// the checkpoint with every typed part taken out (the catalog, configuration and
    /// settings as given); written back on export and read by no rule
    rest: Value,
}

const SCALARS: [&str; 3] = ["current_turn", "current_player_index", "current_unit_id"];

fn take(v: &mut Value, path: &[&str]) -> Result<Value, String> {
    let (last, parents) = path.split_last().unwrap();
    let mut at = v;
    for k in parents {
        at = at
            .get_mut(*k)
            .ok_or_else(|| format!("checkpoint has no {}", path.join(".")))?;
    }
    at.as_object_mut()
        .and_then(|m| m.remove(*last))
        .ok_or_else(|| format!("checkpoint has no {}", path.join(".")))
}

fn put(v: &mut Value, path: &[&str], x: Value) {
    let (last, parents) = path.split_last().unwrap();
    let mut at = v;
    for k in parents {
        at = at.get_mut(*k).expect("path taken at import");
    }
    at.as_object_mut()
        .expect("object")
        .insert((*last).to_string(), x);
}

fn typed<T: serde::de::DeserializeOwned>(v: Value, what: &str) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("{what}: {e}"))
}

const MANGROVE: i32 = 8;

impl Game {
    /// Whether the rules record achievement events (kill streaks, the last violent action):
    /// only at rules124 (its end-turn and area-damage achievement calls and the
    /// attack XP write; rules123 has no such calls), and only with tracking on.
    pub(crate) fn records_achievements(&self) -> bool {
        self.fixed.achievements_active && self.fixed.version == 124
    }

    pub fn import(checkpoint: &Value) -> Result<Game, String> {
        let mut rest = checkpoint.clone();
        let mut tiles = take(&mut rest, &["game", "state", "board", "tiles"])?;
        let mut side = Side::default();
        for tile in tiles.as_array_mut().ok_or("tiles are not a list")? {
            side.shorelines
                .push(typed(take(tile, &["shorelines"])?, "shorelines")?);
            side.upgrade_tech.push(take(tile, &["upgrade_tech"])?);
            side.names.push(match tile.get_mut("improvement") {
                Some(m) if !m.is_null() => typed(take(m, &["name"])?, "city name")?,
                _ => None,
            });
        }
        let mut tiles: Vec<Tile> = typed(tiles, "tiles")?;
        for (tile, name) in tiles.iter_mut().zip(&side.names) {
            if let Some(m) = &mut tile.improvement {
                m.named = name.as_ref().is_some_and(|n| !n.is_empty());
            }
        }
        Self::assemble(rest, tiles, side)
    }

    /// A checkpoint whose cleared board is given as tiles rather than as JSON: `rest` is the
    /// checkpoint with an empty tile list. For `reset`, which would otherwise write every
    /// tile as JSON only to read it back.
    pub(crate) fn import_cleared(mut rest: Value, tiles: Vec<Tile>) -> Result<Game, String> {
        take(&mut rest, &["game", "state", "board", "tiles"])?;
        let side = Side {
            names: vec![None; tiles.len()],
            shorelines: vec![Some(crate::mapgen::candidate::cleared_shorelines()); tiles.len()],
            upgrade_tech: vec![Value::Array(Vec::new()); tiles.len()],
            ..Side::default()
        };
        Self::assemble(rest, tiles, side)
    }

    /// The game from a checkpoint whose tiles have been taken out and typed.
    fn assemble(mut rest: Value, tiles: Vec<Tile>, mut side: Side) -> Result<Game, String> {
        // no rule makes mangrove and the move search does not model it, so a board with it is
        // refused here, once, rather than at every search
        if tiles.iter().any(|x| x.terrain == MANGROVE) {
            return Err("mangrove terrain not implemented".into());
        }
        let units: Vec<Unit> = typed(
            take(&mut rest, &["game", "state", "unit_graph", "units"])?,
            "units",
        )?;
        let effect_lists: Vec<Inline<i64, 9>> = typed(
            take(&mut rest, &["game", "state", "unit_graph", "effect_lists"])?,
            "effect lists",
        )?;
        let memory = |rest: &mut Value, key: &str| rest.as_object_mut().and_then(|m| m.remove(key));
        let sightings = memory(&mut rest, "sightings")
            .map(|v| typed(v, "sightings"))
            .transpose()?;
        let history = memory(&mut rest, "history")
            .map(|v| typed(v, "history"))
            .transpose()?;
        let mut players = take(&mut rest, &["game", "state", "players"])?;
        for p in players.as_array_mut().ok_or("players are not a list")? {
            side.players.push(PlayerSide {
                user_name: typed(take(p, &["user_name"])?, "user name")?,
                account_id: take(p, &["account_id"])?,
                aggressions: take(p, &["aggressions"])?,
            });
        }
        let players: Vec<Player> = typed(players, "players")?;
        let scalar = |rest: &mut Value, k: &str| -> Result<i64, String> {
            take(rest, &["game", "state", k])?
                .as_i64()
                .ok_or_else(|| format!("{k} is not an integer"))
        };
        let current_turn = scalar(&mut rest, SCALARS[0])? as i32;
        let current_player_index = scalar(&mut rest, SCALARS[1])? as i32;
        let current_unit_id = scalar(&mut rest, SCALARS[2])? as u32;
        let current_state = scalar(&mut rest, "current_state")?;
        let current_command = scalar(&mut rest, "current_command")? as u64;
        let needs_routes = take(&mut rest, &["game", "state", "needs_routes"])?
            .as_bool()
            .ok_or("needs_routes is not a flag")?;
        let decisions = take(&mut rest, &["decisions"])?
            .as_u64()
            .ok_or("decisions is not a count")?;
        // a full checkpoint carries the log; ours carries its length
        let commands = match take(&mut rest, &["game", "commands"])? {
            Value::Array(log) => log.len() as u64,
            count => count
                .as_u64()
                .ok_or("commands is neither a log nor a count")?,
        };
        let seeds: Seeds = typed(take(&mut rest, &["game", "state", "seeds"])?, "seeds")?;
        let naming_seeds = typed(
            take(&mut rest, &["game", "state", "naming_random_seeds"])?,
            "naming seeds",
        )?;
        let fixed = {
            let st = &rest["game"]["state"];
            let rules = &st["settings"]["rules"];
            let int = |v: &Value, what: &str| {
                v.as_i64()
                    .ok_or_else(|| format!("checkpoint has no {what}"))
            };
            let flag = |v: &Value, what: &str| {
                v.as_bool()
                    .ok_or_else(|| format!("checkpoint has no {what}"))
            };
            Fixed {
                version: int(&st["version"], "version")? as i32,
                rules_mode: int(&st["settings"]["rules_mode"], "rules mode")?,
                game_type: int(&st["settings"]["game_type"], "game type")?,
                turn_limit: int(&rules["turn_limit"], "turn limit")?,
                score_limit: int(&rules["score_limit"], "score limit")?,
                win_by_capital: flag(&rules["win_by_capital"], "win_by_capital")?,
                win_by_extermination: flag(&rules["win_by_extermination"], "win_by_extermination")?,
                death_condition: int(&rules["death_condition"], "death condition")?,
                achievements_active: flag(&st["achievements"]["active"], "achievements flag")?,
                naming_case: st["naming_case"].as_str().unwrap_or("").to_owned(),
                seats: rest["config"]["tribes"].as_array().map_or(0, Vec::len),
                action_limit: rest["config"]["action_limit"]
                    .as_u64()
                    .ok_or("checkpoint has no action limit")?,
                rest: Value::Null,
            }
        };
        let triggers: Vec<Trigger> = typed(
            take(&mut rest, &["game", "state", "pending_triggers", "entries"])?,
            "pending triggers",
        )?;
        let actions: Vec<crate::action::Action> = typed(
            take(&mut rest, &["game", "actions", "entries"])?,
            "queued actions",
        )?;
        let width = rest["game"]["state"]["board"]["width"]
            .as_i64()
            .ok_or("board has no width")? as i32;
        if tiles.len() as i32
            != width
                * rest["game"]["state"]["board"]["height"]
                    .as_i64()
                    .unwrap_or(-1) as i32
        {
            return Err("tile count disagrees with board size".into());
        }
        for (i, t) in tiles.iter().enumerate() {
            if t.coordinates
                != (Xy {
                    x: i as i32 % width,
                    y: i as i32 / width,
                })
            {
                return Err(format!("tile {i} is not in row-major order"));
            }
        }
        Ok(Game {
            width,
            tiles: tiles.into(),
            units: units.into(),
            players,
            current_turn,
            current_player_index,
            current_unit_id,
            current_state,
            current_command,
            needs_routes,
            decisions,
            commands,
            actions,
            triggers,
            effect_lists: effect_lists.into(),
            memory: Arc::new(Memory { sightings, history }),
            seeds,
            naming_seeds,
            fixed: Arc::new(Fixed { rest, ..fixed }),
            side: Arc::new(side),
        })
    }

    /// The game's skin (state.game_skin), carried untyped: no rule writes it.
    pub fn game_skin(&self) -> i32 {
        self.fixed.rest["game"]["state"]["game_skin"]["skin"]
            .as_i64()
            .unwrap_or(0) as i32
    }

    /// The continents map generation recorded (board.continents, carried untyped).
    pub fn continents(&self) -> &Value {
        &self.fixed.rest["game"]["state"]["board"]["continents"]
    }

    /// Map generation records the continents it drew (nothing reads them afterwards).
    pub fn set_continents(&mut self, continents: Value) {
        Arc::make_mut(&mut self.fixed).rest["game"]["state"]["board"]["continents"] = continents;
    }

    /// Give the city on tile `i` its name.
    pub(crate) fn name_city(&mut self, i: usize, name: String) {
        if let Some(m) = &mut self.tile_mut(i).improvement {
            m.named = !name.is_empty();
        }
        Arc::make_mut(&mut self.side).names[i] = Some(name);
    }

    /// Every player's id, in seat order.
    pub fn player_ids(&self) -> crate::inline::Seats<i32> {
        self.players.iter().map(|p| p.action.id).collect()
    }

    /// Index of the player with id `id` in `players`.
    pub fn player(&self, id: i32) -> Option<&Player> {
        self.players.iter().find(|p| p.action.id == id)
    }

    /// The index in `tiles` of the on-board coordinate `c`.
    pub fn index(&self, c: Xy) -> Option<usize> {
        // on a rectangular board a row past the last one is an index past the end
        let i = (c.y * self.width + c.x) as usize;
        (c.x >= 0 && c.y >= 0 && c.x < self.width && i < self.tiles.len()).then_some(i)
    }

    pub fn tile(&self, at: Xy) -> Option<&Tile> {
        (0..self.width)
            .contains(&at.x)
            .then(|| self.tiles.get((at.y * self.width + at.x) as usize))
            .flatten()
    }

    pub fn export(&self) -> Value {
        let mut v = self.fixed.rest.clone();
        let j = |x: &dyn erased::Ser| x.value();
        let mut tiles = j(&self.tiles);
        for (i, tile) in tiles
            .as_array_mut()
            .expect("tile list")
            .iter_mut()
            .enumerate()
        {
            let tile = tile.as_object_mut().expect("tile");
            tile.insert("shorelines".into(), j(&self.side.shorelines[i]));
            tile.insert("upgrade_tech".into(), self.side.upgrade_tech[i].clone());
            if let Some(m) = tile.get_mut("improvement").and_then(Value::as_object_mut) {
                m.insert("name".into(), j(&self.side.names[i]));
            }
        }
        put(&mut v, &["game", "state", "board", "tiles"], tiles);
        put(
            &mut v,
            &["game", "state", "unit_graph", "units"],
            j(&self.units),
        );
        put(
            &mut v,
            &["game", "state", "unit_graph", "effect_lists"],
            j(&self.effect_lists),
        );
        if let Some(x) = &self.memory.sightings {
            put(&mut v, &["sightings"], j(x));
        }
        if let Some(x) = &self.memory.history {
            put(&mut v, &["history"], j(x));
        }
        let mut players = j(&self.players);
        for (p, side) in players
            .as_array_mut()
            .expect("player list")
            .iter_mut()
            .zip(&self.side.players)
        {
            let p = p.as_object_mut().expect("player");
            p.insert("user_name".into(), j(&side.user_name));
            p.insert("account_id".into(), side.account_id.clone());
            p.insert("aggressions".into(), side.aggressions.clone());
        }
        put(&mut v, &["game", "state", "players"], players);
        put(&mut v, &["game", "state", "seeds"], j(&self.seeds));
        put(
            &mut v,
            &["game", "state", "naming_random_seeds"],
            j(&self.naming_seeds),
        );
        put(
            &mut v,
            &["game", "state", "current_state"],
            Value::from(self.current_state),
        );
        put(
            &mut v,
            &["game", "state", "current_command"],
            Value::from(self.current_command),
        );
        put(
            &mut v,
            &["game", "state", "needs_routes"],
            Value::from(self.needs_routes),
        );
        put(&mut v, &["decisions"], Value::from(self.decisions));
        put(&mut v, &["game", "commands"], Value::from(self.commands));
        put(&mut v, &["game", "actions", "entries"], j(&self.actions));
        put(
            &mut v,
            &["game", "state", "pending_triggers", "entries"],
            j(&self.triggers),
        );
        put(
            &mut v,
            &["game", "state", SCALARS[0]],
            Value::from(self.current_turn),
        );
        put(
            &mut v,
            &["game", "state", SCALARS[1]],
            Value::from(self.current_player_index),
        );
        put(
            &mut v,
            &["game", "state", SCALARS[2]],
            Value::from(self.current_unit_id),
        );
        v
    }
}

mod erased {
    pub trait Ser {
        fn value(&self) -> serde_json::Value;
    }
    impl<T: serde::Serialize> Ser for T {
        fn value(&self) -> serde_json::Value {
            serde_json::to_value(self).expect("serializable state")
        }
    }
}

/// Ok when export(import(checkpoint)) agrees with the checkpoint under the behaviour contract
/// (`contract::projection`); Err names the first differing path.
pub fn roundtrip(checkpoint: &Value) -> Result<(), String> {
    let back = Game::import(checkpoint)?.export();
    match crate::contract::difference(checkpoint, &back)? {
        None => Ok(()),
        Some(path) => Err(path),
    }
}

/// Tiles, units and players own no heap data: copying a game copies flat arrays. The compiler
/// checks it, since a type with a `Vec`, `String` or `Box` inside cannot be `Copy`.
const _: () = {
    const fn flat<T: Copy>() {}
    flat::<Tile>();
    flat::<Unit>();
    flat::<Player>();
    flat::<Trigger>();
};
