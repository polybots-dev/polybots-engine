//! City capture and city names: the Capture command and CaptureCity action, and word making.

use crate::action::{Action, City, Combat, MoveEffect, Payload};
use crate::inline::Seats;
use crate::random::{HashFunction, SystemRandom};
use crate::state::{Game, Xy};
use crate::step::{attack_diplomacy, player_index, relation_entry, task_notification};
use crate::tables::Tables;

const FIELD: i32 = 3;
const INVISIBLE: i64 = 3;
const METROPOLIS_TASK: usize = 6;
const NULL: Xy = Xy { x: -1, y: -1 };
const BANNED: [&str; 4] = ["anus", "loli", "boner", "niga"];

fn relation_level(g: &mut Game, i: usize, other: i32) -> Result<i32, String> {
    Ok(relation_entry(g, i, other)?.embassy_level)
}

/// The state writes of the capture check: the relation lookup toward the city's owner.
pub fn admit_writes(g: &mut Game, p: i32, c: Xy) -> Result<(), String> {
    let owner = g.tile(c).ok_or("capture tile")?.owner;
    if owner != 0 {
        if let Ok(i) = player_index(g, p) {
            relation_entry(g, i, owner)?;
        }
    }
    Ok(())
}

/// The Capture command's children
pub fn capture_command(
    g: &mut Game,
    p: i32,
    unit: u32,
    c: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    crate::step::base(out, p, 7);
    let i = g.index(c).ok_or("capture tile")?;
    if g.tiles[i].improvement.is_none() {
        return Ok(());
    }
    let old = g.tiles[i].owner;
    if player_index(g, p).is_err() {
        return Ok(());
    }
    let id = u64::from(unit);
    let node = g
        .tiles
        .iter()
        .filter_map(|x| x.unit)
        .find(|&n| u64::from(g.units[n as usize].id) == id);
    let mut invisible = false;
    if let Some(n) = node {
        let u = g.units[n as usize];
        invisible = crate::menu::unit_effect_list(g, u.effects).contains(&INVISIBLE);
        crate::units::make_exhausted(g, n)?;
    }
    out.push(Action::UpdateCityConnections {
        actor: p,
        players: Seats::from_iter([p, old]),
    });
    out.push(Action::UpdateRoutes { player: p });
    out.push(Action::Combat(Combat::CaptureCity {
        player: p,
        coordinates: c,
        old_owner: old,
    }));
    if invisible {
        out.push(Action::MoveEffect(MoveEffect::Reveal {
            player: p,
            coordinates: c,
            origin: NULL,
            show_popup: false,
        }));
    }
    Ok(())
}

/// A word with the city-name seed, the persisted culture and the queue of default-Random seeds
pub(crate) fn city_name(
    g: &mut Game,
    t: &Tables,
    tribe: i32,
    skin: i64,
    c: Xy,
) -> Result<String, String> {
    let seed = g.seeds.village_name_seed;
    let hash = g
        .seeds
        .village_name_hash
        .get_or_insert(crate::state::HashSeed { seed });
    let hash = HashFunction(hash.seed as i32);
    let seed = hash
        .range(0, i32::MAX, &[c.x, c.y])
        .map_err(|_| "city name hash range")?;
    make_word(g, t, tribe, skin, seed)
}

/// The syllables drawn when no language applies (they include `da` twice).
pub const DEFAULT_SYLLABLES: [&str; 26] = [
    "po", "ly", "lu", "mi", "lo", "da", "bi", "oo", "sa", "ko", "me", "da", "to", "pi", "as", "an",
    "ki", "uh", "fi", "xi", "-", "sha", "ji", "us", "za", "um",
];

/// Language choice: a skin's language replaces its tribe's when the
/// catalog gives the skin one; no tribe, or a tribe without a language, speaks the default.
fn syllables(t: &Tables, tribe: i32, skin: i64) -> Result<Vec<&str>, String> {
    if tribe == 0 {
        return Ok(DEFAULT_SYLLABLES.to_vec());
    }
    let of_skin = t.skin_languages.iter().find(|(id, _)| *id == skin);
    let language = match of_skin {
        Some((_, language)) => language.as_str(),
        None => t
            .tribes
            .get(tribe as usize)
            .map_or("", |d| d.language.as_str()),
    };
    Ok(if language.is_empty() {
        DEFAULT_SYLLABLES.to_vec()
    } else {
        language.split(',').collect()
    })
}

/// Make a word (length drawn from the stream): a word in the tribe's language from
/// `seed`; seed 0 takes the next default-Random seed from the game's naming queue
pub(crate) fn make_word(
    g: &mut Game,
    t: &Tables,
    tribe: i32,
    skin: i64,
    seed: i32,
) -> Result<String, String> {
    let pieces = syllables(t, tribe, skin)?;
    let next_default = |g: &mut Game| -> Result<SystemRandom, String> {
        let seed = g
            .naming_seeds
            .pop_front()
            .ok_or("city name needs a default-Random seed")?;
        Ok(SystemRandom::new(seed as i32))
    };
    let mut rng = if seed == 0 {
        next_default(g)?
    } else {
        SystemRandom::new(seed)
    };
    let length = rng.next_float() * 3.0_f32 + 3.0_f32;
    let mut frames = 1;
    let mut word = loop {
        let mut word = String::new();
        let mut remaining = 101;
        while (word.encode_utf16().count() as f32) < length {
            remaining -= 1;
            if remaining == 0 {
                break;
            }
            let k = rng
                .next_range(0, pieces.len() as i32)
                .map_err(|_| "naming random range")? as usize;
            word.push_str(pieces[k]);
            if word.starts_with(' ') || word.starts_with('-') {
                word.clear();
            }
        }
        if !BANNED.contains(&word.as_str()) {
            break word;
        }
        rng = next_default(g)?;
        frames += 1;
    };
    if tribe != 5 {
        let case = g.fixed.naming_case.clone();
        for _ in 0..frames {
            let mut units: Vec<u16> = word.encode_utf16().collect();
            let first = units.first_mut().ok_or("empty city name")?;
            *first = match *first {
                0x69 if case == "Common" => 0x49,
                0x69 if case == "Turkic" => 0x130,
                0xfc => 0xdc,
                0x103 => 0x102,
                u if u > 127 || u == 0x69 => {
                    return Err("city name casing needs the culture".into())
                }
                u if (0x61..=0x7a).contains(&u) => u - 32,
                u => u,
            };
            word = String::from_utf16(&units).map_err(|_| "city name UTF-16")?;
        }
    }
    Ok(word)
}

/// The sorted border area, keeping tiles ruled by nobody or this city
fn city_area(g: &Game, c: Xy) -> Result<Vec<usize>, String> {
    let radius = g
        .tile(c)
        .and_then(|x| x.improvement.as_ref())
        .ok_or("city area improvement")?
        .border_size;
    Ok(crate::explore::area_sorted(g, c, radius)?
        .iter()
        .copied()
        .filter(|&i| g.tiles[i].ruling_city == NULL || g.tiles[i].ruling_city == c)
        .collect())
}

fn tribe_has(g: &Game, t: &Tables, i: usize, a: &str) -> bool {
    t.tribes
        .get(g.players[i].tribe as usize)
        .is_some_and(|d| d.tribe_abilities.has(a))
}

/// The CaptureCity combat action
pub fn capture_city(
    g: &mut Game,
    t: &Tables,
    actor: i32,
    city: Xy,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let center = g.index(city).ok_or("capture city tile")?;
    let ai = player_index(g, actor)?;
    let named = g.tiles[center]
        .improvement
        .as_ref()
        .ok_or("capture city improvement")?
        .named;
    if !named {
        let (tribe, skin) = (g.players[ai].tribe, i64::from(g.players[ai].skin));
        if t.tribes.get(tribe as usize).is_none() {
            return Err("naming a city for an unknown tribe not implemented".into());
        }
        let name = city_name(g, t, tribe, skin, city)?;
        g.name_city(center, name);
    }
    if let Some(n) = g.tiles[center].unit {
        let kind = g.units[n as usize].kind;
        if !t
            .units
            .get(kind as usize)
            .is_some_and(|d| d.unit_abilities.has("independent"))
        {
            g.units[n as usize].home = city;
        }
    }
    let nodes: Vec<u32> = g.tiles.iter().filter_map(|x| x.unit).collect();
    for n in nodes {
        let u = &mut g.units[n as usize];
        if u.owner != actor && u.home == city {
            u.home = NULL;
        }
    }
    g.players[ai].cities += 1;
    let old = g.tiles[center].owner;
    let previous = player_index(g, old).ok();
    if let Some(pi) = previous {
        g.players[pi].cities -= 1;
    }
    let mut area = city_area(g, city)?;
    area.reverse();
    let (mut scores, mut new_tiles) = (Vec::new(), Vec::new());
    for &i in &area {
        let tile = g.tiles[i];
        if tile.owner != actor {
            let mut amount = 20;
            if tile.improvement.is_some() && tile.coordinates != city {
                amount += crate::economy::improvement_score(t, g, i);
            }
            scores.push(Action::Payload(Payload::IncreaseScore {
                player: actor,
                amount,
                source: tile.coordinates,
                delay: 50,
            }));
            if tile.owner == 0 {
                new_tiles.push(i);
            } else {
                scores.push(Action::Combat(Combat::DecreaseScore {
                    player: tile.owner,
                    amount,
                }));
            }
        }
        g.tiles[i].owner = actor;
        g.tiles[i].ruling_city = city;
    }
    if g.tiles[center].improvement.as_ref().unwrap().level > 1 {
        if g.players[ai].task(6).is_none() {
            out.push(Action::EnableTask {
                player: actor,
                task: METROPOLIS_TASK as u32,
            });
        }
        out.extend(task_notification(g, t, ai, METROPOLIS_TASK)?);
    }
    out.extend(scores);
    for i in new_tiles {
        let tile = &g.tiles[i];
        let population = tile
            .improvement
            .as_ref()
            .and_then(|m| {
                t.improvements
                    .get(m.kind as usize)
                    .map(|d| crate::economy::population_at_level(d, m.level))
            })
            .unwrap_or(0);
        for _ in 0..population.max(0) {
            out.push(Action::City(City::IncreasePopulation {
                player: actor,
                source: tile.coordinates,
                target: city,
                delay: 60,
            }));
        }
    }
    let (selected, completed) = crate::turns::winner(g)?;
    if completed {
        out.push(Action::GameOver {
            player: actor,
            winner: selected,
        });
    }
    if previous.is_some() && !crate::menu::alive(g, old)? {
        out.push(Action::WipePlayer {
            player: actor,
            target: old,
        });
    }
    let score = crate::economy::improvement_score(t, g, center);
    crate::step::add_score(g, actor, i64::from(score), out)?;
    if old != 0 {
        out.push(Action::Combat(Combat::DecreaseScore {
            player: old,
            amount: score,
        }));
    }
    let mut area = city_area(g, city)?;
    area.reverse();
    crate::polaris::claim_area(g, t, actor, ai, &area, out)?;
    let radius = g.tiles[center].improvement.as_ref().unwrap().border_size;
    let h = g.tiles.len() as i32 / g.width;
    for xx in (city.x - radius).max(0)..=(city.x + radius).min(g.width - 1) {
        for yy in (city.y - radius).max(0)..=(city.y + radius).min(h - 1) {
            let c = Xy { x: xx, y: yy };
            if !g.tile(c).unwrap().explorers.contains(&actor) {
                out.push(Action::Explore {
                    player: actor,
                    coordinates: c,
                });
            }
        }
    }
    let ids: Vec<i32> = g.players.iter().map(|p| p.action.id).collect();
    let capital = g.tiles[center].capital_of;
    if capital != 0 {
        let ci = player_index(g, capital)?;
        for &other in &ids {
            if relation_level(g, ci, other)? > 0 {
                out.push(Action::DestroyEmbassy {
                    player: other,
                    opponent: capital,
                });
            }
        }
    }
    if old != 0 {
        attack_diplomacy(g, actor, old, out)?;
    }
    for &other in &ids {
        if relation_level(g, ai, other)? >= 2 && !g.tiles[center].explorers.contains(&other) {
            out.push(Action::Explore {
                player: other,
                coordinates: city,
            });
        }
    }
    // the city tile becomes field, flooded by a flooding tribe and drained by any other
    g.tiles[center].terrain = FIELD;
    if tribe_has(g, t, ai, "flooded") {
        crate::aquarion::flood_tile(g, actor, city)?;
    } else {
        crate::aquarion::drain_tile(g, city)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Written from the game's language choice:
    /// skin language if the catalog has one, else the tribe's, else the default.
    #[test]
    fn a_skin_language_replaces_the_tribes_and_no_language_is_the_default() {
        let t = Tables::compile(include_str!(env!("ENGINE_FAST_RULES"))).unwrap();
        let imperius = t.tribes.id("imperius").unwrap() as i32;
        let own = syllables(&t, imperius, 0).unwrap();
        assert_eq!(
            own,
            t.tribes
                .get(imperius as usize)
                .unwrap()
                .language
                .split(',')
                .collect::<Vec<_>>()
        );
        // Swamp (17), Magma (18) and Cute (19) have languages; a skin without one keeps the tribe's
        assert_eq!(
            syllables(&t, imperius, 19).unwrap(),
            "do,re,mi,fa,so,la,ti,bo,lo,mo"
                .split(',')
                .collect::<Vec<_>>()
        );
        assert_eq!(syllables(&t, imperius, 17).unwrap()[0], "bub");
        assert_eq!(syllables(&t, imperius, 2).unwrap(), own);
        // no tribe, and Nature (which has no language), speak the default
        assert_eq!(syllables(&t, 0, 19).unwrap(), DEFAULT_SYLLABLES);
        assert_eq!(syllables(&t, 1, 0).unwrap(), DEFAULT_SYLLABLES);
        assert_eq!(DEFAULT_SYLLABLES.iter().filter(|s| **s == "da").count(), 2);
    }
}
