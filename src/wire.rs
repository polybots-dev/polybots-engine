//! Reads a game as the game itself serializes it: the state a multiplayer game's backend
//! returns, after its leading rules version.
//!
//! The layout, in order: a fixed header, the settings, the map (each tile with its improvement
//! and unit), the players, then the pending choices, the commands played so far, the queued
//! actions and two closing fields. Integers are little-endian; a string is a 7-bit-encoded
//! length and UTF-8 bytes.
//!
//! `decode` reads a state; `encode` writes one legal move of the decoded game as the command
//! the backend takes.
//!
//! What is refused, by name, and not guessed at:
//! - a queued action of a kind not listed in `action` (the kinds found waiting behind a
//!   player's pending choice are read);
//! - a state saved in the middle of a command (queued actions and no pending choice);
//! - a player whose colour is left to the rule data (stored as -1);
//! - an account id that is not 32 hexadecimal digits, with or without dashes;
//! - two players with one id, or a current player that is not in the game.
//!
//! The bytes come from outside, so nothing here indexes or allocates by a number it has not
//! checked against the bytes left. A caller should still bound the input's length.

use serde_json::{json, Value};

use crate::action::Command;
use crate::state::{Game, Xy};
use crate::tables::Tables;

/// A decoded state.
pub struct Decoded {
    pub game: Game,
    /// each command played so far, as its serialized bytes (kind and payload)
    pub commands: Vec<Vec<u8>>,
    /// each player's id and account id, in the state's player order
    pub accounts: Vec<(i32, [u8; 16])>,
}

/// The decisions a decoded game may still take: its `action_limit`.
const ACTION_LIMIT: u64 = 10_000;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or_else(|| format!("state ends inside a field at byte {}", self.at))?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn flag(&mut self) -> Result<bool, String> {
        Ok(self.u8()? != 0)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn xy(&mut self) -> Result<Value, String> {
        Ok(json!({"x": self.i32()?, "y": self.i32()?}))
    }
    /// A string: its length in 7-bit groups, low group first, then that many bytes.
    fn string(&mut self) -> Result<&'a [u8], String> {
        let mut size = 0u32;
        for shift in [0, 7, 14, 21, 28] {
            let byte = self.u8()?;
            if shift == 28 && byte > 7 {
                break;
            }
            size |= u32::from(byte & 127) << shift;
            if byte < 128 {
                return self.take(size as usize);
            }
        }
        Err("string length is malformed".into())
    }
    fn text(&mut self, what: &str) -> Result<&'a str, String> {
        std::str::from_utf8(self.string()?).map_err(|_| format!("{what} is not UTF-8"))
    }
    /// A list whose length is `count`, read item by item.
    fn list<T>(
        &mut self,
        count: usize,
        mut item: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        if count > self.bytes.len() - self.at {
            return Err("list is longer than the state".into());
        }
        (0..count).map(|_| item(self)).collect()
    }
}

/// An account id's 16 bytes, in the order the game holds them (the first three groups
/// little-endian), from its text of 32 hexadecimal digits with or without dashes.
pub fn account(text: &[u8]) -> Result<[u8; 16], String> {
    let digits: Vec<u8> = text.iter().copied().filter(|&b| b != b'-').collect();
    let dashed = text.len() == 36 && [8, 13, 18, 23].iter().all(|&i| text[i] == b'-');
    if digits.len() != 32 || !(text.len() == 32 || dashed) {
        return Err("account id text is not 32 hexadecimal digits".into());
    }
    let mut bytes = [0u8; 16];
    if !digits.iter().all(u8::is_ascii_hexdigit) {
        return Err("account id text is not hexadecimal".into());
    }
    for (i, pair) in digits.chunks(2).enumerate() {
        let text = std::str::from_utf8(pair).map_err(|_| "account id text")?;
        bytes[i] = u8::from_str_radix(text, 16).map_err(|_| "account id text")?;
    }
    bytes[..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    Ok(bytes)
}

/// The same id written the usual way: lowercase, dashed.
fn dashed(mut bytes: [u8; 16]) -> String {
    bytes[..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    let mut out = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if [4, 6, 8, 10].contains(&i) {
            out.push('-');
        }
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn settings(r: &mut Reader) -> Result<Value, String> {
    let rules = json!({
        "turn_limit": r.i32()?,
        "score_limit": r.i32()?,
        "win_by_capital": r.flag()?,
        "allow_mirror_pick": r.flag()?,
        "allow_special_tribes": r.flag()?,
        "win_by_extermination": r.flag()?,
        "allow_tech_sharing": r.flag()?,
        "death_condition": r.u16()?,
    });
    let base_mode = r.u8()?;
    let rules_mode = r.u8()?;
    r.string()?; // the game's name
    let map_size = r.i32()?;
    let tribes = |r: &mut Reader| {
        let count = r.u16()?;
        r.list(count.into(), |r| r.u16())
    };
    let disabled_tribes = tribes(r)?;
    let unlocked_tribes = tribes(r)?;
    let difficulty = r.u16()?;
    let opponent_count = r.i32()?;
    let game_type = r.u16()?;
    let map_preset = r.u8()?;
    r.take(4 + 3 * 4 + 4)?; // the time limit, three timer values and four timer flags
    let count = usize::try_from(r.i32()?).map_err(|_| "negative skin count")?;
    let selected_skins = r.list(count, |r| Ok((r.i16()?, r.i16()?)))?;
    let mut seen = std::collections::HashSet::new();
    if !selected_skins.iter().all(|&(tribe, _)| seen.insert(tribe)) {
        return Err("a tribe's skin is given twice".into());
    }
    let mut settings = json!({
        "base_mode": base_mode, "rules_mode": rules_mode, "game_type": game_type,
        "map_preset": map_preset, "map_size": map_size, "difficulty": difficulty,
        "opponent_count": opponent_count, "disabled_tribes": disabled_tribes,
        "unlocked_tribes": unlocked_tribes, "selected_skins": selected_skins, "rules": rules,
    });
    // the account placed on the west side, kept only when it is an id in a form `account` reads
    if let Ok(west) = account(r.string()?) {
        settings["west_placement_id_text"] = json!(dashed(west).into_bytes());
    }
    Ok(settings)
}

/// The units read so far and their effect lists; a tile or a carrier names a unit by position.
#[derive(Default)]
struct Units {
    units: Vec<Value>,
    effect_lists: Vec<Vec<u16>>,
}

/// One unit, its passenger first. Returns its position in `units`.
fn unit(r: &mut Reader, units: &mut Units, depth: usize) -> Result<usize, String> {
    if depth > 8 || units.units.len() >= 1024 {
        return Err("too many units, or passengers nested too deep".into());
    }
    let id = r.u32()?;
    let owner = r.u8()?;
    let kind = r.u16()?;
    let follower = r.u32()?;
    let leader = r.u32()?;
    let coordinates = r.xy()?;
    let home = r.xy()?;
    let health = r.u16()?;
    let promotion_level = r.u16()?;
    let xp = r.u16()?;
    let moved = r.flag()?;
    let attacked = r.flag()?;
    let flipped = r.flag()?;
    let created_turn = r.u16()?;
    let passenger = if r.flag()? {
        Some(unit(r, units, depth + 1)?)
    } else {
        None
    };
    let count = r.u16()?;
    let effects = r.list(count.into(), |r| r.u16())?;
    let birth_climate = r.i16()?;
    let direction = r.u8()?;
    let birth_climate_skin = r.i16()?;
    units.effect_lists.push(effects);
    units.units.push(json!({
        "id": id, "leader": leader, "follower": follower, "owner": owner,
        "birth_climate": birth_climate, "birth_climate_skin": birth_climate_skin, "kind": kind,
        "previous_turn_end_coordinates": {"x": 0, "y": 0},
        "coordinates": coordinates, "home": home, "passenger": passenger, "health": health,
        "promotion_level": promotion_level, "xp": xp, "moved": moved, "attacked": attacked,
        "direction": direction, "flipped": flipped, "created_turn": created_turn,
        "effects": units.effect_lists.len() - 1, "is_temporary_explorer": false,
    }));
    Ok(units.units.len() - 1)
}

fn improvement(r: &mut Reader) -> Result<Value, String> {
    let kind = r.u16()?;
    let level = r.u16()?;
    let founded = r.u16()?;
    let xp = r.i16()?;
    let population = r.i16()?;
    let production = r.u16()?;
    let base_score = r.u16()?;
    let border_size = r.u16()?;
    let upgrade = r.u16()?;
    let connected = r.u8()?;
    let name = if r.flag()? {
        Some(r.text("a city's name")?)
    } else {
        None
    };
    let founder = r.u8()?;
    let count = r.u16()?;
    let rewards = r.list(count.into(), |r| r.u16())?;
    let count = r.u16()?;
    let effects = r.list(count.into(), |r| r.u16())?;
    // the owner is not stored: a tile's owner is the tile's
    Ok(json!({
        "kind": kind, "owner": 0, "founder": founder, "level": level, "founded": founded,
        "xp": xp, "population": population, "production": production, "base_score": base_score,
        "border_size": border_size, "upgrade": upgrade,
        "connected_to_capital_of_player": connected, "name": name, "rewards": rewards,
        "effects": effects,
    }))
}

fn tile(r: &mut Reader, units: &mut Units) -> Result<Value, String> {
    let coordinates = r.xy()?;
    let terrain = r.u16()?;
    let climate = r.i16()?;
    let altitude = r.i16()?;
    let owner = r.u8()?;
    let capital_of = r.u8()?;
    let ruling_city = r.xy()?;
    let resource = if r.flag()? { Some(r.u16()?) } else { None };
    let improvement = if r.flag()? {
        Some(improvement(r)?)
    } else {
        None
    };
    let unit = if r.flag()? {
        Some(unit(r, units, 0)?)
    } else {
        None
    };
    let count = r.u8()?;
    let explorers = r.list(count.into(), |r| r.u8())?;
    let has_road = r.flag()?;
    let has_route = r.flag()?;
    let skin = r.i32()?;
    let count = r.u8()?;
    let effects = r.list(count.into(), |r| r.i32())?;
    // the shorelines are drawn from the finished board, in `decode`
    let edge = json!({"visible": false, "sprite_extension": null});
    Ok(json!({
        "coordinates": coordinates, "terrain": terrain, "climate": climate, "skin": skin,
        "effects": effects, "altitude": altitude, "owner": owner, "capital_of": capital_of,
        "explorers": explorers,
        "shorelines": {"any": false, "north": edge, "south": edge, "east": edge, "west": edge},
        "ruling_city": ruling_city, "improvement": improvement, "resource": resource,
        "unit": unit, "has_road": has_road, "has_route": has_route, "continent": null,
        "had_route": false, "upgrade_tech": [], "last_population_check": 0,
        "available_population": 0,
    }))
}

/// Task kinds by id, with the counter each carries.
const TASKS: [(&str, Option<&str>); 10] = [
    ("", None),
    ("Pacifist", Some("peaceful_turns")),
    ("Genius", None),
    ("Network", None),
    ("Wealth", None),
    ("Killer", Some("kills")),
    ("Metropolis", None),
    ("Explorer", Some("explored_tiles")),
    ("ExploreLighthouses", None),
    ("Convert", Some("conversions")),
];

fn task(r: &mut Reader) -> Result<Value, String> {
    let kind = usize::from(r.u16()?);
    let &(name, counter) = TASKS
        .get(kind)
        .filter(|_| kind > 0)
        .ok_or_else(|| format!("task kind {kind}"))?;
    let mut task = json!({"type": name, "flags": {"started": r.flag()?, "completed": r.flag()?}});
    if let Some(counter) = counter {
        task[counter] = json!(r.i32()?);
    }
    Ok(task)
}

/// One player and its account id.
fn player(r: &mut Reader) -> Result<(Value, [u8; 16]), String> {
    let id = r.u8()?;
    let user_name = r.text("a player's name")?;
    let account_id = account(r.string()?)?;
    let auto_play = r.flag()?;
    let start_tile = r.xy()?;
    let tribe = r.u16()?;
    let has_chosen_tribe = r.flag()?;
    let handicap = r.i32()?;
    let currency = r.i32()?;
    let score = r.u32()?;
    let end_score = r.u32()?;
    let cities = r.u16()?;
    let count = r.u16()?;
    let available_tech = r.list(count.into(), |r| r.u16())?;
    let count = r.u16()?;
    let known_players = r.list(count.into(), |r| r.u8())?;
    let count = r.u16()?;
    let tasks = r.list(count.into(), task)?;
    let kills = r.u32()?;
    let casualties = r.u32()?;
    let wipe_outs = r.u32()?;
    let color = r.i32()?;
    if color == -1 {
        return Err("a player's colour is left to the rule data".into());
    }
    let tribe_mix = r.u8()?;
    let count = r.u16()?;
    let built_unique_improvements = r.list(count.into(), |r| r.i16())?;
    let count = r.u16()?;
    let relations = r.list(count.into(), |r| {
        Ok((
            r.u8()?,
            json!({
                "state": r.u8()?, "last_attack_turn": r.i32()?, "embassy_level": r.u8()?,
                "last_peace_broken_turn": r.i32()?, "first_meet": r.i32()?,
                "embassy_build_turn": r.i32()?, "previous_attack_turn": r.i32()?,
            }),
        ))
    })?;
    let mut seen = [false; 256];
    let fresh = |&(other, _): &(u8, Value)| !std::mem::replace(&mut seen[usize::from(other)], true);
    if !relations.iter().all(fresh) {
        return Err("a player's relation to another is given twice".into());
    }
    let count = r.u16()?;
    let messages = r.list(count.into(), |r| {
        Ok(json!({"kind": r.u8()?, "sender": r.u8()?}))
    })?;
    let killer_id = r.u8()?;
    let killed_turn = r.u32()?;
    let resigned_at_command = r.i32()?;
    let wiped_at_command = r.i32()?;
    // a skin of 65535 is "none", held as -1
    let skin = match r.u16()? {
        65535 => -1,
        skin => i32::from(skin),
    };
    let resigned_turn = r.i32()?;
    let climate = r.u8()?;
    let player = json!({
        "action": {"id": id, "currency": currency, "tasks": tasks, "opinions_dirty": true},
        "user_name": user_name, "account_id": account_id, "auto_play": auto_play,
        "start_tile": start_tile, "tribe": tribe, "tribe_mix": tribe_mix, "climate": climate,
        "has_chosen_tribe": has_chosen_tribe, "handicap": handicap,
        "resigned_turn": resigned_turn, "resigned_at_command": resigned_at_command,
        "wiped_at_command": wiped_at_command, "available_tech": available_tech,
        "aggressions": [], "known_players": known_players,
        "built_unique_improvements": built_unique_improvements, "relations": relations,
        "messages": messages, "skin": skin, "score": score, "end_score": end_score,
        "cities": cities, "kills": kills, "casualties": casualties, "wipe_outs": wipe_outs,
        "killer_id": killer_id, "killed_turn": killed_turn, "color": color,
        "block_train_units": false, "capital_count_cache": 0, "unlocked_tech_cache": [],
    });
    Ok((player, account_id))
}

/// A command's payload length by its kind, after the kind's two bytes. Every command starts
/// with its player's byte; most then name a tile (8 bytes).
fn command_payload(kind: u16) -> Option<usize> {
    Some(match kind {
        // end turn, start match, end match
        15 | 20 | 22 => 1,
        // research; the answer to a peace request
        8 | 28 => 3,
        // resign
        35 => 4,
        // a tile
        3 | 4 | 9 | 10 | 13 | 14 | 17 | 18 | 21 | 24 | 25 | 26 | 33 | 36 | 37 | 38 => 9,
        // a tile and an opponent
        27 | 29 | 30 | 32 => 10,
        // a tile and what to build, train, upgrade to, take or clear
        1 | 5 | 11 | 16 | 39 => 11,
        // capture
        7 => 13,
        // attack; move
        2 | 6 => 21,
        _ => return None,
    })
}

/// One queued action: its kind, its player, then the kind's own fields.
fn action(r: &mut Reader) -> Result<Value, String> {
    let kind = r.u16()?;
    let player = r.u8()?;
    Ok(match kind {
        25 => json!({"Explore": {"player": player, "coordinates": r.xy()?}}),
        26 => {
            let delay = r.i32()?;
            json!({"City": {"IncreasePopulation": {"player": player, "source": r.xy()?,
                "target": r.xy()?, "delay": delay}}})
        }
        27 => json!({"Payload": {"IncreaseScore": {"player": player, "amount": r.i32()?,
            "delay": r.i32()?, "source": r.xy()?}}}),
        28 => json!({"Payload": {"IncreaseCurrency": {"player": player, "delay": r.i32()?,
            "source": r.xy()?, "amount": r.i32()?}}}),
        40 => json!({"Economy": {"ImprovementLevelUp": {"player": player, "target": r.xy()?}}}),
        44 => json!({"EnableTask": {"player": player, "task": r.i32()?}}),
        45 => json!({"TaskCompleted": {"player": player, "task": r.u16()?}}),
        48 => json!({"MoveEffect": {"Reselect": {"player": player, "unit": r.u32()?}}}),
        62 => json!({"Payload": {"EndCommand": {"player": player, "command": r.u16()?}}}),
        72 => json!({"TurnEffect": {"type": "ReceiveDiplomacyMessage", "player": player,
            "sender": r.u8()?, "kind": r.i16()?}}),
        85 => json!({"Payload": {"RunDeferred": {"player": player}}}),
        _ => return Err(format!("queued action of kind {kind}")),
    })
}

/// The game a serialized state holds. `wire` starts at the state's own version field, after
/// the version the backend puts in front of it.
pub fn decode(wire: &[u8]) -> Result<Decoded, String> {
    let mut r = Reader { bytes: wire, at: 0 };
    let version = r.u32()?;
    if version != 123 && version != 124 {
        return Err(format!("rules version {version}"));
    }
    let current_command = r.u16()?;
    let current_turn = r.u32()?;
    let current_player_index = r.u8()?;
    let current_unit_id = r.u32()?;
    let current_state = r.u8()?;
    let gameplay_seed = r.i32()?;
    let settings = settings(&mut r)?;

    let width = r.u16()?;
    let height = r.u16()?;
    let count = usize::from(width) * usize::from(height);
    if count == 0 || count > 30 * 30 {
        return Err(format!("a board of {width} by {height}"));
    }
    let mut units = Units::default();
    let tiles = r.list(count, |r| tile(r, &mut units))?;

    let count = r.u16()?;
    let (players, accounts): (Vec<Value>, Vec<[u8; 16]>) =
        r.list(count.into(), player)?.into_iter().unzip();

    let count = r.u16()?;
    let triggers = r.list(count.into(), |r| {
        let player = r.u8()?;
        let kind = r.u16()?;
        let coordinates = r.xy()?;
        Ok(json!({"player": player, "opponent": r.u8()?, "kind": kind,
            "coordinates": coordinates}))
    })?;
    let count = r.u16()?;
    let commands = r.list(count.into(), |r| {
        let start = r.at;
        let kind = r.u16()?;
        r.take(command_payload(kind).ok_or_else(|| format!("command kind {kind}"))?)?;
        Ok(r.bytes[start..r.at].to_vec())
    })?;
    let count = r.u16()?;
    let actions = r.list(count.into(), action)?;
    // after a whole command, actions wait only behind a choice some player still has to make
    if !actions.is_empty() && triggers.is_empty() {
        return Err("the state was saved in the middle of a command".into());
    }
    let needs_routes = r.flag()?;
    let village_name_seed = r.i32()?;
    if r.at != wire.len() {
        return Err(format!("{} bytes after the state", wire.len() - r.at));
    }

    // the seats, by player id; Nature (255) is not one
    let ids: Vec<i32> = players
        .iter()
        .map(|p| p["action"]["id"].as_i64().unwrap() as i32)
        .collect();
    // the rules find a player by id and the mover by position: both must name one player
    let mut seen = [false; 256];
    if !(ids.iter()).all(|&id| !std::mem::replace(&mut seen[id as usize], true)) {
        return Err("two players share an id".into());
    }
    if usize::from(current_player_index) >= players.len() {
        return Err("the current player is not in the game".into());
    }
    let mut seats: Vec<(i32, &Value)> = ids
        .iter()
        .zip(&players)
        .filter(|(&id, _)| id != 255)
        .map(|(&id, p)| (id, &p["tribe"]))
        .collect();
    seats.sort_by_key(|&(id, _)| id);
    let tribes: Vec<&Value> = seats.iter().map(|&(_, tribe)| tribe).collect();
    let checkpoint = json!({
        "schema": "engine-fast-v1",
        "config": {"rules": version, "seed": 0, "size": settings["map_size"],
            "map": settings["map_preset"], "mode": settings["rules_mode"], "tribes": tribes,
            "action_limit": ACTION_LIMIT},
        "game": {
            "schema": 2,
            "state": {
                "naming_case": null,
                "naming_random_seeds": [],
                "game_skin": {"skin": 0},
                "achievements": {"active": false, "last_violent_action": [], "kill_streak": []},
                "version": version,
                "seeds": {"gameplay_seed": gameplay_seed, "village_name_seed": village_name_seed,
                    "random_hash": null, "village_name_hash": null},
                "current_turn": current_turn, "current_player_index": current_player_index,
                "current_unit_id": current_unit_id, "current_state": current_state,
                "settings": settings,
                "board": {"width": width, "height": height, "tiles": tiles, "continents": []},
                "unit_graph": {"units": units.units, "effect_lists": units.effect_lists},
                "players": players,
                "current_command": current_command,
                "pending_triggers": {"entries": triggers},
                "needs_routes": needs_routes,
            },
            "commands": commands.len(),
            "actions": {"entries": actions},
        },
        "decisions": 0,
        "sightings": {},
    });
    let mut game = Game::import(&checkpoint)?;
    crate::mapgen::candidate::generate_shorelines(&mut game);
    Ok(Decoded {
        game,
        commands,
        accounts: ids.into_iter().zip(accounts).collect(),
    })
}

/// A legal move of `g` (a candidate's action from `menu::menu`) as a serialized command: its
/// kind, the mover, then the kind's own fields.
pub fn encode(g: &Game, t: &Tables, action: &engine_fast_frame::Action) -> Result<Vec<u8>, String> {
    // admitting a move writes to the state it is checked against, so it is done on a copy
    let mut scratch = g.clone();
    let player = crate::step::mover(g);
    let command = crate::step::admit(&mut scratch, t, action)?;
    let kind = u16::try_from(action.kind).map_err(|_| "command kind")?;
    let mut out = kind.to_le_bytes().to_vec();
    out.push(player as u8);
    let xy = |out: &mut Vec<u8>, c: Xy| {
        out.extend(c.x.to_le_bytes());
        out.extend(c.y.to_le_bytes());
    };
    match command {
        Command::EndTurn | Command::EndMatch | Command::StartMatch => {}
        Command::Research { technology } => out.extend((technology as u16).to_le_bytes()),
        Command::Build { kind, target }
        | Command::Train { kind, target }
        | Command::Upgrade {
            kind,
            coordinates: target,
        }
        | Command::ClearTileEffect {
            effect: kind,
            coordinates: target,
        } => {
            out.extend((kind as u16).to_le_bytes());
            xy(&mut out, target);
        }
        Command::CityReward { reward, target } => {
            xy(&mut out, target);
            out.extend((reward as u16).to_le_bytes());
        }
        Command::Move { unit, from, to } => {
            xy(&mut out, from);
            xy(&mut out, to);
            out.extend(unit.to_le_bytes());
        }
        Command::Attack {
            unit,
            origin,
            target,
        } => {
            out.extend(unit.to_le_bytes());
            xy(&mut out, origin);
            xy(&mut out, target);
        }
        Command::Capture { unit, coordinates } => {
            out.extend(unit.to_le_bytes());
            xy(&mut out, coordinates);
        }
        // a diplomacy command names the opponent's capital
        Command::PeaceTreaty { opponent }
        | Command::BreakPeace { opponent }
        | Command::EstablishEmbassy { opponent } => {
            xy(&mut out, crate::step::current_capital(g, opponent)?);
            out.push(opponent as u8);
        }
        Command::PeaceRequestResponse { opponent, accepted } => {
            out.extend([opponent as u8, u8::from(accepted)]);
        }
        Command::Resign {
            resigned_player,
            kicker_player,
            was_kicked,
        } => out.extend([
            resigned_player as u8,
            kicker_player as u8,
            u8::from(was_kicked),
        ]),
        Command::Destroy { coordinates }
        | Command::Disembark { coordinates }
        | Command::ExamineRuins { coordinates }
        | Command::HealOthers { coordinates }
        | Command::Promote { coordinates }
        | Command::Recover { coordinates }
        | Command::Disband { coordinates }
        | Command::FreezeArea { coordinates }
        | Command::BreakIce { coordinates }
        | Command::Swarm { coordinates }
        | Command::Boost { coordinates }
        | Command::Explode { coordinates }
        | Command::Decompose { coordinates }
        | Command::Flood { coordinates } => xy(&mut out, coordinates),
    }
    Ok(out)
}
