//! The rule data as the constant block the kernel sources read (`kernel/tables.h`), built
//! from the Rust engine's `Tables`. Names are resolved to ids here, once per batch.
//!
//! The engine keeps its own resolved forms (the tech tree, routers, requirement lists)
//! private, so this file derives what it needs again from the catalog sections, following
//! `engine-fast/src/tables.rs`; the per-decision comparisons with the engine are what check
//! the result. A catalog the block cannot hold, or one whose derived tables the engine would
//! report an error for, is refused here rather than carried as a per-game error.

use engine_fast::tables::Tables;

/// Ability names in the bit order of `kernel/abilities.h`.
pub const ABILITY_NAMES: [&str; 78] = [
    "algae",
    "alienclimate",
    "amphibious",
    "attract",
    "autoflood",
    "autofreeze",
    "bridge",
    "bubble",
    "capitalvision",
    "carry",
    "consumed",
    "convert",
    "creep",
    "dash",
    "decompose",
    "destroy",
    "disband",
    "discrete",
    "doubleattack",
    "drench",
    "eat",
    "embark",
    "embassy",
    "escape",
    "explode",
    "flood",
    "flooded",
    "fly",
    "fortify",
    "freelance",
    "freeze",
    "freezearea",
    "glide",
    "grow",
    "heal",
    "hide",
    "independent",
    "infiltrate",
    "land",
    "limited",
    "literacy",
    "manual",
    "microbes",
    "network",
    "patina",
    "peacetreaty",
    "persist",
    "poison",
    "poisonresist",
    "pontoon",
    "rainbowvision",
    "scout",
    "simple",
    "skate",
    "sneak",
    "splash",
    "spread",
    "static",
    "stiff",
    "stomp",
    "surprise",
    "swarm",
    "swim",
    "tentacles",
    "unique",
    "water",
    "autoheal",
    "boost",
    "discipline",
    "doubled",
    "fill",
    "navigate",
    "picky",
    "protect",
    "slow",
    "treehouse",
    "newability",
    "other",
];

pub const MAX_TECHS: usize = 64;
pub const MAX_TRIBES: usize = 32;
pub const MAX_NAME_PIECES: usize = 40;
pub const MAX_PIECE_UNITS: usize = 6;
pub const MAX_LANGUAGES: usize = 40;
pub const MAX_SKIN_LANGUAGES: usize = 8;
pub const MAX_IMPROVEMENTS: usize = 64;
pub const MAX_UNIT_KINDS: usize = 64;
pub const MAX_CHILDREN: usize = 8;
pub const MAX_TASK_UNLOCKS: usize = 4;
pub const MAX_UNLOCKS: usize = 12;
pub const TASK_KIND_COUNT: usize = 10;
pub const MAX_TERRAINS: usize = 16;
pub const MAX_RESOURCES: usize = 16;

pub const TRIBE_KNOWN: i32 = 1;
pub const TRIBE_RAINBOWVISION: i32 = 2;
pub const MAX_TREES: usize = 12;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EfMask {
    pub lo: u64,
    pub hi: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfUnitData {
    pub known: i32,
    pub cost: i32,
    pub health: i32,
    pub defence: i32,
    pub movement: i32,
    pub weapon: i32,
    pub range: i32,
    pub attack: i32,
    pub promotion_limit: i32,
    pub hidden: i32,
    pub has_growth_rate: i32,
    pub growth_rate: i32,
    pub has_movement_terrain: i32,
    pub movement_terrain: u32,
    pub movement_terrain_error: i32,
    pub upgrades_from: i32,
    pub abilities: EfMask,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfTechData {
    pub known: i32,
    pub cost: i32,
    pub task_unlocks_len: i32,
    pub task_unlocks_error: i32,
    pub task_unlocks: [i32; MAX_TASK_UNLOCKS],
    pub movements: u32,
    pub movements_error: i32,
    pub defence_terrains: u32,
    pub defence_value: [i32; MAX_TERRAINS],
    pub pad: i32,
    pub abilities: EfMask,
}

/// kernel/tables.h EfTechLinks: what a tech unlocks for one tribe, its overrides resolved
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EfTechLinks {
    pub children_len: i32,
    pub children_error: i32,
    pub children: [i32; MAX_CHILDREN],
    pub reveals: u32,
    pub reveals_error: i32,
    pub embark_unlock: i32,
    pub embark_unlock_error: i32,
    pub units_len: i32,
    pub units_error: i32,
    pub units: [i32; MAX_UNLOCKS],
    pub improvements_len: i32,
    pub improvements_error: i32,
    pub improvements: [i32; MAX_UNLOCKS],
}

/// kernel/tables.h EfTechTree: one tribe's tech tree
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfTechTree {
    pub order_len: i32,
    pub order_error: i32,
    pub order: [i32; MAX_TECHS],
    pub links: [EfTechLinks; MAX_TECHS],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfImprovementData {
    pub known: i32,
    pub hidden: i32,
    pub has_work: i32,
    pub work: i32,
    pub has_max_level: i32,
    pub max_level: i32,
    pub has_growth_rate: i32,
    pub growth_rate: i32,
    pub field_requirement: i32,
    pub named: i32,
    pub growth_score: i32,
    pub adjacency_len: i32,
    pub adjacency: [EfAdjacency; MAX_ADJACENCY],
    pub router_terrain: u32,
    pub router_range: i32,
    pub router_connects: i32,
    pub router_error: i32,
    pub has_cost: i32,
    pub cost: i32,
    pub has_border_size: i32,
    pub border_size: i32,
    pub has_routes: i32,
    pub reward_currency: i32,
    pub reward_population: i32,
    pub reward_score: i32,
    pub creates_len: i32,
    pub creates: [EfCreates; MAX_CREATES],
    pub growth_len: i32,
    pub growth_population: [i32; MAX_GROWTH],
    pub growth_score_each: [i32; MAX_GROWTH],
    pub requirement_len: i32,
    pub adjacency_requirement_len: i32,
    pub requirements: [EfNames; MAX_REQUIREMENTS],
    pub adjacency_requirements: [EfNames; MAX_REQUIREMENTS],
    pub abilities: EfMask,
}

pub const MAX_ADJACENCY: usize = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EfAdjacency {
    pub improvement: i32,
    pub improvement_error: i32,
    pub resource: i32,
    pub resource_error: i32,
}

pub const NAMED_LIGHTHOUSE: i32 = 1;
pub const NAMED_ICEBANK: i32 = 2;
pub const NAMED_BRIDGE: i32 = 3;
pub const NAMED_FUNGI: i32 = 4;
pub const MAX_CREATES: usize = 8;
pub const MAX_GROWTH: usize = 8;
pub const MAX_REQUIREMENTS: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EfCreates {
    pub unit: i32,
    pub unit_error: i32,
    pub effect: i32,
    pub effect_pad: i32,
    pub terrain: i32,
    pub terrain_error: i32,
    pub resource: i32,
    pub resource_error: i32,
    pub effect_kind: i32,
    pub effect_kind_error: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EfNames {
    pub resource: i32,
    pub resource_error: i32,
    pub terrain: i32,
    pub terrain_error: i32,
    pub improvement: i32,
    pub improvement_error: i32,
    pub not_improvement: i32,
    pub not_improvement_error: i32,
    pub positive: i32,
    pub pad: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfTribeData {
    pub flags: i32,
    pub language: i32,
    pub starting_len: i32,
    pub starting_error: i32,
    pub starting: [i32; MAX_UNLOCKS],
    pub tree: i32,
    pub pad: i32,
    pub abilities: EfMask,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfLanguage {
    pub piece_count: i32,
    pub pad: i32,
    pub piece_len: [i32; MAX_NAME_PIECES],
    pub pieces: [[i32; MAX_PIECE_UNITS]; MAX_NAME_PIECES],
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EfTables {
    pub tech_count: i32,
    pub tribe_count: i32,
    pub improvement_count: i32,
    pub unit_count: i32,
    pub tree_count: i32,
    pub tree_pad: i32,
    pub has_embassy_income: i32,
    pub embassy_income: i32,
    pub terrain_count: i32,
    pub language_count: i32,
    /// device addresses of the searches' per-thread scratch (kernel/device.cu fills its own
    /// copy; 0 here)
    pub scratch: u64,
    pub scratch_size: u64,
    pub skin_language_count: i32,
    pub skin_language_pad: i32,
    pub skin_language_skin: [i64; MAX_SKIN_LANGUAGES],
    pub skin_language: [i32; MAX_SKIN_LANGUAGES],
    pub spores: i32,
    pub spores_error: i32,
    pub ruin: i32,
    pub ruin_error: i32,
    pub algae: i32,
    pub algae_error: i32,
    pub algae_spawn: i32,
    pub algae_spawn_error: i32,
    pub game: i32,
    pub game_error: i32,
    pub segment: i32,
    pub segment_error: i32,
    pub centipede: i32,
    pub centipede_error: i32,
    pub resource_list_len: i32,
    pub resource_pad: i32,
    pub resource_list: [i32; MAX_RESOURCES],
    pub resource_terrains: [i32; MAX_RESOURCES],
    pub resource_terrains_error: [i32; MAX_RESOURCES],
    pub fresh_continent: crate::state::EfContinent,
    pub tribe_continent_known: [i32; MAX_TRIBES],
    pub tribe_continent_error: [i32; MAX_TRIBES],
    pub tribe_continent: [crate::state::EfContinent; MAX_TRIBES],
    pub task_unlocks_len: [i32; TASK_KIND_COUNT],
    pub task_unlocks_error: [i32; TASK_KIND_COUNT],
    pub task_unlocks: [[i32; MAX_UNLOCKS]; TASK_KIND_COUNT],
    pub terrain_known: [i32; MAX_TERRAINS],
    pub techs: [EfTechData; MAX_TECHS],
    pub tribes: [EfTribeData; MAX_TRIBES],
    pub improvements: [EfImprovementData; MAX_IMPROVEMENTS],
    pub units: [EfUnitData; MAX_UNIT_KINDS],
    pub languages: [EfLanguage; MAX_LANGUAGES],
    pub trees: [EfTechTree; MAX_TREES],
    pub unit_of: [[i16; MAX_UNIT_KINDS]; MAX_TRIBES],
}

/// The device tables for a catalog, with the reasons for the entries the engine's own tables
/// hold as errors (the catalog names something it does not define); a kernel that reads such
/// an entry stops with the table-error code and the reason's number.
pub struct DeviceTables {
    pub block: Box<EfTables>,
    pub reasons: Vec<String>,
}

/// The kernel's code for "the table entry is an error" (`kernel/errors.h` EF_ERR_TABLE).
pub fn err_table() -> i32 {
    // SAFETY: no arguments, no side effects.
    unsafe { ef_twin_err_table() }
}

/// Add a naming language (its syllables split at commas) and return its slot.
fn language(d: &mut EfTables, text: &str) -> Result<i32, String> {
    let slot = d.language_count as usize;
    d.language_count = fits(slot + 1, MAX_LANGUAGES, "languages")?;
    let l = &mut d.languages[slot];
    let pieces: Vec<Vec<u16>> = text
        .split(',')
        .map(|p| p.encode_utf16().collect())
        .collect();
    l.piece_count = fits(pieces.len(), MAX_NAME_PIECES, "language pieces")?;
    for (k, piece) in pieces.iter().enumerate() {
        l.piece_len[k] = fits(piece.len(), MAX_PIECE_UNITS, "language piece units")?;
        for (unit, u) in l.pieces[k].iter_mut().zip(piece) {
            *unit = i32::from(*u);
        }
    }
    Ok(slot as i32)
}

/// Names resolved to ids in order, failing on the first the catalog lacks.
fn unlocks(
    names: &[String],
    id: impl Fn(&str) -> Result<usize, String>,
) -> Result<Vec<i32>, String> {
    names.iter().map(|n| id(n).map(|k| k as i32)).collect()
}

/// tables.rs requirements: one entry's names (EF_NONE where absent, the reason kept where
/// the catalog lacks the name)
fn names(t: &Tables, entry: &serde_json::Value, keep: &mut impl FnMut(String) -> i32) -> EfNames {
    let mut n = EfNames {
        resource: -1,
        terrain: -1,
        improvement: -1,
        not_improvement: -1,
        positive: i32::from(
            ["resource", "terrain", "improvement"]
                .iter()
                .any(|k| entry.get(*k).is_some()),
        ),
        ..EfNames::default()
    };
    let mut id = |key: &str,
                  section: &dyn Fn(&str) -> Result<usize, String>,
                  value: &mut i32,
                  error: &mut i32| {
        if let Some(name) = entry.get(key).and_then(serde_json::Value::as_str) {
            match section(name) {
                Ok(k) => *value = k as i32,
                Err(reason) => *error = keep(reason),
            }
        }
    };
    id(
        "resource",
        &|x| t.resources.id(x),
        &mut n.resource,
        &mut n.resource_error,
    );
    id(
        "terrain",
        &|x| t.terrain.id(x),
        &mut n.terrain,
        &mut n.terrain_error,
    );
    id(
        "improvement",
        &|x| t.improvements.id(x),
        &mut n.improvement,
        &mut n.improvement_error,
    );
    id(
        "notImprovement",
        &|x| t.improvements.id(x),
        &mut n.not_improvement,
        &mut n.not_improvement_error,
    );
    n
}

fn fits(count: usize, max: usize, what: &str) -> Result<i32, String> {
    if count > max {
        return Err(format!("{count} {what}: the device tables hold {max}"));
    }
    Ok(count as i32)
}

/// The ability mask `has` gives for each name.
fn mask(has: impl Fn(&str) -> bool) -> EfMask {
    let mut m = EfMask::default();
    for (bit, name) in ABILITY_NAMES.iter().enumerate() {
        if has(name) {
            if bit < 64 {
                m.lo |= 1 << bit;
            } else {
                m.hi |= 1 << (bit - 64);
            }
        }
    }
    m
}

/// tables.rs resolve: `name` as a tribe with
/// `overrides` gets it; None when the override names no catalog entry.
fn resolve<T>(
    section: &engine_fast::tables::Section<T>,
    overrides: &std::collections::BTreeMap<String, String>,
    name: &str,
) -> Result<Option<usize>, String> {
    let raw = section.id(name)?;
    Ok(match overrides.get(name) {
        None => Some(raw),
        Some(target) => section.id_of.get(target).copied(),
    })
}

/// tables.rs build_tree: one tribe's tree with its tech, unit and improvement overrides
/// resolved (`overrides` in that order), each entry's error kept as a table error.
fn tree(
    t: &Tables,
    out: &mut EfTechTree,
    overrides: [&std::collections::BTreeMap<String, String>; 3],
    keep: &mut impl FnMut(String) -> i32,
) -> Result<(), String> {
    let [tech_ov, unit_ov, improvement_ov] = overrides;
    let tech = |name: &str| -> Result<usize, String> {
        resolve(&t.techs, tech_ov, name)?.ok_or_else(|| "tech override resolved null".into())
    };
    let mut improvement_lists: Vec<Result<Vec<usize>, String>> = Vec::new();
    for id in 0..t.techs.len() {
        let links = &mut out.links[id];
        let Some(data) = t.techs.get(id) else {
            for e in [
                &mut links.children_error,
                &mut links.reveals_error,
                &mut links.embark_unlock_error,
                &mut links.units_error,
                &mut links.improvements_error,
            ] {
                *e = keep("unknown tech".into());
            }
            improvement_lists.push(Err("unknown tech".into()));
            continue;
        };
        match data
            .tech_unlocks
            .iter()
            .map(|c| tech(c))
            .collect::<Result<Vec<usize>, String>>()
        {
            Ok(children) => {
                links.children_len =
                    fits(children.len(), MAX_CHILDREN, "techs unlocked by one tech")?;
                for (slot, c) in links.children.iter_mut().zip(children) {
                    *slot = c as i32;
                }
            }
            Err(reason) => links.children_error = keep(reason),
        }
        // a name with no entry is a null reference, which skips
        let improvements = (|| -> Result<Vec<usize>, String> {
            let mut out = Vec::new();
            for name in &data.improvement_unlocks {
                if t.improvements.id_of.contains_key(name) {
                    out.extend(resolve(&t.improvements, improvement_ov, name)?);
                }
            }
            Ok(out)
        })();
        match &improvements {
            Ok(ids) => {
                links.improvements_len =
                    fits(ids.len(), MAX_UNLOCKS, "improvements unlocked by one tech")?;
                for (slot, &k) in links.improvements.iter_mut().zip(ids) {
                    *slot = k as i32;
                }
                // reveals: the resource kinds its unlocked, non-hidden improvements need
                let reveals = || -> Result<u32, String> {
                    let mut kinds = 0u32;
                    for &k in ids {
                        let d = t.improvements.get(k).ok_or("unknown improvement")?;
                        if d.hidden {
                            continue;
                        }
                        for r in &d.terrain_requirements {
                            match r["resource"].as_str().and_then(|n| t.resources.id(n).ok()) {
                                Some(k) if k < 32 => kinds |= 1 << k,
                                Some(_) => return Err("resource id past the mask".into()),
                                None => {}
                            }
                        }
                    }
                    Ok(kinds)
                };
                match reveals() {
                    Ok(k) => links.reveals = k,
                    Err(reason) => links.reveals_error = keep(reason),
                }
                // movement.rs mover_context / routes.rs can_embark
                links.embark_unlock = i32::from(ids.iter().any(|&k| {
                    t.improvements
                        .get(k)
                        .is_some_and(|d| !d.hidden && d.improvement_abilities.has("embark"))
                }));
            }
            Err(reason) => {
                links.improvements_error = keep(reason.clone());
                links.reveals_error = keep(reason.clone());
                links.embark_unlock_error = keep(reason.clone());
            }
        }
        improvement_lists.push(improvements);
        let units = data
            .unit_unlocks
            .iter()
            .map(|n| {
                resolve(&t.units, unit_ov, n)?.ok_or_else(|| "unit override resolved null".into())
            })
            .collect::<Result<Vec<usize>, String>>();
        match units {
            Ok(ids) => {
                links.units_len = fits(ids.len(), MAX_UNLOCKS, "units unlocked by one tech")?;
                for (slot, k) in links.units.iter_mut().zip(ids) {
                    *slot = k as i32;
                }
            }
            Err(reason) => links.units_error = keep(reason),
        }
    }
    // Tribe tech order: breadth-first from tech 0, each queued tech resolved before the
    // first-visit check, at most 200 dequeues
    let order = (|| -> Result<Vec<usize>, String> {
        let mut pending = std::collections::VecDeque::from([0usize]);
        let mut seen = Vec::new();
        for _ in 0..200 {
            let Some(raw) = pending.pop_front() else {
                break;
            };
            let id = tech(t.techs.name(raw)?)?;
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            for c in &t.techs.get(id).ok_or("unknown tech")?.tech_unlocks {
                pending.push_back(t.techs.id(c)?);
            }
        }
        Ok(seen)
    })();
    match order {
        Ok(ids) => {
            out.order_len = fits(ids.len(), MAX_TECHS, "techs in the tree")?;
            for (slot, k) in out.order.iter_mut().zip(ids) {
                *slot = k as i32;
            }
        }
        Err(reason) => out.order_error = keep(reason),
    }
    Ok(())
}

impl DeviceTables {
    /// The device tables for `t`. Fails when the catalog is larger than the block.
    pub fn new(t: &Tables) -> Result<DeviceTables, String> {
        // SAFETY: the block is integers only, for which all-zero bytes are valid.
        let mut d: Box<EfTables> = unsafe { Box::new_zeroed().assume_init() };
        let mut reasons: Vec<String> = Vec::new();
        // the number (plus one) under which a reason is kept
        let mut keep = |reason: String| {
            reasons.push(reason);
            reasons.len() as i32
        };
        d.tech_count = fits(t.techs.len(), MAX_TECHS, "techs")?;
        d.tribe_count = fits(t.tribes.len(), MAX_TRIBES, "tribes")?;
        d.improvement_count = fits(t.improvements.len(), MAX_IMPROVEMENTS, "improvements")?;
        d.unit_count = fits(t.units.len(), MAX_UNIT_KINDS, "unit kinds")?;
        d.terrain_count = fits(t.terrain.len(), MAX_TERRAINS, "terrains")?;
        // capture.rs syllables: slot 0 the default language, then the skins' and the tribes'
        language(&mut d, &engine_fast::capture::DEFAULT_SYLLABLES.join(","))?;
        let skins = t.skin_languages();
        d.skin_language_count = fits(skins.len(), MAX_SKIN_LANGUAGES, "skin languages")?;
        for (k, (skin, text)) in skins.iter().enumerate() {
            d.skin_language_skin[k] = *skin;
            d.skin_language[k] = language(&mut d, text)?;
        }
        for id in 0..t.terrain.len() {
            d.terrain_known[id] = i32::from(t.terrain.name(id).is_ok());
        }
        match t
            .diplomacy
            .get("embassyIncome")
            .and_then(serde_json::Value::as_i64)
        {
            Some(v) => {
                d.has_embassy_income = 1;
                d.embassy_income =
                    i32::try_from(v).map_err(|_| "embassy income outside 32 bits")?;
            }
            None => d.has_embassy_income = 0,
        }
        for id in 0..t.techs.len() {
            let entry = &mut d.techs[id];
            let Some(tech) = t.techs.get(id) else {
                // tables.rs tech_tree: every id's entry is "unknown tech" where the catalog has none
                entry.movements_error = keep("unknown tech".into());
                continue;
            };
            entry.known = 1;
            entry.cost = tech.cost;
            entry.abilities = mask(|n| tech.ability_unlocks.has(n));
            // tables.rs tech_tree: movements (the terrains its movement unlocks name)
            let movements = || -> Result<u32, String> {
                let mut terrains = 0u32;
                for name in tech.movement_unlocks.keys() {
                    match t.terrain.id(name)? {
                        id if id < 32 => terrains |= 1 << id,
                        _ => return Err("terrain id past the mask".into()),
                    }
                }
                Ok(terrains)
            };
            match movements() {
                Ok(m) => entry.movements = m,
                Err(reason) => entry.movements_error = keep(reason),
            }
            // combat.rs defence_bonus: the integer entries of defence_bonus_unlocks by terrain
            for id in 0..t.terrain.len().min(MAX_TERRAINS) {
                let Ok(name) = t.terrain.name(id) else {
                    continue;
                };
                if let Some(v) = tech
                    .defence_bonus_unlocks
                    .get(name)
                    .and_then(serde_json::Value::as_i64)
                {
                    entry.defence_terrains |= 1 << id;
                    entry.defence_value[id] =
                        i32::try_from(v).unwrap_or(if v > 0 { i32::MAX } else { i32::MIN });
                }
            }
            // step.rs Research: the tasks a tech enables, by the catalog's task ids
            match tech
                .task_unlocks
                .iter()
                .map(|n| t.tasks.id(n))
                .collect::<Result<Vec<usize>, String>>()
            {
                Ok(ids) => {
                    entry.task_unlocks_len =
                        fits(ids.len(), MAX_TASK_UNLOCKS, "tasks unlocked by one tech")?;
                    for (slot, k) in entry.task_unlocks.iter_mut().zip(ids) {
                        *slot = k as i32;
                    }
                }
                Err(reason) => entry.task_unlocks_error = keep(reason),
            }
        }
        // menu.rs unlocked_improvements: a completed task's improvement unlocks
        for kind in 0..TASK_KIND_COUNT {
            let name = engine_fast::state::TaskKind(kind as u8)
                .name()
                .to_lowercase();
            let list = t.tasks.id(&name).and_then(|id| {
                let task = t.tasks.get(id).ok_or("unknown task")?;
                unlocks(&task.improvement_unlocks, |n| t.improvements.id(n))
            });
            match list {
                Ok(ids) => {
                    d.task_unlocks_len[kind] =
                        fits(ids.len(), MAX_UNLOCKS, "improvements unlocked by one task")?;
                    d.task_unlocks[kind][..ids.len()].copy_from_slice(&ids);
                }
                Err(reason) => d.task_unlocks_error[kind] = keep(reason),
            }
        }
        match t.resources.id("spores") {
            Ok(k) => d.spores = k as i32,
            Err(reason) => d.spores_error = keep(reason),
        }
        match t.units.id("segment") {
            Ok(k) => d.segment = k as i32,
            Err(reason) => d.segment_error = keep(reason),
        }
        match t.units.id("centipede") {
            Ok(k) => d.centipede = k as i32,
            Err(reason) => d.centipede_error = keep(reason),
        }
        match t.resources.id("game") {
            Ok(k) => d.game = k as i32,
            Err(reason) => d.game_error = keep(reason),
        }
        match t.improvements.id("algae") {
            Ok(k) => d.algae = k as i32,
            Err(reason) => d.algae_error = keep(reason),
        }
        match t.improvements.id("ruin") {
            Ok(k) => d.ruin = k as i32,
            Err(reason) => d.ruin_error = keep(reason),
        }
        match t.improvements.id("algaespawn") {
            Ok(k) => d.algae_spawn = k as i32,
            Err(reason) => d.algae_spawn_error = keep(reason),
        }
        // ruins.rs lost_city: the resources in id order and the terrains each needs
        for id in 0..t.resources.len() {
            let Some(r) = t.resources.get(id) else {
                continue;
            };
            let k = fits(id + 1, MAX_RESOURCES, "resources")? as usize - 1;
            d.resource_list[d.resource_list_len as usize] = id as i32;
            d.resource_list_len += 1;
            let terrains: Result<Vec<usize>, String> = r
                .resource_terrain_requirements
                .iter()
                .map(|n| t.terrain.id(n))
                .collect();
            match terrains {
                Ok(ids) => {
                    for x in ids {
                        fits(x + 1, 32, "terrain ids")?;
                        d.resource_terrains[k] |= 1 << x;
                    }
                }
                Err(reason) => d.resource_terrains_error[k] = keep(reason),
            }
        }
        use engine_fast::mapgen::candidate::{fresh_continent, tribe_continent};
        d.fresh_continent = crate::state::continent_to(&fresh_continent())?;
        for id in 0..MAX_TRIBES.min(t.tribes.len()) {
            match tribe_continent(t, id as i32) {
                Ok(Some(c)) => {
                    d.tribe_continent_known[id] = 1;
                    d.tribe_continent[id] = crate::state::continent_to(&c)?;
                }
                Ok(None) => {}
                Err(reason) => d.tribe_continent_error[id] = keep(reason),
            }
        }
        // tables.rs trees: tree 0 without overrides, one per tribe that has any
        let none = std::collections::BTreeMap::new();
        tree(t, &mut d.trees[0], [&none, &none, &none], &mut keep)?;
        d.tree_count = 1;
        for id in 0..t.tribes.len() {
            let Some(tribe) = t.tribes.get(id) else {
                continue;
            };
            d.tribes[id].abilities = mask(|n| tribe.tribe_abilities.has(n));
            if !tribe.language.is_empty() {
                d.tribes[id].language = language(&mut d, &tribe.language)?;
            }
            // reset.rs start_match: the starting technologies by id
            match unlocks(&tribe.starting_tech, |n| t.techs.id(n)) {
                Ok(ids) => {
                    d.tribes[id].starting_len =
                        fits(ids.len(), MAX_UNLOCKS, "starting technologies")?;
                    d.tribes[id].starting[..ids.len()].copy_from_slice(&ids);
                }
                Err(reason) => d.tribes[id].starting_error = keep(reason),
            }
            d.tribes[id].flags = TRIBE_KNOWN
                | if tribe.tribe_abilities.has("rainbowvision") {
                    TRIBE_RAINBOWVISION
                } else {
                    0
                };
            let overrides = [
                &tribe.tech_overrides,
                &tribe.unit_overrides,
                &tribe.improvement_overrides,
            ];
            if overrides.iter().any(|o| !o.is_empty()) {
                let slot = d.tree_count as usize;
                fits(slot + 1, MAX_TREES, "tribes with overrides")?;
                tree(t, &mut d.trees[slot], overrides, &mut keep)?;
                d.tribes[id].tree = slot as i32;
                d.tree_count += 1;
            }
            // tables.rs tribe_unit: each kind as this tribe gets it, -1 where it resolves null
            for kind in 0..t.units.len().min(MAX_UNIT_KINDS) {
                d.unit_of[id][kind] = match t.units.name(kind) {
                    Ok(name) => {
                        resolve(&t.units, &tribe.unit_overrides, name)?.map_or(-1, |k| k as i16)
                    }
                    Err(_) => -1,
                };
            }
        }
        for id in 0..t.improvements.len() {
            let Some(m) = t.improvements.get(id) else {
                continue;
            };
            let entry = &mut d.improvements[id];
            entry.known = 1;
            entry.hidden = i32::from(m.hidden);
            entry.has_work = i32::from(m.work.is_some());
            entry.work = m.work.unwrap_or(0);
            entry.field_requirement = i32::from(
                m.terrain_requirements
                    .iter()
                    .any(|r| r["terrain"] == "field"),
            );
            entry.abilities = mask(|n| m.improvement_abilities.has(n));
            // routes.rs build_router
            let router = || -> Result<u32, String> {
                let mut terrain = 0u32;
                for v in &m.routes {
                    let id = v
                        .as_str()
                        .and_then(|n| t.terrain.id(n).ok())
                        .ok_or("route terrain")?;
                    terrain |= 1u32.checked_shl(id as u32).ok_or("route terrain")?;
                }
                Ok(terrain)
            };
            match router() {
                Ok(terrain) => entry.router_terrain = terrain,
                Err(reason) => entry.router_error = keep(reason),
            }
            entry.router_range = m.range.unwrap_or(0);
            entry.router_connects = i32::from(m.improvement_abilities.has("network"));
            // economy.rs field: a missing or non-integer field counts 0, truncated to 32 bits
            let field = |v: &serde_json::Value, k: &str| v[k].as_i64().unwrap_or(0) as i32;
            entry.has_cost = i32::from(m.cost.is_some());
            entry.cost = m.cost.unwrap_or(0);
            entry.has_border_size = i32::from(m.border_size.is_some());
            entry.border_size = m.border_size.unwrap_or(0);
            entry.has_routes = i32::from(!m.routes.is_empty());
            for r in &m.rewards {
                entry.reward_currency = entry.reward_currency.wrapping_add(field(r, "currency"));
                entry.reward_population =
                    entry.reward_population.wrapping_add(field(r, "population"));
                entry.reward_score = entry.reward_score.wrapping_add(field(r, "score").max(0));
            }
            entry.creates_len = fits(m.creates.len(), MAX_CREATES, "creates entries")?;
            for (slot, e) in entry.creates.iter_mut().zip(&m.creates) {
                *slot = EfCreates {
                    unit: -1,
                    unit_error: 0,
                    effect: i32::from(e["effect"].is_string()),
                    effect_pad: 0,
                    terrain: -1,
                    terrain_error: 0,
                    resource: -1,
                    resource_error: 0,
                    effect_kind: 0,
                    effect_kind_error: 0,
                };
                // menu.rs can_build: the effect a creates entry names
                match e.get("effect").and_then(serde_json::Value::as_str) {
                    None => {}
                    Some("flooded") => slot.effect_kind = 1,
                    Some("algae") => slot.effect_kind = 4,
                    Some(other) => {
                        slot.effect_kind_error =
                            keep(format!("creates effect {other} not implemented"))
                    }
                }
                if let Some(n) = e["unit"].as_str() {
                    match t.units.id(n) {
                        Ok(k) => slot.unit = k as i32,
                        Err(reason) => slot.unit_error = keep(reason),
                    }
                }
                if let Some(n) = e["terrain"].as_str() {
                    match t.terrain.id(n) {
                        Ok(k) => slot.terrain = k as i32,
                        Err(reason) => slot.terrain_error = keep(reason),
                    }
                }
                if let Some(n) = e["resource"].as_str() {
                    match t.resources.id(n) {
                        Ok(k) => slot.resource = k as i32,
                        Err(reason) => slot.resource_error = keep(reason),
                    }
                }
            }
            entry.growth_len = fits(m.growth_rewards.len(), MAX_GROWTH, "growth rewards")?;
            for (k, r) in m.growth_rewards.iter().enumerate() {
                entry.growth_population[k] = field(r, "population");
                entry.growth_score_each[k] = field(r, "score");
            }
            entry.requirement_len = fits(
                m.terrain_requirements.len(),
                MAX_REQUIREMENTS,
                "terrain requirements",
            )?;
            for (slot, r) in entry.requirements.iter_mut().zip(&m.terrain_requirements) {
                *slot = names(t, r, &mut keep);
            }
            entry.adjacency_requirement_len = fits(
                m.adjacency_requirements.len(),
                MAX_REQUIREMENTS,
                "adjacency requirements",
            )?;
            for (slot, r) in entry
                .adjacency_requirements
                .iter_mut()
                .zip(&m.adjacency_requirements)
            {
                *slot = names(t, r, &mut keep);
            }
            entry.has_max_level = i32::from(m.max_level.is_some());
            entry.max_level = m.max_level.unwrap_or(0);
            entry.has_growth_rate = i32::from(m.growth_rate.is_some());
            entry.growth_rate = m.growth_rate.unwrap_or(0);
            entry.named = match t.improvements.name(id).unwrap_or("") {
                "lighthouse" => NAMED_LIGHTHOUSE,
                "icebank" => NAMED_ICEBANK,
                "bridge" => NAMED_BRIDGE,
                "fungi" => NAMED_FUNGI,
                _ => 0,
            };
            // economy.rs field(r, "score"): a missing or non-integer score counts 0
            entry.growth_score = m
                .growth_rewards
                .iter()
                .map(|r| r["score"].as_i64().unwrap_or(0) as i32)
                .fold(0i32, i32::wrapping_add);
            entry.adjacency_len = fits(
                m.adjacency_improvements.len(),
                MAX_ADJACENCY,
                "adjacency entries",
            )?;
            for (slot, e) in entry.adjacency.iter_mut().zip(&m.adjacency_improvements) {
                *slot = EfAdjacency {
                    improvement: -1,
                    improvement_error: 0,
                    resource: -1,
                    resource_error: 0,
                };
                if let Some(n) = e["improvement"].as_str() {
                    match t.improvements.id(n) {
                        Ok(k) => slot.improvement = k as i32,
                        Err(reason) => slot.improvement_error = keep(reason),
                    }
                }
                if let Some(n) = e["resources"].as_str() {
                    match t.resources.id(n) {
                        Ok(k) => slot.resource = k as i32,
                        Err(reason) => slot.resource_error = keep(reason),
                    }
                }
            }
        }
        for id in 0..t.units.len() {
            let Some(u) = t.units.get(id) else { continue };
            d.units[id] = EfUnitData {
                known: 1,
                cost: u.cost,
                health: u.health,
                defence: u.defence,
                movement: u.movement,
                weapon: u.weapon,
                range: u.range,
                attack: u.attack,
                promotion_limit: u.promotion_limit,
                hidden: i32::from(u.hidden),
                has_growth_rate: i32::from(u.growth_rate.is_some()),
                growth_rate: u.growth_rate.unwrap_or(0),
                abilities: mask(|n| u.unit_abilities.has(n)),
                has_movement_terrain: i32::from(!u.movement_terrain.is_empty()),
                movement_terrain: 0,
                movement_terrain_error: 0,
                // menu.rs unit_actions_with: `upgrades_from` against the unit kind's name
                upgrades_from: u
                    .upgrades_from
                    .as_deref()
                    .and_then(|n| t.units.id(n).ok())
                    .map_or(-1, |k| k as i32),
            };
            let own = || -> Result<u32, String> {
                let mut bits = 0u32;
                for n in &u.movement_terrain {
                    let id = t.terrain.id(n)? as i32;
                    bits |= if (0..32).contains(&id) { 1 << id } else { 0 };
                }
                Ok(bits)
            };
            match own() {
                Ok(bits) => d.units[id].movement_terrain = bits,
                Err(reason) => d.units[id].movement_terrain_error = keep(reason),
            }
        }
        Ok(DeviceTables { block: d, reasons })
    }

    /// The Rust engine's text for a kernel error code and its argument.
    pub fn error_text(&self, code: i32, arg: i64) -> String {
        if code == err_table() {
            return self
                .reasons
                .get(arg as usize)
                .cloned()
                .unwrap_or_else(|| "unknown table reason".into());
        }
        // SAFETY: the C side returns a pointer to a constant, zero-terminated string.
        let text = unsafe { std::ffi::CStr::from_ptr(ef_twin_error_text(code)) };
        text.to_string_lossy().replace("{}", &arg.to_string())
    }
}

extern "C" {
    fn ef_twin_tables_size() -> i32;
    fn ef_twin_err_table() -> i32;
    fn ef_twin_error_text(code: i32) -> *const std::os::raw::c_char;
    fn ef_twin_error_count() -> i32;
}

/// A kernel function's result as the C side lays it out (`kernel/errors.h` EfResult).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KernelResult {
    pub code: i32,
    pub pad: i32,
    pub arg: i64,
}

/// Every error text the kernel sources know, by code (code 0 is success).
pub fn error_texts() -> Vec<String> {
    // SAFETY: no arguments; the texts are constant, zero-terminated strings.
    let count = unsafe { ef_twin_error_count() };
    (0..count)
        .map(|code| {
            unsafe { std::ffi::CStr::from_ptr(ef_twin_error_text(code)) }
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// The C compiler's size of the tables block.
pub fn twin_size() -> usize {
    // SAFETY: no arguments, no side effects.
    unsafe { ef_twin_tables_size() as usize }
}
