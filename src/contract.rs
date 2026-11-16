//! Canonical digests and comparisons of game states.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Hex SHA-256 of the canonical JSON of `v`: object keys sorted, no whitespace. Key order in the
/// producer therefore never matters, only content.
pub fn digest(v: &Value) -> String {
    let mut out = Vec::new();
    canonical(v, &mut out);
    let h = Sha256::digest(&out);
    h.iter().map(|b| format!("{b:02x}")).collect()
}

fn canonical(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend(serde_json::to_vec(k).unwrap());
                out.push(b':');
                canonical(&m[*k], out);
            }
            out.push(b'}');
        }
        Value::Array(a) => {
            out.push(b'[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                canonical(x, out);
            }
            out.push(b']');
        }
        other => out.extend(serde_json::to_vec(other).unwrap()),
    }
}

/// The first JSON path at which `a` and `b` differ, for divergence reports.
pub fn first_difference(a: &Value, b: &Value, path: &mut String) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let len = path.len();
                path.push('.');
                path.push_str(k);
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => {
                        if let Some(d) = first_difference(p, q, path) {
                            return Some(d);
                        }
                    }
                    _ => return Some(path.clone()),
                }
                path.truncate(len);
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                let len = path.len();
                path.push_str(&format!("[{i}]"));
                if let Some(d) = first_difference(p, q, path) {
                    return Some(d);
                }
                path.truncate(len);
            }
            (x.len() != y.len()).then(|| format!("{path} (length {} vs {})", x.len(), y.len()))
        }
        _ => (a != b).then(|| path.clone()),
    }
}

/// The behaviour contract's comparison: the part of a
/// checkpoint that can change a later legal move, a later outcome or what a player sees.
/// Two engines agree on a state when their projections are equal. What is left out, and
/// why no rule reads it, is documented with the state fields;
/// viewer memory (`sightings`, `history`) is left out because it belongs to input
/// preparation. Lists whose order no rule reads are sorted; a unit is written where it
/// stands, without its id or node index.
pub fn projection(checkpoint: &Value) -> Result<Value, String> {
    use serde_json::json;
    let state = &checkpoint["game"]["state"];
    let list = |v: &Value, what: &str| -> Result<Vec<Value>, String> {
        v.as_array()
            .cloned()
            .ok_or_else(|| format!("checkpoint has no {what}"))
    };
    // a list whose order no rule reads; anything but a list is a malformed checkpoint
    let sorted = |v: &Value| -> Result<Value, String> {
        let mut a = v
            .as_array()
            .cloned()
            .ok_or("a list is missing from the checkpoint")?;
        a.sort_by_key(|x| x.to_string());
        Ok(Value::Array(a))
    };
    let pick = |v: &Value, keys: &[&str]| -> Value {
        Value::Object(
            keys.iter()
                .map(|k| ((*k).to_owned(), v[*k].clone()))
                .collect(),
        )
    };
    let tiles = list(&state["board"]["tiles"], "tiles")?;
    let units = list(&state["unit_graph"]["units"], "units")?;
    let effect_lists = list(&state["unit_graph"]["effect_lists"], "effect lists")?;
    let width = state["board"]["width"].as_i64().ok_or("board width")?;
    let height = state["board"]["height"].as_i64().ok_or("board height")?;
    // which on-board units share each effect list (the head unit and its passenger chain)
    let mut sharers: std::collections::BTreeMap<u64, Vec<String>> = Default::default();
    let mut chain = |start: &Value, at: &Value| -> Result<(), String> {
        let (mut node, mut depth) = (start.as_u64(), 0);
        while let Some(n) = node {
            let u = units.get(n as usize).ok_or("unit node out of range")?;
            let list = u["effects"].as_u64().ok_or("unit effects")?;
            sharers
                .entry(list)
                .or_default()
                .push(format!("{at}/{depth}"));
            node = u["passenger"].as_u64();
            depth += 1;
            if depth > 8 {
                return Err("passenger chain too long".into());
            }
        }
        Ok(())
    };
    for tile in &tiles {
        chain(&tile["unit"], &tile["coordinates"])?;
    }
    fn unit(
        node: &Value,
        units: &[Value],
        effect_lists: &[Value],
        sharers: &std::collections::BTreeMap<u64, Vec<String>>,
        depth: usize,
    ) -> Result<Value, String> {
        let Some(n) = node.as_u64() else {
            return Ok(Value::Null);
        };
        if depth > 8 {
            return Err("passenger chain too long".into());
        }
        let u = units.get(n as usize).ok_or("unit node out of range")?;
        let list = u["effects"].as_u64().ok_or("unit effects")?;
        let mut effects = effect_lists
            .get(list as usize)
            .and_then(Value::as_array)
            .cloned()
            .ok_or("effect list out of range")?;
        effects.sort_by_key(|x| x.to_string());
        let shared = sharers.get(&list).filter(|s| s.len() > 1);
        let mut out = serde_json::Map::new();
        for k in [
            "owner",
            "kind",
            "coordinates",
            "home",
            "health",
            "promotion_level",
            "xp",
            "moved",
            "attacked",
            "direction",
            "is_temporary_explorer",
            "birth_climate_skin",
        ] {
            out.insert(k.into(), u[k].clone());
        }
        // rules read only whether a unit is linked
        out.insert("has_leader".into(), Value::Bool(u["leader"] != 0));
        out.insert("has_follower".into(), Value::Bool(u["follower"] != 0));
        out.insert("effects".into(), Value::Array(effects));
        out.insert(
            "effects_shared_by".into(),
            serde_json::to_value(shared).map_err(|e| e.to_string())?,
        );
        out.insert(
            "passenger".into(),
            unit(&u["passenger"], units, effect_lists, sharers, depth + 1)?,
        );
        Ok(Value::Object(out))
    }
    let mut out_tiles = Vec::with_capacity(tiles.len());
    for tile in &tiles {
        let c = &tile["coordinates"];
        let corner = [0, width - 1].contains(&c["x"].as_i64().unwrap_or(-1))
            && [0, height - 1].contains(&c["y"].as_i64().unwrap_or(-1));
        let improvement = match &tile["improvement"] {
            Value::Null => Value::Null,
            m => {
                let mut o = pick(
                    m,
                    &[
                        "kind",
                        // owner and founder decide only the look a player sees
                        "owner",
                        "founder",
                        "level",
                        "founded",
                        "xp",
                        "population",
                        "production",
                        "base_score",
                        "border_size",
                        "connected_to_capital_of_player",
                        "name",
                    ],
                );
                o["rewards"] = sorted(&m["rewards"])?;
                o["effects"] = sorted(&m["effects"])?;
                o
            }
        };
        let mut o = pick(
            tile,
            &[
                "coordinates",
                "terrain",
                "climate",
                "owner",
                "capital_of",
                "ruling_city",
                "resource",
                "has_road",
                "has_route",
                "skin",
            ],
        );
        // the order players explored a tile shows only on a lighthouse (a board corner)
        o["explorers"] = if corner {
            tile["explorers"].clone()
        } else {
            sorted(&tile["explorers"])?
        };
        o["effects"] = sorted(&tile["effects"])?;
        o["improvement"] = improvement;
        o["unit"] = unit(&tile["unit"], &units, &effect_lists, &sharers, 0)?;
        out_tiles.push(o);
    }
    let default = json!({"state": 0, "embassy_level": 0, "last_attack_turn": -100, "last_peace_broken_turn": -100});
    let mut players = Vec::new();
    for p in list(&state["players"], "players")? {
        let mut relations = Vec::new();
        for entry in list(&p["relations"], "relations")? {
            let r = pick(
                &entry[1],
                &[
                    "state",
                    "embassy_level",
                    "last_attack_turn",
                    "last_peace_broken_turn",
                ],
            );
            // a record with default values is the same as none
            if r != default {
                relations.push(json!([entry[0], r]));
            }
        }
        relations.sort_by_key(|x| x[0].as_i64());
        let mut tasks: Vec<Value> = p["action"]["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                pick(
                    t,
                    &["type", "flags", "peaceful_turns", "kills", "conversions"],
                )
            })
            .collect();
        tasks.sort_by_key(|x| x["type"].to_string());
        let mut o = pick(
            &p,
            &[
                "tribe",
                "score",
                "cities",
                "casualties",
                "start_tile",
                "handicap",
                "auto_play",
                "resigned_at_command",
                "wiped_at_command",
                "messages",
                "block_train_units",
                "skin",
                "end_score",
                "color",
            ],
        );
        o["id"] = p["action"]["id"].clone();
        o["currency"] = p["action"]["currency"].clone();
        o["available_tech"] = sorted(&p["available_tech"])?;
        o["known_players"] = sorted(&p["known_players"])?;
        o["built_unique_improvements"] = sorted(&p["built_unique_improvements"])?;
        o["relations"] = Value::Array(relations);
        o["tasks"] = Value::Array(tasks);
        players.push(o);
    }
    // a hash record is created from its seed on first use
    let seed = |hash: &str, source: &str| match &state["seeds"][hash] {
        Value::Null => state["seeds"][source].clone(),
        h => h["seed"].clone(),
    };
    let settings = &state["settings"];
    Ok(json!({
        "tiles": out_tiles,
        "players": players,
        "current_turn": state["current_turn"],
        "current_player_index": state["current_player_index"],
        "current_state": state["current_state"],
        // the game keeps a log, engine-fast its length
        "commands": match &checkpoint["game"]["commands"] {
            Value::Array(log) => json!(log.len()),
            count => count.clone(),
        },
        "current_command": state["current_command"],
        "actions": checkpoint["game"]["actions"]["entries"],
        "pending_triggers": state["pending_triggers"]["entries"],
        "needs_routes": state["needs_routes"],
        "gameplay_seed": seed("random_hash", "gameplay_seed"),
        "village_name_seed": seed("village_name_hash", "village_name_seed"),
        "naming_random_seeds": state["naming_random_seeds"],
        "naming_case": state["naming_case"],
        "rules_mode": settings["rules_mode"],
        "game_type": settings["game_type"],
        "rules": pick(&settings["rules"], &["turn_limit", "score_limit", "win_by_capital", "win_by_extermination", "death_condition"]),
        "achievements_active": state["achievements"]["active"],
        "tribes": checkpoint["config"]["tribes"],
        "action_limit": checkpoint["config"]["action_limit"],
        "decisions": checkpoint["decisions"],
    }))
}

/// The behaviour contract's verdict on two checkpoints: None when their projections are
/// equal, else the first path at which the projections differ.
pub fn difference(reference: &Value, ours: &Value) -> Result<Option<String>, String> {
    let (a, b) = (projection(reference)?, projection(ours)?);
    Ok((digest(&a) != digest(&b))
        .then(|| first_difference(&a, &b, &mut String::new()).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn digest_ignores_key_order_only() {
        assert_eq!(
            digest(&json!({"a": 1, "b": [1, 2]})),
            digest(&json!({"b": [1, 2], "a": 1}))
        );
        assert_ne!(
            digest(&json!({"a": 1, "b": [1, 2]})),
            digest(&json!({"a": 1, "b": [2, 1]}))
        );
        assert_ne!(digest(&json!({"a": 1})), digest(&json!({"a": 1.0})));
    }

    #[test]
    fn first_difference_names_the_path() {
        let a = json!({"x": {"units": [{"hp": 10}, {"hp": 5}]}});
        let b = json!({"x": {"units": [{"hp": 10}, {"hp": 4}]}});
        assert_eq!(
            first_difference(&a, &b, &mut String::new()).as_deref(),
            Some(".x.units[1].hp")
        );
        assert_eq!(first_difference(&a, &a, &mut String::new()), None);
    }
}
