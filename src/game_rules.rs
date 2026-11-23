//! The game's own rule asset (the rule text the game runs with), read into the engine's rule
//! types (`tables`). Parsing is strict: a key this module does not know is
//! refused, so a change to the asset cannot silently drop a rule. Keys the engine does not read
//! are named here and skipped (`IgnoredAny`).

use crate::tables::{
    Abilities, ImprovementData, Indexed, Requirement, Reward, Rules, SkinLanguage, Strict,
    TechData, TribeData, UnitData,
};
use serde::de::IgnoredAny;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Unit {
    idx: usize,
    #[serde(default)]
    hidden: bool,
    cost: i32,
    health: i32,
    defence: i32,
    movement: i32,
    weapon: i32,
    range: i32,
    attack: i32,
    promotion_limit: i32,
    #[serde(default)]
    unit_abilities: Abilities,
    growth_rate: Option<i32>,
    upgrades_from: Option<String>,
    #[serde(default)]
    movement_terrain: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Tech {
    idx: usize,
    cost: i32,
    #[serde(default)]
    tech_unlocks: Vec<String>,
    #[serde(default)]
    unit_unlocks: Vec<String>,
    #[serde(default)]
    improvement_unlocks: Vec<String>,
    /// only the terrain names are read
    #[serde(default)]
    movement_unlocks: BTreeMap<String, IgnoredAny>,
    #[serde(default)]
    ability_unlocks: Abilities,
    #[serde(default)]
    task_unlocks: Vec<String>,
    #[serde(default)]
    defence_bonus_unlocks: BTreeMap<String, i64>,
}

/// An entry of an improvement's lists. `resources` is the key the asset's adjacency lists use
/// for a resource; the other lists use `resource`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Names {
    terrain: Option<String>,
    resource: Option<String>,
    resources: Option<String>,
    improvement: Option<String>,
    not_improvement: Option<String>,
    effect: Option<String>,
    unit: Option<String>,
}

impl Names {
    /// The entry of a list that names a resource with `resource` (all but adjacency).
    fn requirement(self) -> Result<Requirement, String> {
        if self.resources.is_some() {
            return Err("an improvement list entry names `resources`".into());
        }
        Ok(Requirement {
            terrain: self.terrain,
            resource: self.resource,
            improvement: self.improvement,
            not_improvement: self.not_improvement,
            effect: self.effect,
            unit: self.unit,
        })
    }

    /// An adjacencyImprovements entry: the engine reads its improvement and `resources` only.
    fn adjacency(self) -> Result<Requirement, String> {
        if self.terrain.is_some()
            || self.resource.is_some()
            || self.not_improvement.is_some()
            || self.effect.is_some()
            || self.unit.is_some()
        {
            return Err("an adjacency entry with a key the engine does not read".into());
        }
        Ok(Requirement {
            improvement: self.improvement,
            resource: self.resources,
            ..Requirement::default()
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Amounts {
    #[serde(default)]
    population: i32,
    #[serde(default)]
    score: i32,
    #[serde(default)]
    currency: i32,
}

impl From<Amounts> for Reward {
    fn from(a: Amounts) -> Reward {
        Reward {
            population: a.population,
            score: a.score,
            currency: a.currency,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Improvement {
    idx: usize,
    #[serde(default)]
    hidden: bool,
    max_level: Option<i32>,
    cost: Option<i32>,
    work: Option<i32>,
    border_size: Option<i32>,
    #[serde(default)]
    terrain_requirements: Vec<Names>,
    #[serde(default)]
    improvement_abilities: Abilities,
    #[serde(default)]
    rewards: Vec<Amounts>,
    #[allow(dead_code)] // a hint for the game's interface
    should_suggest_unlock: Option<IgnoredAny>,
    #[serde(default)]
    adjacency_improvements: Vec<Names>,
    #[serde(default)]
    adjacency_requirements: Vec<Names>,
    #[serde(default)]
    growth_rewards: Vec<Amounts>,
    range: Option<i32>,
    #[serde(default)]
    routes: Vec<String>,
    #[serde(default)]
    creates: Vec<Names>,
    growth_rate: Option<i32>,
}

/// The tribe keys no rule reads: its art style, store tier, category and skins.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[allow(dead_code)]
struct Tribe {
    idx: usize,
    color: i64,
    style: IgnoredAny,
    climate: i32,
    language: String,
    bonus: i32,
    starting_unit: String,
    category: IgnoredAny,
    price_tier: IgnoredAny,
    #[serde(default)]
    starting_tech: Vec<String>,
    #[serde(default)]
    tribe_abilities: Abilities,
    #[serde(default)]
    starting_resource: Vec<String>,
    #[serde(default)]
    terrain_modifier: BTreeMap<String, f64>,
    skins: Option<IgnoredAny>,
    #[serde(default)]
    resource_modifier: BTreeMap<String, f64>,
    starting_stars: Option<i32>,
    #[serde(default)]
    unit_overrides: BTreeMap<String, String>,
    #[serde(default)]
    tech_overrides: BTreeMap<String, String>,
    #[serde(default)]
    improvement_overrides: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Kind {
    idx: usize,
    #[serde(default)]
    resource_terrain_requirements: Vec<String>,
    #[serde(default)]
    improvement_unlocks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Skin {
    #[allow(dead_code)] // the skin's display colour
    color: Option<IgnoredAny>,
    language: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Asset {
    tribe_data: BTreeMap<String, Tribe>,
    tech_data: BTreeMap<String, Tech>,
    unit_data: BTreeMap<String, Unit>,
    improvement_data: BTreeMap<String, Improvement>,
    terrain_data: BTreeMap<String, Kind>,
    resource_data: BTreeMap<String, Kind>,
    task_data: BTreeMap<String, Kind>,
    skin_data: BTreeMap<String, Skin>,
    /// only embassyIncome is read
    diplomacy_data: BTreeMap<String, Value>,
}

/// the game's skin type ids of the skins the asset can describe (it names them, lowercase)
const SKINS: [(&str, i64); 17] = [
    ("default", 0),
    ("ranger", 1),
    ("ninja", 2),
    ("baerion", 3),
    ("scholar", 5),
    ("mercenary", 7),
    ("sfinx", 8),
    ("skeleton", 9),
    ("arty", 10),
    ("pirate", 11),
    ("aibo", 12),
    ("urkaz", 13),
    ("ikarus", 14),
    ("darkelf", 15),
    ("swamp", 17),
    ("magma", 18),
    ("cute", 19),
];

fn list<T, U>(items: Vec<T>, f: impl Fn(T) -> Result<U, String>) -> Result<Vec<U>, String> {
    items.into_iter().map(f).collect()
}

/// The game's asset text as the engine's rules.
pub(crate) fn read(text: &str) -> Result<Rules, String> {
    // The game's own data repeats a key in one entry (techData.rituals has "cost" twice, both 2).
    // Identical repeats are accepted; a repeat with a different value is refused.
    let v: Strict = serde_json::from_str(text).map_err(|e| format!("catalog: {e}"))?;
    // The colour of a second player of one tribe is drawn from the playable tribes in the
    // asset's order; the engine's rules keep them in id order, so the two must agree.
    let order: Vec<i64> = v.0["tribeData"]
        .as_object()
        .ok_or("catalog tribes")?
        .values()
        .map(|t| t["idx"].as_i64().ok_or("tribe idx"))
        .collect::<Result<_, _>>()?;
    if order.windows(2).any(|w| w[0] >= w[1]) {
        return Err("catalog tribes are not in id order".into());
    }
    let a: Asset = serde_json::from_value(v.0).map_err(|e| format!("catalog: {e}"))?;
    let mut skin_languages = Vec::new();
    for (name, skin) in a.skin_data {
        let id = SKINS
            .iter()
            .find(|(n, _)| *n == name)
            .ok_or("catalog skin")?
            .1;
        if let Some(language) = skin.language.filter(|l| !l.is_empty()) {
            skin_languages.push(SkinLanguage { skin: id, language });
        }
    }
    let embassy_income = match a.diplomacy_data.get("embassyIncome") {
        None => None,
        Some(v) => Some(v.as_i64().ok_or("embassy income is not an integer")?),
    };
    let mut improvements = BTreeMap::new();
    for (name, m) in a.improvement_data {
        let entry = ImprovementData {
            idx: m.idx,
            hidden: m.hidden,
            max_level: m.max_level,
            cost: m.cost,
            work: m.work,
            border_size: m.border_size,
            terrain_requirements: list(m.terrain_requirements, Names::requirement)?,
            improvement_abilities: m.improvement_abilities,
            rewards: m.rewards.into_iter().map(Reward::from).collect(),
            adjacency_improvements: list(m.adjacency_improvements, Names::adjacency)?,
            adjacency_requirements: list(m.adjacency_requirements, Names::requirement)?,
            growth_rewards: m.growth_rewards.into_iter().map(Reward::from).collect(),
            range: m.range,
            routes: m.routes,
            creates: list(m.creates, Names::requirement)?,
            growth_rate: m.growth_rate,
        };
        improvements.insert(name, entry);
    }
    let kinds = |m: BTreeMap<String, Kind>| {
        m.into_iter()
            .map(|(name, k)| {
                let entry = Indexed {
                    idx: k.idx,
                    resource_terrain_requirements: k.resource_terrain_requirements,
                    improvement_unlocks: k.improvement_unlocks,
                };
                (name, entry)
            })
            .collect()
    };
    Ok(Rules {
        tribes: a
            .tribe_data
            .into_iter()
            .map(|(name, t)| {
                let entry = TribeData {
                    idx: t.idx,
                    color: t.color,
                    climate: t.climate,
                    language: t.language,
                    bonus: t.bonus,
                    starting_unit: t.starting_unit,
                    starting_tech: t.starting_tech,
                    tribe_abilities: t.tribe_abilities,
                    starting_resource: t.starting_resource,
                    terrain_modifier: t.terrain_modifier,
                    resource_modifier: t.resource_modifier,
                    starting_stars: t.starting_stars,
                    unit_overrides: t.unit_overrides,
                    tech_overrides: t.tech_overrides,
                    improvement_overrides: t.improvement_overrides,
                };
                (name, entry)
            })
            .collect(),
        techs: a
            .tech_data
            .into_iter()
            .map(|(name, t)| {
                let entry = TechData {
                    idx: t.idx,
                    cost: t.cost,
                    tech_unlocks: t.tech_unlocks,
                    unit_unlocks: t.unit_unlocks,
                    improvement_unlocks: t.improvement_unlocks,
                    movement_unlocks: t.movement_unlocks.into_keys().collect(),
                    ability_unlocks: t.ability_unlocks,
                    task_unlocks: t.task_unlocks,
                    defence_bonus_unlocks: t.defence_bonus_unlocks,
                };
                (name, entry)
            })
            .collect(),
        units: a
            .unit_data
            .into_iter()
            .map(|(name, u)| {
                let entry = UnitData {
                    idx: u.idx,
                    hidden: u.hidden,
                    cost: u.cost,
                    health: u.health,
                    defence: u.defence,
                    movement: u.movement,
                    weapon: u.weapon,
                    range: u.range,
                    attack: u.attack,
                    promotion_limit: u.promotion_limit,
                    unit_abilities: u.unit_abilities,
                    growth_rate: u.growth_rate,
                    upgrades_from: u.upgrades_from,
                    movement_terrain: u.movement_terrain,
                };
                (name, entry)
            })
            .collect(),
        improvements,
        terrain: kinds(a.terrain_data),
        resources: kinds(a.resource_data),
        tasks: kinds(a.task_data),
        skin_languages,
        embassy_income,
    })
}
