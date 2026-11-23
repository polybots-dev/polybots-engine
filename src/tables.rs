//! Rule tables, compiled from rule data in the engine's own format (`Rules`, the files in
//! `rules/`) or from the game's own rule asset (which a checkpoint made from it carries as
//! `game.catalog_json`; `game_rules` reads it). Both give the same tables. Parsing is
//! strict: a key the engine does not know is refused, so a change to the data cannot silently
//! drop a rule. Entries are stored by id (the game's enum id); names map to ids.

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// A catalog ability list. Membership of a name the catalog shipped with is one bit test; any
/// other name is looked up in the list itself, so a changed catalog stays exact.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Abilities {
    names: Vec<String>,
    mask: u128,
}

/// the bit of an ability name (unit, improvement, tribe and tech abilities of game_logic_28)
#[inline(always)]
fn ability_bit(name: &str) -> Option<u32> {
    Some(match name {
        "algae" => 0,
        "alienclimate" => 1,
        "amphibious" => 2,
        "attract" => 3,
        "autoflood" => 4,
        "autofreeze" => 5,
        "bridge" => 6,
        "bubble" => 7,
        "capitalvision" => 8,
        "carry" => 9,
        "consumed" => 10,
        "convert" => 11,
        "creep" => 12,
        "dash" => 13,
        "decompose" => 14,
        "destroy" => 15,
        "disband" => 16,
        "discrete" => 17,
        "doubleattack" => 18,
        "drench" => 19,
        "eat" => 20,
        "embark" => 21,
        "embassy" => 22,
        "escape" => 23,
        "explode" => 24,
        "flood" => 25,
        "flooded" => 26,
        "fly" => 27,
        "fortify" => 28,
        "freelance" => 29,
        "freeze" => 30,
        "freezearea" => 31,
        "glide" => 32,
        "grow" => 33,
        "heal" => 34,
        "hide" => 35,
        "independent" => 36,
        "infiltrate" => 37,
        "land" => 38,
        "limited" => 39,
        "literacy" => 40,
        "manual" => 41,
        "microbes" => 42,
        "network" => 43,
        "patina" => 44,
        "peacetreaty" => 45,
        "persist" => 46,
        "poison" => 47,
        "poisonresist" => 48,
        "pontoon" => 49,
        "rainbowvision" => 50,
        "scout" => 51,
        "simple" => 52,
        "skate" => 53,
        "sneak" => 54,
        "splash" => 55,
        "spread" => 56,
        "static" => 57,
        "stiff" => 58,
        "stomp" => 59,
        "surprise" => 60,
        "swarm" => 61,
        "swim" => 62,
        "tentacles" => 63,
        "unique" => 64,
        "water" => 65,
        // names the rules ask about that game_logic_28 does not list
        "autoheal" => 66,
        "boost" => 67,
        "discipline" => 68,
        "doubled" => 69,
        "fill" => 70,
        "navigate" => 71,
        "picky" => 72,
        "protect" => 73,
        "slow" => 74,
        "treehouse" => 75,
        _ => return None,
    })
}

impl Abilities {
    #[inline(always)]
    pub fn has(&self, name: &str) -> bool {
        match ability_bit(name) {
            Some(bit) => self.mask >> bit & 1 == 1,
            None => self.names.iter().any(|x| x == name),
        }
    }
}

/// The bit of terrain `id` in a terrain mask (none for an id past the mask).
pub(crate) fn terrain_bit(id: i32) -> u32 {
    1u32.checked_shl(id as u32).unwrap_or(0)
}

/// Whether the ability mask `mask` (`Tables::abilities`) has `name`. Every name the rules ask
/// about has a bit (`ability_bit`); a name without one is never in a mask.
#[inline(always)]
pub(crate) fn unlocks(mask: u128, name: &str) -> bool {
    ability_bit(name).is_some_and(|bit| mask >> bit & 1 == 1)
}

impl Abilities {
    fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

impl Serialize for Abilities {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.names.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Abilities {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let names = Vec::<String>::deserialize(d)?;
        let mask = names
            .iter()
            .filter_map(|n| ability_bit(n))
            .fold(0, |m, bit| m | 1u128 << bit);
        Ok(Abilities { names, mask })
    }
}

/// A unit kind. The rule types below are the engine's own rule format (`rules/`), field for
/// field: only what the engine reads, typed. `game_rules` reads the game's asset into them.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnitData {
    #[serde(rename = "id")]
    pub idx: usize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    pub cost: i32,
    pub health: i32,
    pub defence: i32,
    pub movement: i32,
    pub weapon: i32,
    pub range: i32,
    pub attack: i32,
    pub promotion_limit: i32,
    #[serde(default, skip_serializing_if = "Abilities::is_empty")]
    pub unit_abilities: Abilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub growth_rate: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrades_from: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub movement_terrain: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TechData {
    #[serde(rename = "id")]
    pub idx: usize,
    pub cost: i32,
    /// in the order the research tree is walked
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tech_unlocks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unit_unlocks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub improvement_unlocks: Vec<String>,
    /// the terrains this tech lets units move on
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub movement_unlocks: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Abilities::is_empty")]
    pub ability_unlocks: Abilities,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_unlocks: Vec<String>,
    /// a defence bonus by terrain name (a value above 1 gives the bonus)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defence_bonus_unlocks: BTreeMap<String, i64>,
}

/// An entry of an improvement's terrain, adjacency or creates list: the names it gives.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub improvement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_improvement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// What building (or growing) an improvement gives; an absent amount is 0.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reward {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub population: i32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub score: i32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub currency: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImprovementData {
    #[serde(rename = "id")]
    pub idx: usize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_level: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_size: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terrain_requirements: Vec<Requirement>,
    #[serde(default, skip_serializing_if = "Abilities::is_empty")]
    pub improvement_abilities: Abilities,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewards: Vec<Reward>,
    /// the neighbours that add to this improvement's level: improvement or resource entries
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adjacency_improvements: Vec<Requirement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adjacency_requirements: Vec<Requirement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub growth_rewards: Vec<Reward>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<i32>,
    /// the terrains a route of this improvement crosses
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creates: Vec<Requirement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub growth_rate: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TribeData {
    #[serde(rename = "id")]
    pub idx: usize,
    pub color: i64,
    pub climate: i32,
    /// the syllables city and player names are made of, comma separated
    pub language: String,
    pub bonus: i32,
    pub starting_unit: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starting_tech: Vec<String>,
    #[serde(default, skip_serializing_if = "Abilities::is_empty")]
    pub tribe_abilities: Abilities,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starting_resource: Vec<String>,
    /// the map generator's terrain weights for this tribe's land, by terrain name
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub terrain_modifier: BTreeMap<String, f64>,
    /// the map generator's resource weights for this tribe's land, by resource name
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resource_modifier: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starting_stars: Option<i32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unit_overrides: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tech_overrides: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub improvement_overrides: BTreeMap<String, String>,
}

/// A terrain, resource or task kind.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Indexed {
    #[serde(rename = "id")]
    pub idx: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_terrain_requirements: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub improvement_unlocks: Vec<String>,
}

fn is_false(x: &bool) -> bool {
    !x
}

fn is_zero(x: &i32) -> bool {
    *x == 0
}

/// The format name the engine's own rule files carry in their `format` key.
pub const FORMAT: &str = "engine-fast-rules-1";

/// Rule data in the engine's own format: every section by name, each entry with its id. A rule
/// file is this object with two more keys: `format` (`FORMAT`) and `name` (which rules).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub tribes: BTreeMap<String, TribeData>,
    pub techs: BTreeMap<String, TechData>,
    pub units: BTreeMap<String, UnitData>,
    pub improvements: BTreeMap<String, ImprovementData>,
    pub terrain: BTreeMap<String, Indexed>,
    pub resources: BTreeMap<String, Indexed>,
    pub tasks: BTreeMap<String, Indexed>,
    /// the naming syllables of the skins that have their own
    pub skin_languages: Vec<SkinLanguage>,
    /// an embassy's income per level (doubled at peace)
    pub embassy_income: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkinLanguage {
    /// the game's skin id
    pub skin: i64,
    pub language: String,
}

/// One catalog section stored by id, with its name lookup.
#[derive(Debug, PartialEq)]
pub struct Section<T> {
    pub by_id: Vec<Option<(String, T)>>,
    pub id_of: BTreeMap<String, usize>,
}

impl<T> Section<T> {
    fn build(
        entries: BTreeMap<String, T>,
        idx: impl Fn(&T) -> usize,
        what: &str,
    ) -> Result<Self, String> {
        let mut by_id: Vec<Option<(String, T)>> = Vec::new();
        let mut id_of = BTreeMap::new();
        for (name, e) in entries {
            let i = idx(&e);
            if by_id.len() <= i {
                by_id.resize_with(i + 1, || None);
            }
            if by_id[i].is_some() {
                return Err(format!("{what}: two entries with idx {i}"));
            }
            id_of.insert(name.clone(), i);
            by_id[i] = Some((name, e));
        }
        Ok(Section { by_id, id_of })
    }

    pub fn get(&self, id: usize) -> Option<&T> {
        self.by_id.get(id).and_then(|e| e.as_ref()).map(|(_, e)| e)
    }

    /// one past the largest id
    #[allow(clippy::len_without_is_empty)] // an id bound, not a count of entries
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn name(&self, id: usize) -> Result<&str, String> {
        self.by_id
            .get(id)
            .and_then(|e| e.as_ref())
            .map(|(n, _)| n.as_str())
            .ok_or_else(|| format!("catalog has no entry {id}"))
    }

    pub fn id(&self, name: &str) -> Result<usize, String> {
        self.id_of
            .get(name)
            .copied()
            .ok_or_else(|| format!("catalog has no entry {name}"))
    }
}

/// FNV-1a over the catalogue text: which text a `Tables` was compiled from.
fn text_digest(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Resolving within one section: `name` as the tribe with `overrides` gets it.
/// None when the override names no catalog entry.
fn resolve<T>(
    section: &Section<T>,
    overrides: &BTreeMap<String, String>,
    name: &str,
) -> Result<Option<usize>, String> {
    let raw = section.id(name)?;
    Ok(match overrides.get(name) {
        None => Some(raw),
        Some(target) => section.id_of.get(target).copied(),
    })
}

/// Which rule data a `Tables` was compiled from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// the game's asset, which a new game's state carries in full (`catalog_json`)
    Game,
    /// a rule file in the engine's format, which a new game's state names (`rules`)
    Own { name: String, sha256: String },
}

#[derive(Debug)]
pub struct Tables {
    /// the digest of the text these tables were compiled from (`compiled_from`)
    text: u64,
    source: Source,
    pub tribes: Section<TribeData>,
    pub techs: Section<TechData>,
    pub units: Section<UnitData>,
    pub improvements: Section<ImprovementData>,
    pub terrain: Section<Indexed>,
    pub resources: Section<Indexed>,
    pub tasks: Section<Indexed>,
    /// an embassy's income per level (doubled at peace)
    pub embassy_income: Option<i64>,
    /// a skin's naming language by skin id, for the skins the rules give one
    pub(crate) skin_languages: Vec<(i64, String)>,
    /// the colours of the playable tribes in id order, which the colour of a second player of
    /// the same tribe is drawn from
    pub(crate) tribe_colors: Vec<i32>,
    /// per improvement kind, filled on first use (routes.rs)
    pub(crate) routers: std::sync::OnceLock<Vec<Result<Option<crate::routes::Router>, String>>>,
    /// the tech tree by id, one per tribe id with that tribe's overrides resolved,
    /// filled on first use
    tech_trees: std::sync::OnceLock<Vec<TechTree>>,
    /// per tech id, bit k set: its movementUnlocks name terrain k (filled on first use)
    tech_movements: std::sync::OnceLock<Vec<Result<u32, String>>>,
    /// per improvement kind, filled on first use
    requirements: std::sync::OnceLock<Vec<Requirements>>,
    /// per resource id, the terrain ids it may sit on
    resource_terrains: std::sync::OnceLock<Vec<Result<Vec<i32>, String>>>,
}

/// One entry of an improvement's requirement lists with its names resolved once. An unknown
/// name stays an error for the rule that reads it.
#[derive(Debug, PartialEq)]
pub(crate) struct Names {
    pub resource: Option<Result<i32, String>>,
    pub terrain: Option<Result<i32, String>>,
    pub improvement: Option<Result<i32, String>>,
    pub not_improvement: Option<Result<i32, String>>,
    pub effect: Option<String>,
    /// the entry has a resource, terrain or improvement key (whatever its value)
    pub positive: bool,
}

/// An improvement's terrainRequirements, adjacencyRequirements and creates lists.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Requirements {
    pub terrain: Vec<Names>,
    pub adjacency: Vec<Names>,
    pub creates: Vec<Names>,
}

/// The tech tree with its names resolved, per tech id, for one tribe: its unit, tech and
/// improvement overrides applied as the section resolve does. Errors stay per entry: a tech
/// nothing asks about cannot refuse a query.
#[derive(Debug, PartialEq)]
pub(crate) struct TechTree {
    // the tribe's tech tree: breadth-first from tech 0, each queued tech resolved before the
    /// resolved before the first-visit check, at most 200 dequeues
    pub order: Result<Vec<usize>, String>,
    /// techUnlocks as ids, each resolved
    pub children: Vec<Result<Vec<usize>, String>>,
    /// bit k set: an improvement this tech unlocks (not hidden) needs resource kind k
    pub reveals: Vec<Result<u32, String>>,
    /// unitUnlocks as resolved ids, in list order
    units: Vec<Result<Vec<usize>, String>>,
    /// improvementUnlocks as resolved ids, in list order. A name with no catalog entry is a null
    /// reference and left out; an override to "none" resolves
    /// to the hidden "none" improvement, which the readers skip as hidden.
    improvements: Vec<Result<Vec<usize>, String>>,
}

impl TechTree {
    /// The unit kinds tech `tech` unlocks, in the catalog's order.
    pub(crate) fn units_of(&self, tech: i32) -> Result<&[usize], String> {
        match self.units.get(tech as usize) {
            Some(list) => list.as_deref().map_err(Clone::clone),
            None => Err("unknown tech".into()),
        }
    }

    /// The improvement kinds tech `tech` unlocks, in the catalog's order.
    pub(crate) fn improvements_of(&self, tech: i32) -> Result<&[usize], String> {
        match self.improvements.get(tech as usize) {
            Some(list) => list.as_deref().map_err(Clone::clone),
            None => Err("unknown tech".into()),
        }
    }
}

impl Tables {
    /// Whether these tables were compiled from `text` (byte for byte).
    pub fn compiled_from(&self, text: &str) -> bool {
        self.text == text_digest(text)
    }

    /// Which rule data these tables were compiled from.
    pub fn source(&self) -> &Source {
        &self.source
    }
}

impl Tables {
    /// Whether these tables and `o` hold the same rules: every table, the ones filled on first
    /// use included (this fills them). The text compiled from and its kind (`source`) do not
    /// count.
    pub fn same_tables(&self, o: &Tables) -> bool {
        let filled = |t: &Tables| {
            for kind in 0..t.improvements.len() {
                let _ = crate::routes::router(t, kind as i32);
                t.requirements(kind);
            }
            let _ = t.trees();
            let _ = t.movements(&[]);
            let _ = t.resource_terrains(0);
        };
        filled(self);
        filled(o);
        self.tribes == o.tribes
            && self.techs == o.techs
            && self.units == o.units
            && self.improvements == o.improvements
            && self.terrain == o.terrain
            && self.resources == o.resources
            && self.tasks == o.tasks
            && self.embassy_income == o.embassy_income
            && self.skin_languages == o.skin_languages
            && self.tribe_colors == o.tribe_colors
            && self.routers.get() == o.routers.get()
            && self.tech_trees.get() == o.tech_trees.get()
            && self.tech_movements.get() == o.tech_movements.get()
            && self.requirements.get() == o.requirements.get()
            && self.resource_terrains.get() == o.resource_terrains.get()
    }
}

/// Units, improvements and techs have ids below this (the fixed action table's id blocks).
pub const MAX_ID_BLOCK: usize = 64;

impl Tables {
    /// Compiles rule data: a rule file in the engine's format (it has a top-level `format` key,
    /// which the game's asset does not) or the game's asset (a checkpoint's `game.catalog_json`).
    pub fn compile(text: &str) -> Result<Tables, String> {
        let head: BTreeMap<String, serde::de::IgnoredAny> =
            serde_json::from_str(text).map_err(|e| format!("rules: {e}"))?;
        let (rules, source) = if head.contains_key("format") {
            let (rules, name) = read_own(text)?;
            let sha256 = sha2::Sha256::digest(text.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            (rules, Source::Own { name, sha256 })
        } else {
            (crate::game_rules::read(text)?, Source::Game)
        };
        Tables::build(rules, text_digest(text), source)
    }

    fn build(r: Rules, text: u64, source: Source) -> Result<Tables, String> {
        let furthest = r.units.values().map(|u| u.movement).max().unwrap_or(0);
        if r.units
            .values()
            .any(|u| u.range > crate::combat::ATTACK_RANGE as i32)
        {
            return Err("a unit attacks further than a target list holds".into());
        }
        if 2 * (furthest as usize + 2) + 1 > crate::action::PATH_TILES {
            return Err("a unit moves further than a move path holds".into());
        }
        // the fixed action table gives units, improvements and techs 64 slots each: a larger id would need a new layout
        let ids = |idx: Vec<usize>, what: &str| match idx.into_iter().max() {
            Some(i) if i >= MAX_ID_BLOCK => {
                Err(format!("{what} id {i} past the action table's 64"))
            }
            _ => Ok(()),
        };
        ids(r.units.values().map(|e| e.idx).collect(), "unit")?;
        ids(
            r.improvements.values().map(|e| e.idx).collect(),
            "improvement",
        )?;
        ids(r.techs.values().map(|e| e.idx).collect(), "tech")?;
        let tribes = Section::build(r.tribes, |e| e.idx, "tribes")?;
        // tribes 0 and 1 (none, Nature) are not playable
        let tribe_colors = tribes
            .by_id
            .iter()
            .flatten()
            .filter(|(_, d)| d.idx >= 2)
            .map(|(_, d)| d.color as i32)
            .collect();
        Ok(Tables {
            text,
            source,
            tribes,
            techs: Section::build(r.techs, |e| e.idx, "techs")?,
            units: Section::build(r.units, |e| e.idx, "units")?,
            improvements: Section::build(r.improvements, |e| e.idx, "improvements")?,
            terrain: Section::build(r.terrain, |e| e.idx, "terrain")?,
            resources: Section::build(r.resources, |e| e.idx, "resources")?,
            tasks: Section::build(r.tasks, |e| e.idx, "tasks")?,
            embassy_income: r.embassy_income,
            tribe_colors,
            skin_languages: r
                .skin_languages
                .into_iter()
                .map(|s| (s.skin, s.language))
                .collect(),
            routers: Default::default(),
            tech_trees: Default::default(),
            tech_movements: Default::default(),
            requirements: Default::default(),
            resource_terrains: Default::default(),
        })
    }

    /// Each skin's naming language by skin id, for the skins the catalog gives one.
    pub fn skin_languages(&self) -> &[(i64, String)] {
        &self.skin_languages
    }

    pub(crate) fn requirements(&self, kind: usize) -> &Requirements {
        let all = self.requirements.get_or_init(|| {
            let names = |list: &[Requirement]| -> Vec<Names> {
                list.iter()
                    .map(|entry| {
                        let id = |name: &Option<String>,
                                  section: &dyn Fn(&str) -> Result<usize, String>| {
                            name.as_deref().map(|n| section(n).map(|x| x as i32))
                        };
                        Names {
                            resource: id(&entry.resource, &|n| self.resources.id(n)),
                            terrain: id(&entry.terrain, &|n| self.terrain.id(n)),
                            improvement: id(&entry.improvement, &|n| self.improvements.id(n)),
                            not_improvement: id(&entry.not_improvement, &|n| {
                                self.improvements.id(n)
                            }),
                            effect: entry.effect.clone(),
                            positive: entry.resource.is_some()
                                || entry.terrain.is_some()
                                || entry.improvement.is_some(),
                        }
                    })
                    .collect()
            };
            (0..self.improvements.len())
                .map(|kind| match self.improvements.get(kind) {
                    Some(d) => Requirements {
                        terrain: names(&d.terrain_requirements),
                        adjacency: names(&d.adjacency_requirements),
                        creates: names(&d.creates),
                    },
                    None => Requirements::default(),
                })
                .collect()
        });
        &all[kind]
    }

    /// The tech tree of tribe `tribe`, its overrides resolved.
    pub(crate) fn tribe_tree(&self, tribe: i32) -> Result<&TechTree, String> {
        usize::try_from(tribe)
            .ok()
            .filter(|&k| self.tribes.get(k).is_some())
            .and_then(|k| self.trees().get(k))
            .ok_or_else(|| "unknown tribe".into())
    }

    fn trees(&self) -> &[TechTree] {
        self.tech_trees.get_or_init(|| {
            let none = BTreeMap::new();
            (0..self.tribes.len())
                .map(|k| match self.tribes.get(k) {
                    Some(x) => self.build_tree(
                        &x.tech_overrides,
                        &x.unit_overrides,
                        &x.improvement_overrides,
                    ),
                    None => self.build_tree(&none, &none, &none),
                })
                .collect()
        })
    }

    /// Unit kind `kind` as tribe `tribe` gets it.
    pub(crate) fn tribe_unit(&self, tribe: i32, kind: i32) -> Result<i32, String> {
        let overrides = &self
            .tribes
            .get(tribe as usize)
            .ok_or("unknown tribe")?
            .unit_overrides;
        let name = self.units.name(kind as usize)?;
        let id = resolve(&self.units, overrides, name)?.ok_or("unit override resolved null")?;
        Ok(id as i32)
    }

    fn build_tree(
        &self,
        tech_overrides: &BTreeMap<String, String>,
        unit_overrides: &BTreeMap<String, String>,
        improvement_overrides: &BTreeMap<String, String>,
    ) -> TechTree {
        let tech = |name: &str| -> Result<usize, String> {
            resolve(&self.techs, tech_overrides, name)?
                .ok_or_else(|| "tech override resolved null".into())
        };
        let ids = 0..self.techs.len();
        let children: Vec<Result<Vec<usize>, String>> = ids
            .clone()
            .map(|id| {
                let data = self.techs.get(id).ok_or("unknown tech")?;
                data.tech_unlocks.iter().map(|c| tech(c)).collect()
            })
            .collect();
        let improvements: Vec<Result<Vec<usize>, String>> = ids
            .clone()
            .map(|id| {
                let data = self.techs.get(id).ok_or("unknown tech")?;
                let mut out = Vec::new();
                for name in &data.improvement_unlocks {
                    // a name with no entry is a null reference, which skips
                    if self.improvements.id_of.contains_key(name) {
                        out.extend(resolve(&self.improvements, improvement_overrides, name)?);
                    }
                }
                Ok(out)
            })
            .collect();
        let units = ids
            .clone()
            .map(|id| {
                let data = self.techs.get(id).ok_or("unknown tech")?;
                data.unit_unlocks
                    .iter()
                    .map(|n| {
                        resolve(&self.units, unit_overrides, n)?
                            .ok_or_else(|| "unit override resolved null".into())
                    })
                    .collect()
            })
            .collect();
        let reveals = improvements
            .iter()
            .map(|list| {
                let mut kinds = 0u32;
                for &id in list.as_deref().map_err(Clone::clone)? {
                    let d = self.improvements.get(id).unwrap();
                    if d.hidden {
                        continue;
                    }
                    for r in &d.terrain_requirements {
                        let kind = r
                            .resource
                            .as_deref()
                            .and_then(|n| self.resources.id(n).ok());
                        match kind {
                            Some(k) if k < 32 => kinds |= 1 << k,
                            Some(_) => return Err("resource id past the mask".into()),
                            None => {}
                        }
                    }
                }
                Ok(kinds)
            })
            .collect();
        let order = (|| {
            let mut pending = std::collections::VecDeque::from([0usize]);
            let mut seen = Vec::new();
            for _ in 0..200 {
                let Some(raw) = pending.pop_front() else {
                    break;
                };
                let id = tech(self.techs.name(raw)?)?;
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                for c in &self.techs.get(id).ok_or("unknown tech")?.tech_unlocks {
                    pending.push_back(self.techs.id(c)?);
                }
            }
            Ok(seen)
        })();
        TechTree {
            order,
            children,
            reveals,
            units,
            improvements,
        }
    }

    /// The terrains the techs in `techs` let a player move on, as a mask by terrain id.
    pub(crate) fn movements(&self, techs: &[i32]) -> Result<u32, String> {
        const WATER: i32 = 1;
        const OCEAN: i32 = 2;
        let all = self.tech_movements.get_or_init(|| {
            (0..self.techs.len())
                .map(|k| {
                    let mut terrains = 0u32;
                    for name in &self.techs.get(k).ok_or("unknown tech")?.movement_unlocks {
                        match self.terrain.id(name)? {
                            id if id < 32 => terrains |= 1 << id,
                            _ => return Err("terrain id past the mask".into()),
                        }
                    }
                    Ok(terrains)
                })
                .collect()
        });
        let mut terrains = 0;
        for &k in techs {
            terrains |= all
                .get(k as usize)
                .ok_or("unknown tech")?
                .as_ref()
                .map_err(Clone::clone)?;
        }
        if terrains & terrain_bit(OCEAN) != 0 {
            terrains |= terrain_bit(WATER);
        }
        Ok(terrains)
    }

    /// The terrain ids resource `kind` may sit on (empty: any), resolved once per catalog.
    pub(crate) fn resource_terrains(&self, kind: i32) -> Result<&[i32], String> {
        let all = self.resource_terrains.get_or_init(|| {
            (0..self.resources.len())
                .map(|id| {
                    let data = self.resources.get(id).ok_or("unknown resource")?;
                    let names = data.resource_terrain_requirements.iter();
                    names
                        .map(|n| self.terrain.id(n).map(|x| x as i32))
                        .collect()
                })
                .collect()
        });
        match all.get(kind as usize) {
            Some(list) => list.as_deref().map_err(Clone::clone),
            None => Err("unknown resource".into()),
        }
    }

    /// The player abilities the techs in `techs` unlock, as a mask for `unlocks`.
    pub(crate) fn abilities(&self, techs: &[i32]) -> u128 {
        techs
            .iter()
            .filter_map(|&k| self.techs.get(k as usize))
            .fold(0, |mask, tech| mask | tech.ability_unlocks.mask)
    }
}

/// A rule file in the engine's format: its rules and name. The format must be `FORMAT`.
fn read_own(text: &str) -> Result<(Rules, String), String> {
    let Strict(serde_json::Value::Object(mut v)) =
        serde_json::from_str(text).map_err(|e| format!("rules: {e}"))?
    else {
        return Err("rules: not an object".into());
    };
    let mut key = |k: &str| match v.remove(k) {
        Some(serde_json::Value::String(s)) => Ok(s),
        _ => Err(format!("rules: no {k} string")),
    };
    let (format, name) = (key("format")?, key("name")?);
    if format != FORMAT {
        return Err(format!("rules: format {format}, not {FORMAT}"));
    }
    let rules =
        serde_json::from_value(serde_json::Value::Object(v)).map_err(|e| format!("rules: {e}"))?;
    Ok((rules, name))
}

/// JSON value parsed with a duplicate-key check: a key repeated with an equal value is kept once,
/// with a different value it is an error.
pub(crate) struct Strict(pub serde_json::Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, SeqAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_bool<E>(self, b: bool) -> Result<Strict, E> {
                Ok(Strict(serde_json::Value::Bool(b)))
            }
            fn visit_i64<E>(self, x: i64) -> Result<Strict, E> {
                Ok(Strict(serde_json::Value::from(x)))
            }
            fn visit_u64<E>(self, x: u64) -> Result<Strict, E> {
                Ok(Strict(serde_json::Value::from(x)))
            }
            fn visit_f64<E: Error>(self, x: f64) -> Result<Strict, E> {
                serde_json::Number::from_f64(x)
                    .map(|n| Strict(serde_json::Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E>(self, x: &str) -> Result<Strict, E> {
                Ok(Strict(serde_json::Value::from(x)))
            }
            fn visit_string<E>(self, x: String) -> Result<Strict, E> {
                Ok(Strict(serde_json::Value::String(x)))
            }
            fn visit_unit<E>(self) -> Result<Strict, E> {
                Ok(Strict(serde_json::Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut out = Vec::new();
                while let Some(Strict(x)) = a.next_element()? {
                    out.push(x);
                }
                Ok(Strict(serde_json::Value::Array(out)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some((k, Strict(x))) = a.next_entry::<String, Strict>()? {
                    match out.get(&k) {
                        Some(old) if *old != x => {
                            return Err(A::Error::custom(format!(
                                "key {k} repeated with different values"
                            )))
                        }
                        Some(_) => {}
                        None => {
                            out.insert(k, x);
                        }
                    }
                }
                Ok(Strict(serde_json::Value::Object(out)))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::Strict;

    #[test]
    fn repeated_keys_must_agree() {
        assert!(serde_json::from_str::<Strict>(r#"{"a": {"cost": 2, "cost": 2}}"#).is_ok());
        assert!(serde_json::from_str::<Strict>(r#"{"a": {"cost": 2, "cost": 3}}"#).is_err());
    }

    #[test]
    fn ability_bits_match_the_list() {
        let a: super::Abilities = serde_json::from_str(r#"["scout", "newability"]"#).unwrap();
        assert!(a.has("scout") && a.has("newability"));
        assert!(!a.has("dash") && !a.has("other"));
    }

    /// The ability names the engine's source passes to `unlocks` and `player_ability` (or to a
    /// local `ability` closure over `unlocks`), as written: the first string literal inside each
    /// call.
    fn player_ability_names_in_source() -> std::collections::BTreeSet<String> {
        fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    files(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut paths = Vec::new();
        files(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut paths,
        );
        let mut names = std::collections::BTreeSet::new();
        for path in paths {
            let text = std::fs::read_to_string(&path).unwrap();
            for call in ["unlocks(", "player_ability(", "ability("] {
                for (at, _) in text.match_indices(call) {
                    // the name alone (not the end of a longer identifier)
                    let before = text[..at].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    // the call's own text, up to its closing parenthesis
                    let rest = &text[at + call.len()..];
                    let args = &rest[..rest.find(')').unwrap_or(rest.len())];
                    if let Some(open) = args.find('"') {
                        if let Some(len) = args[open + 1..].find('"') {
                            let name = &args[open + 1..open + 1 + len];
                            if name.chars().all(|c| c.is_ascii_lowercase()) && !name.is_empty() {
                                names.insert(name.to_owned());
                            }
                        }
                    }
                }
            }
        }
        names
    }

    /// `unlocks` reads a mask, which holds only names with a bit: every player ability the
    /// rules ask about must have one. The names are taken from the source, so a new call with
    /// a name without a bit fails here.
    #[test]
    fn player_abilities_the_rules_ask_about_have_bits() {
        let names = player_ability_names_in_source();
        // the scan finds the calls known when it was written
        for known in [
            "capitalvision",
            "decompose",
            "destroy",
            "literacy",
            "treehouse",
        ] {
            assert!(names.contains(known), "{known} not found: {names:?}");
        }
        for name in &names {
            let listed: super::Abilities = serde_json::from_str(&format!(r#"["{name}"]"#)).unwrap();
            assert!(
                super::unlocks(listed.mask, name),
                "{name} has no ability bit"
            );
            assert!(!super::unlocks(0, name), "{name}");
        }
    }

    /// S3: rule data with a unit, improvement or tech id past the action table's blocks is
    /// refused at load; the rule data in use loads.
    #[test]
    fn ids_past_the_action_table_are_refused() {
        let text = include_str!("../rules/rules-124.json");
        assert!(super::Tables::compile(text).is_ok());
        for section in ["units", "improvements", "techs"] {
            let mut c: serde_json::Value = serde_json::from_str(text).unwrap();
            let first = c[section]
                .as_object_mut()
                .unwrap()
                .values_mut()
                .next()
                .unwrap();
            first["id"] = serde_json::json!(super::MAX_ID_BLOCK);
            let e = super::Tables::compile(&c.to_string()).err();
            assert!(
                e.as_deref()
                    .is_some_and(|e| e.contains("past the action table")),
                "{section}: {e:?}"
            );
        }
    }
}
