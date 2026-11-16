//! A new game built from scratch: the roster
//! with its colours and names, the chosen map, the starting units, the StartMatch command and
//! the first viewer memory. Scope: rules 123 and 124, pass-and-play, 2 to 4 seats of given
//! tribes.
use crate::action::Action;
use crate::mapgen::{select, MapGeneratorSettings};
use crate::random::SystemRandom;
use crate::state::Game;
use crate::step::{add_score, player_index, task_notification};
use crate::tables::Tables;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub rules: i32,
    pub seed: i32,
    /// board side
    pub size: i32,
    /// the map type id
    pub map: i32,
    /// the game mode id
    pub mode: i32,
    /// one tribe id per seat
    pub tribes: Vec<i32>,
    pub action_limit: u32,
}

/// Nano: this engine's own board side below the game's smallest.
/// NANO_MAP_WIDTH). The game generator runs unchanged at this side.
pub const NANO: i32 = 8;

/// The board sides a game may have: the game's 11 to 30, and nano.
pub fn side_admitted(size: i32) -> bool {
    (11..=30).contains(&size) || size == NANO
}

const NATURE: i32 = 255;
// game mode ids
const PERFECTION: i32 = 1;
const DOMINATION: i32 = 2;
const GLORY: i32 = 3;
const MIGHT: i32 = 4;
const GENIUS_TASK: usize = 2;

/// A new player state with the identity, tribe and handicap
fn player(id: i32, tribe: i32, handicap: i32) -> Value {
    json!({
        "action": {"id": id, "currency": 0, "tasks": [], "opinions_dirty": true},
        "user_name": null, "account_id": vec![0; 16], "auto_play": true,
        "start_tile": {"x": -1, "y": -1}, "tribe": tribe, "tribe_mix": 0, "climate": tribe,
        "has_chosen_tribe": tribe != 0, "handicap": handicap, "resigned_turn": -1,
        "resigned_at_command": -1, "wiped_at_command": -1, "available_tech": [],
        "aggressions": [], "known_players": [], "built_unique_improvements": [],
        "relations": [], "messages": [], "skin": 0, "score": 0, "end_score": 0, "cities": 0,
        "kills": 0, "casualties": 0, "wipe_outs": 0, "killer_id": 0, "killed_turn": 0,
        "color": -1, "block_train_units": false, "capital_count_cache": 0,
        "unlocked_tech_cache": [],
    })
}

/// Squared distance between two colours on normalized RGB
fn color_distance_squared(first: i32, second: i32) -> f64 {
    fn normalized(color: i32) -> [f64; 3] {
        let [blue, green, red, _] = color.to_le_bytes();
        let rgb = [f64::from(red), f64::from(green), f64::from(blue)];
        let norm = rgb.iter().map(|c| c * c).sum::<f64>().sqrt();
        rgb.map(|c| c / norm)
    }
    let (a, b) = (normalized(first), normalized(second));
    let square = |i: usize| (a[i] - b[i]).powi(2);
    (square(0) + square(1)) + square(2)
}

/// A new colour: the available colour farthest from every picked one
fn new_color(available: &[i32], picked: &[i32], random: &mut SystemRandom) -> i32 {
    if available.is_empty() {
        return (random.next_float() * 16_777_215.0_f32) as i32;
    }
    let (mut best, mut best_distance) = (-1, f64::MIN);
    for &candidate in available {
        let mut closest = f64::MAX;
        for &other in picked {
            let distance = color_distance_squared(candidate, other);
            if distance.is_nan() {
                closest = distance;
                break;
            }
            closest = closest.min(distance);
        }
        if closest > best_distance {
            best_distance = closest;
            best = candidate;
        }
    }
    best
}

/// the next default-Random seed of the game's naming queue
fn default_random(g: &mut Game) -> Result<(), String> {
    g.naming_seeds
        .pop_front()
        .map(|_| ())
        .ok_or_else(|| "no default-Random seed left".into())
}

/// Player colours and names: tribe colours, duplicates moved to the
/// farthest free tribe colour, then a name for every unnamed player
fn colors_and_names(g: &mut Game, t: &Tables, gameplay_seed: i32) -> Result<(), String> {
    let color_of = |tribe: i32| {
        t.tribes
            .get(tribe as usize)
            .map_or(0xffffff, |d| d.color as i32)
    };
    let (mut picked, mut duplicates) = (Vec::new(), Vec::new());
    for i in 0..g.players.len() {
        let nature = g.players[i].action.id == NATURE;
        if nature && g.fixed.version == 123 {
            continue;
        }
        let color = color_of(g.players[i].tribe);
        g.players[i].color = i64::from(color);
        if nature {
            continue;
        }
        if picked.contains(&color) {
            duplicates.push(i);
        } else {
            picked.push(color);
        }
    }
    if !duplicates.is_empty() {
        let mut random = SystemRandom::new(gameplay_seed);
        let mut available = Vec::new();
        // the catalog's own tribe order, playable tribes only
        for &color in &t.tribe_colors {
            if !picked.contains(&color) {
                available.push(color);
            }
        }
        for i in duplicates {
            let color = new_color(&available, &picked, &mut random);
            g.players[i].color = i64::from(color);
            if let Some(k) = available.iter().position(|&c| c == color) {
                available.remove(k);
            }
            picked.push(color);
        }
        // the game replaces its static Random with a default one here
        default_random(g)?;
    }
    for i in 0..g.players.len() {
        if g.side.players[i]
            .user_name
            .as_deref()
            .unwrap_or("")
            .is_empty()
        {
            let (tribe, skin) = (g.players[i].tribe, i64::from(g.players[i].skin));
            let color = g.players[i].color as i32;
            let name = crate::capture::make_word(g, t, tribe, skin, color.wrapping_add(1))?;
            std::sync::Arc::make_mut(&mut g.side).players[i].user_name = Some(name);
        }
    }
    Ok(())
}

/// The starting roster for a locally created game: starting stars and one starting
/// unit on each capital (which queues nothing in scope)
fn starting_roster(g: &mut Game, t: &Tables) -> Result<(), String> {
    for i in 0..g.players.len() {
        let (id, tribe) = (g.players[i].action.id, g.players[i].tribe);
        if id == NATURE {
            continue;
        }
        let data = t.tribes.get(tribe as usize).ok_or("local player tribe")?;
        // the game's TribeData default where the catalog names none
        g.players[i].action.currency = data.starting_stars.unwrap_or(5).max(0);
        if tribe == 0 {
            continue;
        }
        let Ok(kind) = t.units.id(&data.starting_unit) else {
            continue;
        };
        if kind == 0 {
            continue;
        }
        let start = g.players[i].start_tile;
        let mut queued = Vec::new();
        let node = crate::units::create_unit(g, t, id, kind as i32, start, &mut queued)?;
        if !queued.is_empty() {
            return Err("a starting unit that queues actions".into());
        }
        let score = crate::units::unit_score(g, t, &g.units[node as usize]);
        let mut over = Vec::new();
        add_score(g, id, i64::from(score), &mut over)?;
        if !over.is_empty() {
            return Err("a score-limit win at game start not implemented".into());
        }
        g.units[node as usize].moved = false;
        g.units[node as usize].attacked = false;
    }
    Ok(())
}

/// Learn the starting technology: the tech is known at once, with its score and tasks
fn learn(
    g: &mut Game,
    t: &Tables,
    i: usize,
    tech: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    if g.players[i].available_tech.contains(&tech) {
        return Ok(());
    }
    let id = g.players[i].action.id;
    let (mut score, mut tasks, mut vision) = (0, Vec::new(), false);
    if let Some(data) = t.techs.get(tech as usize) {
        let value = data.cost.wrapping_mul(100) as f32;
        score = if value >= 2147483648.0_f32 || value < -2147483648.0_f32 {
            i32::MIN
        } else {
            value as i32
        };
        for name in &data.task_unlocks {
            tasks.push(t.tasks.id(name)?);
        }
        vision = data.ability_unlocks.has("capitalvision");
    }
    if vision {
        return Err("a starting technology with capital vision not implemented".into());
    }
    g.players[i].available_tech.push(tech);
    add_score(g, id, i64::from(score), out)?;
    for task in tasks {
        if g.players[i].task(task).is_none() {
            out.push(Action::EnableTask {
                player: id,
                task: task as u32,
            });
        }
    }
    out.extend(task_notification(g, t, i, GENIUS_TASK)?);
    Ok(())
}

/// The StartMatch action: every player learns its tribe's starting
/// technologies, a player already seeing a map corner gets the lighthouse hook, and the match
/// is Started.
pub(crate) fn start_match(g: &mut Game, t: &Tables, out: &mut Vec<Action>) -> Result<(), String> {
    for i in 0..g.players.len() {
        let (id, tribe) = (g.players[i].action.id, g.players[i].tribe);
        let data = if tribe == 0 {
            None
        } else {
            t.tribes.get(tribe as usize)
        };
        let mut reveal_hook = true;
        if let Some(data) = data {
            let mut techs = Vec::new();
            for name in &data.starting_tech {
                techs.push(t.techs.id(name)? as i32);
            }
            reveal_hook = !techs.is_empty();
            for tech in techs {
                learn(g, t, i, tech, out)?;
            }
        }
        if reveal_hook && id != NATURE {
            let (w, h) = (g.width - 1, g.tiles.len() as i32 / g.width - 1);
            for (x, y) in [(w, h), (0, 0), (0, h), (w, 0)] {
                let c = crate::state::Xy { x, y };
                let corner = g.index(c).ok_or("lighthouse corner")?;
                if crate::explore::explored(g, corner, id) {
                    crate::explore::reveal_lighthouse(g, t, id, c, out)?;
                    break;
                }
            }
        }
    }
    g.current_state = crate::menu::STARTED;
    Ok(())
}

/// A new game: the state the game environment has after `reset` with candidate details
/// on (the first viewer memory included). `catalog_json` is the catalog text the game carries
/// and `t` its compiled tables.
pub fn reset(config: &Config, catalog_json: &str, t: &Tables) -> Result<Game, String> {
    if !t.compiled_from(catalog_json) {
        return Err("the tables were not compiled from the catalogue text given".into());
    }
    if ![123, 124].contains(&config.rules) {
        return Err("reset for rules other than 123 and 124 not implemented".into());
    }
    if !(2..=4).contains(&config.tribes.len())
        || !side_admitted(config.size)
        || !(1..=10_000).contains(&config.action_limit)
        || config.tribes.iter().any(|t| !(2..=17).contains(t))
        || !(1..=6).contains(&config.map)
    {
        return Err("configuration outside the environment's scope".into());
    }
    if ![GLORY, PERFECTION, DOMINATION, MIGHT].contains(&config.mode) {
        return Err("game mode outside scope".into());
    }
    let mut map_random = SystemRandom::new(config.seed);
    let mut names = SystemRandom::new(31);
    let naming_seeds: Vec<i32> = (0..1024).map(|_| names.next()).collect();
    let (gameplay_seed, village_seed) = (map_random.next(), map_random.next());
    let mut players: Vec<Value> = Vec::new();
    for (index, &tribe) in config.tribes.iter().enumerate() {
        let id = index as i32 + 1;
        let mut p = player(id, tribe, 1);
        p["auto_play"] = json!(false);
        p["account_id"] = json!(vec![id; 16]);
        players.push(p);
    }
    // Nature's handicap: rules 124 gives it the settings difficulty's (and
    // the environment has no difficulty input, so the settings default, Easy: 0); rules 123
    // keeps the player default, 1. At 123 it also joins after the colours
    // so it keeps colour -1 (`colors_and_names`).
    let mut nature = player(NATURE, 1, if config.rules == 123 { 1 } else { 0 });
    nature["climate"] = json!(0);
    nature["user_name"] = json!("Nature");
    players.push(nature);
    let width = config.size;
    let checkpoint = json!({
        "schema": "engine-fast-v1",
        "config": {"rules": config.rules, "seed": config.seed, "size": config.size, "map": config.map,
            "mode": config.mode, "tribes": config.tribes, "action_limit": config.action_limit},
        "game": {
            "schema": 2,
            "state": {
                "naming_case": "Common",
                "naming_random_seeds": naming_seeds,
                "game_skin": {"skin": 0},
                "achievements": {"active": false, "last_violent_action": [], "kill_streak": []},
                "version": config.rules,
                "seeds": {"gameplay_seed": gameplay_seed, "village_name_seed": village_seed,
                    "random_hash": null, "village_name_hash": null},
                "current_turn": 0, "current_player_index": 0, "current_unit_id": 0,
                "current_state": 0,
                "settings": {
                    "base_mode": config.mode, "rules_mode": config.mode, "game_type": 2,
                    "map_preset": config.map, "map_size": config.size, "difficulty": 0,
                    "opponent_count": config.tribes.len() - 1, "disabled_tribes": [],
                    "unlocked_tribes": [], "selected_skins": [],
                    "rules": {
                        "turn_limit": if config.mode == PERFECTION { 30 } else { 0 },
                        "score_limit": if config.mode == GLORY { 10000 } else { 0 },
                        "win_by_capital": config.mode == MIGHT,
                        "win_by_extermination": config.mode == MIGHT || config.mode == DOMINATION,
                        "allow_mirror_pick": false, "allow_special_tribes": true,
                        "allow_tech_sharing": true, "death_condition": 0,
                    },
                },
                // the cleared tiles are given typed to `import_cleared`
                "board": {"width": width, "height": width, "tiles": [], "continents": []},
                "unit_graph": {"units": [], "effect_lists": []},
                "players": players,
                "current_command": 0,
                "pending_triggers": {"entries": []},
                "needs_routes": false,
            },
            "commands": [],
            "actions": {"entries": []},
            "catalog_json": catalog_json,
        },
        "decisions": 0,
        "sightings": {},
        "history": {},
    });
    let cleared = (0..width)
        .flat_map(|y| (0..width).map(move |x| crate::state::Xy { x, y }))
        .map(crate::mapgen::candidate::cleared_tile)
        .collect();
    let mut g = Game::import_cleared(checkpoint, cleared)?;
    colors_and_names(&mut g, t, gameplay_seed)?;
    // the map generator's constructor makes a default Random it never reads
    default_random(&mut g)?;
    let settings = MapGeneratorSettings::from_preset(config.map);
    // no west-placement request: the environment's settings never name a preferred player
    select::select(&mut g, t, &settings, None, || map_random.next())?;
    starting_roster(&mut g, t)?;
    crate::step::submit(&mut g, &t, 1, crate::action::Command::StartMatch)?;
    if player_index(&g, NATURE)? == g.current_player_index as usize {
        return Err("a game starting on Nature's turn not implemented".into());
    }
    crate::viewer::refresh(&mut g)?;
    Ok(g)
}
