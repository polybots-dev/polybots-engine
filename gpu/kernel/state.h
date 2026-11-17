/* state.h - a game's rule state as one fixed-size block: the device form of
 * engine-fast/src/state.rs `Game`.
 *
 * Every field is an integer: 32 or 64 bits where the engine's value needs it, narrower where
 * its range is known (fields with the suffix _n, written only where the
 * value fits; tests/narrow.rs). The block has no implicit padding, so it has the same layout
 * for the C compiler, nvcc and the Rust mirror in ../src/state.rs, which must list the same
 * fields in the same order (tests/state_roundtrip.rs compares sizes and offsets). Not held here, because no rule reads it: city name text, shorelines, display
 * values and viewer memory.
 *
 * Units and effect lists sit in tables whose slots are reused; -1 means "none". A slot's
 * number is a handle only and differs from the Rust engine's node index.
*/
#ifndef EF_STATE_H
#define EF_STATE_H

#include "ef.h"
#include "action.h"

#define EF_MAX_SIDE 30
#define EF_MAX_TILES (EF_MAX_SIDE * EF_MAX_SIDE)
/* a unit and its passenger on every tile, plus room for the ones a step replaces */
#define EF_MAX_UNITS (2 * EF_MAX_TILES + 32)
#define EF_MAX_PLAYERS 5        /* four seats and Nature */
#define EF_MAX_CONTINENTS 24    /* the most map generation makes at sides 11 to 30 is 8; a lost city adds one */
#define EF_MAX_TECH 56
#define EF_MAX_TASKS 10
#define EF_MAX_TRIGGERS 8
#define EF_NAMING_SEEDS 1024
/* queued actions, kept between decisions and used as the stack inside a step (at most 70 in
 * 5 900 measured games) */
#define EF_MAX_ACTIONS 128
#define EF_NONE (-1)
/* unit slots go in int16 fields (tile.unit_n) */
#ifdef __cplusplus
static_assert(EF_MAX_UNITS + 1 <= INT16_MAX, "unit and effect-list slots fit int16");
#else
_Static_assert(EF_MAX_UNITS + 1 <= INT16_MAX, "unit and effect-list slots fit int16");
#endif

/* Narrowed: player ids are 0
 * to 255 (Nature), turns, populations and scores fit 16 bits, kinds, levels and counts fit 8;
 * `upgrade_n` is the engine's wrapping u16. A write that could leave its range goes through
 * EF_PUT, which refuses. */
typedef struct {
    uint16_t production_n, base_score_n, upgrade_n, level_n;   /* the engine's wrapping u16 values */
    int16_t founded_n, population_n, xp_n;
    uint8_t owner_n, founder_n, connected_to_capital_of_player_n;
    int8_t kind_n, border_size_n, named_n, effects_len_n;
    int8_t effects_n[2];
    uint8_t rewards_n[14];                              /* counts, at most 255 each */
    int8_t pad;
} EfImprovement;

typedef struct {
    EfXy coordinates;
    EfXy ruling_city;
    EfImprovement improvement;
    int16_t unit_n;               /* slot in units, or EF_NONE */
    int16_t continent_n, last_population_check_n, available_population_n;
    uint8_t owner_n, capital_of_n;
    uint8_t explorers_n[5];
    int8_t terrain_n, climate_n, altitude_n, explorers_len_n;
    int8_t has_improvement_n, has_resource_n, resource_n;
    int8_t has_road_n, has_route_n, has_continent_n, had_route_n, skin_n;
    int8_t effects_len_n, effects_n[5];
    uint8_t explorers_mask_n; /* the explorers as bits (ef_explorer_bit), kept with explorers_n */
} EfTile;

/* narrowed: ids stay 32 bits (they grow through a game); slots, health, xp
 * and turns are 16 bits, the rest 8 */
typedef struct {
    uint32_t id, leader, follower;
    EfXy previous_turn_end_coordinates, coordinates, home;
    int16_t passenger_n;          /* slot in units, or EF_NONE */
    int16_t effects_n;            /* slot in effect_lists; slot 0 is the shared empty list */
    uint16_t health_n;            /* the engine reads it as a u16 */
    int16_t xp_n, created_turn_n;
    uint8_t owner_n;
    int8_t used_n;                /* 0: the slot is free */
    int8_t kind_n, promotion_level_n, moved_n, attacked_n, direction_n, flipped_n;
    int8_t is_temporary_explorer_n, birth_climate_n, birth_climate_skin_n;
    int8_t pad[3];
} EfUnit;

/* a unit effect list, narrowed: effect ids are below 15
 * (the view refuses others), the length is at most 9; to_device refuses anything wider */
typedef struct {
    int8_t used_n, len_n;
    int8_t items_n[9];
} EfEffectList;

typedef struct {
    int32_t kind, started, completed;
    int32_t has_peaceful_turns, has_kills, has_conversions, has_explored_tiles;
    int32_t pad;
    int64_t peaceful_turns, kills, conversions, explored_tiles;
} EfTask;

typedef struct {
    int32_t state, last_attack_turn, embassy_level, last_peace_broken_turn, first_meet, embassy_build_turn;
    int32_t previous_attack_turn;
} EfRelation;

typedef struct {
    int32_t id, currency;
    int32_t tasks_len, opinions_dirty;
    EfTask tasks[EF_MAX_TASKS];
    int32_t tribe;
    int32_t available_tech_len, available_tech[EF_MAX_TECH];
    int32_t known_players_len, known_players[EF_MAX_PLAYERS];
    int32_t cities, kills, casualties;
    int32_t relations_len, relation_ids[EF_MAX_PLAYERS];
    EfRelation relations[EF_MAX_PLAYERS];
    int32_t has_tech_cache, tech_cache_len, tech_cache[EF_MAX_TECH];
    EfXy start_tile;
    int32_t handicap, auto_play;
    int32_t messages_len, message_senders[EF_MAX_PLAYERS];
    int32_t built_unique_len, built_unique[8];
    int32_t block_train_units, skin, climate, has_chosen_tribe;
    int32_t pad;
    int64_t message_kinds[EF_MAX_PLAYERS];
    int64_t score, resigned_at_command, wiped_at_command, end_score, color;
    /* carried for the checkpoint; no rule reads these */
    int64_t tribe_mix, resigned_turn, wipe_outs, killer_id, killed_turn, capital_count_cache;
} EfPlayer;

typedef struct {
    /* what no step changes */
    int32_t width, tile_count, player_count, seats;
    int32_t win_by_capital, win_by_extermination, achievements_active;
    int32_t naming_case;          /* 0 Common, 1 Turkic, 2 any other */
    int32_t version, game_skin;   /* the rules version (123 or 124); the game's skin (state.game_skin, 0 none) */
    int64_t rules_mode, game_type, turn_limit, score_limit, death_condition, action_limit;
    /* counters and flags */
    int32_t current_turn, current_player_index;
    uint32_t current_unit_id;
    int32_t needs_routes;
    int64_t current_state;
    uint64_t current_command, decisions, commands;
    /* seeds */
    int64_t gameplay_seed, village_name_seed, random_hash, village_name_hash;
    int32_t has_random_hash, has_village_name_hash;
    int32_t naming_next, naming_count;   /* the seeds not yet drawn are naming_seeds[naming_next..naming_count] */
    int32_t triggers_len, actions_len;
    /* the block's room: tiles, units; the effect
     * lists number unit_cap + 1 */
    int32_t tile_cap, unit_cap;
    EfTrigger triggers[EF_MAX_TRIGGERS];
    EfAction actions[EF_MAX_ACTIONS];
    EfPlayer players[EF_MAX_PLAYERS];
    int32_t naming_seeds[EF_NAMING_SEEDS];
    /* board.continents (carried untyped by engine-fast): continent_count of them, -1 when the
     * checkpoint's list does not read as continents; a lost city appends one */
    int32_t continent_count, continent_pad;
    EfContinent continents[EF_MAX_CONTINENTS];
} EfGame;

/* The block is this header followed by its regions, at offsets from its capacities alone (the
 * one layout function, below): units[unit_cap], tiles[tile_cap], effect_lists[unit_cap + 1],
 * padded to 8 bytes. Every access to a region goes through the accessors. */
EF_FN uint64_t ef_units_offset(void) { return (uint64_t)sizeof(EfGame); }
EF_FN uint64_t ef_tiles_offset(int32_t unit_cap) { return ef_units_offset() + (uint64_t)unit_cap * sizeof(EfUnit); }
EF_FN uint64_t ef_lists_offset(int32_t tile_cap, int32_t unit_cap) {
    return ef_tiles_offset(unit_cap) + (uint64_t)tile_cap * sizeof(EfTile);
}
EF_FN uint64_t ef_block_size(int32_t tile_cap, int32_t unit_cap) {
    uint64_t end = ef_lists_offset(tile_cap, unit_cap) + (uint64_t)(unit_cap + 1) * sizeof(EfEffectList);
    return (end + 7u) & ~(uint64_t)7u;
}
EF_FN EfUnit *ef_units(EfGame *g) { return (EfUnit *)((char *)g + ef_units_offset()); }
EF_FN const EfUnit *ef_cunits(const EfGame *g) { return (const EfUnit *)((const char *)g + ef_units_offset()); }
EF_FN EfTile *ef_tiles(EfGame *g) { return (EfTile *)((char *)g + ef_tiles_offset(g->unit_cap)); }
EF_FN const EfTile *ef_ctiles(const EfGame *g) { return (const EfTile *)((const char *)g + ef_tiles_offset(g->unit_cap)); }
EF_FN EfEffectList *ef_lists(EfGame *g) { return (EfEffectList *)((char *)g + ef_lists_offset(g->tile_cap, g->unit_cap)); }
EF_FN const EfEffectList *ef_clists(const EfGame *g) {
    return (const EfEffectList *)((const char *)g + ef_lists_offset(g->tile_cap, g->unit_cap));
}

/* A digest of the whole block, its `size` bytes, for comparing the copy on a device with the
 * one on the host. The structs have no implicit padding (the host build fails on any:
 * -Werror=padded), so every byte hashed is a field or the zeroed tail pad. */
EF_FN uint64_t ef_game_digest(const EfGame *g, uint64_t size) {
    const unsigned char *bytes = (const unsigned char *)(const void *)g;
    uint64_t h = 0xcbf29ce484222325ull;
    for (uint64_t i = 0; i + 4 <= size; i += 4) {
        uint32_t w;
        memcpy(&w, bytes + i, sizeof w);
        h = (h ^ w) * 0x100000001b3ull;
    }
    return h;
}

/* Values read through the fields, so a layout that differs between builds shows: the units on
 * the board (reached from tiles), the sum of their health, the sum of the players' stars, and
 * the sum of tile terrain. */
EF_FN void ef_game_probe(const EfGame *g, int64_t out[4]) {
    int64_t units = 0, health = 0, stars = 0, terrain = 0;
    for (int i = 0; i < g->tile_count; i++) {
        terrain += ef_ctiles(g)[i].terrain_n;
        int guard = 0;
        for (int u = ef_ctiles(g)[i].unit_n; u != EF_NONE && guard < 4; u = ef_cunits(g)[u].passenger_n, guard++) {
            units += 1;
            health += ef_cunits(g)[u].health_n;
        }
    }
    for (int p = 0; p < g->player_count; p++) stars += g->players[p].currency;
    out[0] = units; out[1] = health; out[2] = stars; out[3] = terrain;
}

#endif /* EF_STATE_H */
