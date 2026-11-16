/* tables.h - the rule data as constant integer arrays: the device form of
 * engine-fast/src/tables.rs `Tables`, filled on the host by ../src/tables.rs (one block for a
 * whole batch). Names are resolved to ids there; nothing here is looked up by text. Where
 * the engine's own table holds an error instead of a value, the entry carries the reason's
 * number plus one (0: no error) and a rule that reads it stops with EF_ERR_TABLE.
 *
 * It holds what the ported rules read so far and grows with them. */
#ifndef EF_TABLES_H
#define EF_TABLES_H

#include "ef.h"
#include "abilities.h"
#include "action.h"

#define EF_MAX_TECHS 64
#define EF_MAX_TRIBES 32
#define EF_MAX_IMPROVEMENTS 64
#define EF_MAX_UNIT_KINDS 64
#define EF_MAX_CHILDREN 8
#define EF_MAX_TASK_UNLOCKS 4
#define EF_MAX_UNLOCKS 12
#define EF_TASK_KIND_COUNT 10
#define EF_MAX_TERRAINS 16
#define EF_MAX_RESOURCES 16
#define EF_MAX_NAME_PIECES 40
#define EF_MAX_LANGUAGES 40
#define EF_MAX_SKIN_LANGUAGES 8
#define EF_MAX_PIECE_UNITS 6

/* tribe flags */
#define EF_TRIBE_KNOWN 1
#define EF_TRIBE_RAINBOWVISION 2

typedef struct {
    int32_t known, cost, health, defence, movement, weapon, range, attack, promotion_limit, hidden;
    int32_t has_growth_rate, growth_rate;
    int32_t has_movement_terrain;   /* the unit lists its own terrain */
    uint32_t movement_terrain;      /* that list, one bit per terrain id */
    int32_t movement_terrain_error;
    int32_t upgrades_from;          /* the unit kind its upgrades_from names, or EF_NONE */
    EfMask abilities;
} EfUnitData;

/* What a tech unlocks for one tribe, its overrides resolved (tables.rs build_tree):
 * techUnlocks as ids; the resource kinds its unlocked, non-hidden
 * improvements need; whether one of those has Embark; unitUnlocks and improvementUnlocks as
 * ids in catalog order. Each with the table-error number of a name the catalog lacks. */
typedef struct {
    int32_t children_len, children_error;
    int32_t children[EF_MAX_CHILDREN];
    uint32_t reveals;
    int32_t reveals_error;
    int32_t embark_unlock, embark_unlock_error;
    int32_t units_len, units_error, units[EF_MAX_UNLOCKS];
    int32_t improvements_len, improvements_error, improvements[EF_MAX_UNLOCKS];
} EfTechLinks;

/* One tribe's tech tree (tables.rs TechTree): the game's order and each tech's
 * links. Tree 0 has no overrides; a tribe with overrides has its own (EfTribeData.tree). */
#define EF_MAX_TREES 12
typedef struct {
    int32_t order_len, order_error;
    int32_t order[EF_MAX_TECHS];
    EfTechLinks links[EF_MAX_TECHS];
} EfTechTree;

typedef struct {
    int32_t known, cost;
    int32_t task_unlocks_len, task_unlocks_error;
    int32_t task_unlocks[EF_MAX_TASK_UNLOCKS];
    uint32_t movements;          /* terrain kinds its movement unlocks name, one bit per id */
    int32_t movements_error;
    /* defence_bonus_unlocks: the terrains with an integer entry (one bit per id) and its value */
    uint32_t defence_terrains;
    int32_t defence_value[EF_MAX_TERRAINS];
    int32_t pad;
    EfMask abilities;
} EfTechData;

#define EF_MAX_ADJACENCY 4

/* one adjacency_improvements entry: an improvement kind and a resource kind (EF_NONE where the
 * entry names none), each with the table-error number of a name the catalog does not define */
typedef struct {
    int32_t improvement, improvement_error, resource, resource_error;
} EfAdjacency;

/* improvement kinds the rules single out by name */
enum { EF_NAMED_NONE, EF_NAMED_LIGHTHOUSE, EF_NAMED_ICEBANK, EF_NAMED_BRIDGE, EF_NAMED_FUNGI };

#define EF_MAX_CREATES 8
#define EF_MAX_GROWTH 8
#define EF_MAX_REQUIREMENTS 8

/* one `creates` entry: the unit kind, terrain and resource it creates (EF_NONE where it names
 * none), with table errors, and whether it names a tile effect */
typedef struct {
    int32_t unit, unit_error, effect, effect_pad;
    int32_t terrain, terrain_error, resource, resource_error;
    /* menu.rs can_build: the tile effect it names (0 none, else the effect id), or the error */
    int32_t effect_kind, effect_kind_error;
} EfCreates;

/* tables.rs Names: one requirement entry; EF_NONE where it names nothing, with the table
 * error of a name the catalog does not define; positive: it names a resource, terrain or
 * improvement */
typedef struct {
    int32_t resource, resource_error, terrain, terrain_error;
    int32_t improvement, improvement_error, not_improvement, not_improvement_error;
    int32_t positive, pad;
} EfNames;

typedef struct {
    int32_t known, hidden;
    int32_t has_work, work;
    int32_t has_max_level, max_level;
    int32_t has_growth_rate, growth_rate;
    int32_t field_requirement;   /* a terrain requirement names "field" */
    int32_t named;               /* EF_NAMED_* */
    int32_t growth_score;        /* the growth rewards' scores, summed */
    int32_t adjacency_len;
    EfAdjacency adjacency[EF_MAX_ADJACENCY];
    /* routes.rs router: the route terrains (one bit per id), range and Network ability; a
     * kind with routes the catalog cannot resolve holds the error */
    uint32_t router_terrain;
    int32_t router_range, router_connects, router_error;
    int32_t has_cost, cost, has_border_size, border_size;
    int32_t has_routes;
    /* the rewards' currency, population and score (each at least 0), summed */
    int32_t reward_currency, reward_population, reward_score;
    int32_t creates_len;
    EfCreates creates[EF_MAX_CREATES];
    /* growth_rewards entries: population and score */
    int32_t growth_len;
    int32_t growth_population[EF_MAX_GROWTH], growth_score_each[EF_MAX_GROWTH];
    int32_t requirement_len, adjacency_requirement_len;
    EfNames requirements[EF_MAX_REQUIREMENTS];             /* terrain_requirements */
    EfNames adjacency_requirements[EF_MAX_REQUIREMENTS];   /* adjacency_requirements */
    EfMask abilities;
} EfImprovementData;

/* language: the slot in EfTables.languages of the tribe's language (0: the default) */
/* starting: the tribe's starting technologies (reset.rs start_match), or the table error of
 * a name the catalog lacks */
/* tree: the slot in EfTables.trees of its tech tree (0: no overrides) */
typedef struct {
    int32_t flags, language;
    int32_t starting_len, starting_error;
    int32_t starting[EF_MAX_UNLOCKS];
    int32_t tree, pad;
    EfMask abilities;
} EfTribeData;

/* a naming language: its syllables (split at commas) as UTF-16 units */
typedef struct {
    int32_t piece_count, pad;
    int32_t piece_len[EF_MAX_NAME_PIECES];
    int32_t pieces[EF_MAX_NAME_PIECES][EF_MAX_PIECE_UNITS];
} EfLanguage;

typedef struct {
    int32_t tech_count, tribe_count, improvement_count, unit_count;
    int32_t tree_count, tree_pad;
    int32_t has_embassy_income, embassy_income;
    int32_t terrain_count, language_count;
    /* scratch for the searches, one region of scratch_size bytes per thread of a launch (a
     * device address, filled by kernel/device.cu for its own copy of the tables; 0 on the
     * host, which uses thread-local copies): movement.h EF_SCRATCH_TAKE */
    uint64_t scratch, scratch_size;
    /* capture.rs syllables: the skins with a language of their own, and its slot */
    int32_t skin_language_count, skin_language_pad;
    int64_t skin_language_skin[EF_MAX_SKIN_LANGUAGES];
    int32_t skin_language[EF_MAX_SKIN_LANGUAGES];
    int32_t spores, spores_error, ruin, ruin_error; /* the ids named "spores" (resource) and "ruin" (improvement) */
    int32_t algae, algae_error, algae_spawn, algae_spawn_error; /* the improvements "algae" (rules123) and "algaespawn" */
    int32_t game, game_error;                     /* the resource named "game" */
    int32_t segment, segment_error, centipede, centipede_error; /* the unit kinds "segment" and "centipede" */
    /* ruins.rs lost_city: the catalog's resources in id order, and per resource id the terrains
     * it needs as a bit set (0: any) or the table error (Tables::resource_terrains) */
    int32_t resource_list_len, resource_pad;
    int32_t resource_list[EF_MAX_RESOURCES], resource_terrains[EF_MAX_RESOURCES], resource_terrains_error[EF_MAX_RESOURCES];
    /* candidate.rs fresh_continent, and per tribe tribe_continent (known 0: no tribe the catalog
     * has, so the fresh one; else the template or the table error) */
    EfContinent fresh_continent;
    int32_t tribe_continent_known[EF_MAX_TRIBES], tribe_continent_error[EF_MAX_TRIBES];
    EfContinent tribe_continent[EF_MAX_TRIBES];
    /* menu.rs unlocked_improvements: per task kind, the improvements its completion unlocks */
    int32_t task_unlocks_len[EF_TASK_KIND_COUNT], task_unlocks_error[EF_TASK_KIND_COUNT];
    int32_t task_unlocks[EF_TASK_KIND_COUNT][EF_MAX_UNLOCKS];
    int32_t terrain_known[EF_MAX_TERRAINS];
    EfTechData techs[EF_MAX_TECHS];
    EfTribeData tribes[EF_MAX_TRIBES];
    EfImprovementData improvements[EF_MAX_IMPROVEMENTS];
    EfUnitData units[EF_MAX_UNIT_KINDS];
    EfLanguage languages[EF_MAX_LANGUAGES];
    EfTechTree trees[EF_MAX_TREES];
    /* tables.rs tribe_unit: unit kind k as tribe r gets it, or -1 where its override names no
     * catalog entry */
    int16_t unit_of[EF_MAX_TRIBES][EF_MAX_UNIT_KINDS];
} EfTables;

/* tables.rs tribe_tree: the tech tree of tribe `tribe`, or NULL for a tribe the catalog lacks */
EF_FN const EfTechTree *ef_tribe_tree(const EfTables *t, int32_t tribe) {
    if (tribe < 0 || tribe >= EF_MAX_TRIBES || !(t->tribes[tribe].flags & EF_TRIBE_KNOWN)) return 0;
    int32_t k = t->tribes[tribe].tree;
    return k >= 0 && k < t->tree_count ? &t->trees[k] : 0;
}


/* tables.rs terrain_bit */
EF_FN uint32_t ef_terrain_bit(int32_t id) { return id >= 0 && id < 32 ? 1u << id : 0u; }

EF_FN int ef_tech_known(const EfTables *t, int32_t id) {
    return id >= 0 && id < t->tech_count && t->techs[id].known;
}
EF_FN int32_t ef_tribe_flags(const EfTables *t, int32_t id) {
    return id >= 0 && id < t->tribe_count ? t->tribes[id].flags : 0;
}
EF_FN int ef_improvement_known(const EfTables *t, int32_t id) {
    return id >= 0 && id < t->improvement_count && t->improvements[id].known;
}
EF_FN const EfUnitData *ef_unit_data(const EfTables *t, int32_t kind) {
    return kind >= 0 && kind < t->unit_count && t->units[kind].known ? &t->units[kind] : 0;
}
EF_FN const EfImprovementData *ef_improvement_data(const EfTables *t, int32_t kind) {
    return ef_improvement_known(t, kind) ? &t->improvements[kind] : 0;
}
EF_FN const EfTechData *ef_tech_data(const EfTables *t, int32_t id) {
    return ef_tech_known(t, id) ? &t->techs[id] : 0;
}
/* tables.rs Abilities::has on a unit, improvement or tribe kind; false for an unknown kind */
EF_FN int ef_unit_has(const EfTables *t, int32_t kind, int ability) {
    const EfUnitData *d = ef_unit_data(t, kind);
    return d && ef_has(d->abilities, ability);
}
EF_FN int ef_improvement_has(const EfTables *t, int32_t kind, int ability) {
    const EfImprovementData *d = ef_improvement_data(t, kind);
    return d && ef_has(d->abilities, ability);
}
EF_FN int ef_tribe_has(const EfTables *t, int32_t tribe, int ability) {
    return (ef_tribe_flags(t, tribe) & EF_TRIBE_KNOWN) && ef_has(t->tribes[tribe].abilities, ability);
}

#endif /* EF_TABLES_H */
