/* dump.h - every rule field of a game block as one list of numbers, each read through its
 * field name. ../src/dump.rs writes the same list from the Rust engine's `Game`, so a field
 * that the C structs and the Rust mirror place differently shows up as a difference
 * (tests/state_roundtrip.rs). A change to the list's order is made in both files. */
#ifndef EF_DUMP_H
#define EF_DUMP_H

#include "state.h"

typedef struct { int64_t *out; int32_t count, capacity; } EfDump;

EF_FN void ef_put(EfDump *d, int64_t v) {
    if (d->count < d->capacity) d->out[d->count] = v;
    d->count += 1;
}
EF_FN void ef_put_xy(EfDump *d, EfXy c) { ef_put(d, c.x); ef_put(d, c.y); }
EF_FN void ef_put_list(EfDump *d, const int32_t *items, int32_t len) {
    ef_put(d, len);
    for (int i = 0; i < len; i++) ef_put(d, items[i]);
}
EF_FN void ef_put_list_u8(EfDump *d, const uint8_t *items, int32_t len) {
    ef_put(d, len);
    for (int i = 0; i < len; i++) ef_put(d, items[i]);
}
EF_FN void ef_put_list_i8(EfDump *d, const int8_t *items, int32_t len) {
    ef_put(d, len);
    for (int i = 0; i < len; i++) ef_put(d, items[i]);
}

EF_FN void ef_dump_player(EfDump *d, const EfPlayer *p) {
    ef_put(d, p->id); ef_put(d, p->currency); ef_put(d, p->opinions_dirty);
    ef_put(d, p->tasks_len);
    for (int i = 0; i < p->tasks_len; i++) {
        const EfTask *t = &p->tasks[i];
        ef_put(d, t->kind); ef_put(d, t->started); ef_put(d, t->completed);
        ef_put(d, t->has_peaceful_turns); ef_put(d, t->peaceful_turns);
        ef_put(d, t->has_kills); ef_put(d, t->kills);
        ef_put(d, t->has_conversions); ef_put(d, t->conversions);
        ef_put(d, t->has_explored_tiles); ef_put(d, t->explored_tiles);
    }
    ef_put(d, p->tribe);
    ef_put_list(d, p->available_tech, p->available_tech_len);
    ef_put_list(d, p->known_players, p->known_players_len);
    ef_put(d, p->score); ef_put(d, p->cities); ef_put(d, p->kills); ef_put(d, p->casualties);
    ef_put(d, p->relations_len);
    for (int i = 0; i < p->relations_len; i++) {
        const EfRelation *r = &p->relations[i];
        ef_put(d, p->relation_ids[i]);
        ef_put(d, r->state); ef_put(d, r->last_attack_turn); ef_put(d, r->embassy_level);
        ef_put(d, r->last_peace_broken_turn); ef_put(d, r->first_meet); ef_put(d, r->embassy_build_turn);
        ef_put(d, r->previous_attack_turn);
    }
    ef_put(d, p->has_tech_cache);
    ef_put_list(d, p->tech_cache, p->has_tech_cache ? p->tech_cache_len : 0);
    ef_put_xy(d, p->start_tile);
    ef_put(d, p->handicap); ef_put(d, p->auto_play);
    ef_put(d, p->resigned_at_command); ef_put(d, p->wiped_at_command);
    ef_put(d, p->messages_len);
    for (int i = 0; i < p->messages_len; i++) { ef_put(d, p->message_kinds[i]); ef_put(d, p->message_senders[i]); }
    ef_put_list(d, p->built_unique, p->built_unique_len);
    ef_put(d, p->block_train_units); ef_put(d, p->skin); ef_put(d, p->end_score); ef_put(d, p->color);
    ef_put(d, p->tribe_mix); ef_put(d, p->climate); ef_put(d, p->has_chosen_tribe); ef_put(d, p->resigned_turn);
    ef_put(d, p->wipe_outs); ef_put(d, p->killer_id); ef_put(d, p->killed_turn); ef_put(d, p->capital_count_cache);
}

EF_FN void ef_dump_unit(EfDump *d, const EfGame *g, const EfUnit *u) {
    ef_put(d, u->id); ef_put(d, u->leader); ef_put(d, u->follower); ef_put(d, u->owner_n); ef_put(d, u->kind_n);
    ef_put_xy(d, u->previous_turn_end_coordinates); ef_put_xy(d, u->coordinates); ef_put_xy(d, u->home);
    ef_put(d, u->health_n); ef_put(d, u->promotion_level_n); ef_put(d, u->xp_n); ef_put(d, u->moved_n);
    ef_put(d, u->attacked_n); ef_put(d, u->direction_n); ef_put(d, u->flipped_n); ef_put(d, u->created_turn_n);
    /* which list (units may share one), then its entries */
    ef_put(d, u->effects_n);
    const EfEffectList *l = &ef_clists(g)[u->effects_n];
    ef_put(d, l->len_n);
    for (int i = 0; i < l->len_n; i++) ef_put(d, l->items_n[i]);
    ef_put(d, u->is_temporary_explorer_n); ef_put(d, u->birth_climate_n); ef_put(d, u->birth_climate_skin_n);
}

EF_FN void ef_dump_tile(EfDump *d, const EfGame *g, const EfTile *t) {
    ef_put_xy(d, t->coordinates);
    ef_put(d, t->terrain_n); ef_put(d, t->climate_n); ef_put(d, t->altitude_n); ef_put(d, t->owner_n); ef_put(d, t->capital_of_n);
    ef_put_list_u8(d, t->explorers_n, t->explorers_len_n);
    ef_put_xy(d, t->ruling_city);
    ef_put(d, t->has_improvement_n);
    if (t->has_improvement_n) {
        const EfImprovement *m = &t->improvement;
        ef_put(d, m->kind_n); ef_put(d, m->owner_n); ef_put(d, m->founder_n); ef_put(d, m->level_n); ef_put(d, m->founded_n);
        ef_put(d, m->xp_n); ef_put(d, m->population_n); ef_put(d, m->production_n); ef_put(d, m->base_score_n);
        ef_put(d, m->border_size_n); ef_put(d, m->upgrade_n); ef_put(d, m->connected_to_capital_of_player_n);
        ef_put(d, m->named_n);
        for (int i = 0; i < 14; i++) ef_put(d, m->rewards_n[i]);
        ef_put_list_i8(d, m->effects_n, m->effects_len_n);
    }
    ef_put(d, t->has_resource_n); ef_put(d, t->has_resource_n ? t->resource_n : 0);
    ef_put(d, t->has_road_n); ef_put(d, t->has_route_n);
    ef_put(d, t->has_continent_n); ef_put(d, t->has_continent_n ? t->continent_n : 0);
    ef_put(d, t->had_route_n); ef_put(d, t->last_population_check_n); ef_put(d, t->available_population_n); ef_put(d, t->skin_n);
    ef_put_list_i8(d, t->effects_n, t->effects_len_n);
    /* the unit on the tile and the units it carries, each after a 1; a 0 ends the chain */
    int guard = 0;
    for (int u = t->unit_n; u != EF_NONE && guard < 8; u = ef_cunits(g)[u].passenger_n, guard++) {
        ef_put(d, 1);
        ef_dump_unit(d, g, &ef_cunits(g)[u]);
    }
    ef_put(d, 0);
}

/* Writes at most `capacity` numbers and returns how many the game has. */
EF_FN int32_t ef_game_dump(const EfGame *g, int64_t *out, int32_t capacity) {
    EfDump d;
    d.out = out; d.count = 0; d.capacity = capacity;
    ef_put(&d, g->width); ef_put(&d, g->tile_count); ef_put(&d, g->player_count); ef_put(&d, g->seats);
    ef_put(&d, g->win_by_capital); ef_put(&d, g->win_by_extermination); ef_put(&d, g->achievements_active);
    ef_put(&d, g->naming_case); ef_put(&d, g->version);
    ef_put(&d, g->rules_mode); ef_put(&d, g->game_type); ef_put(&d, g->turn_limit); ef_put(&d, g->score_limit);
    ef_put(&d, g->death_condition); ef_put(&d, g->action_limit);
    ef_put(&d, g->current_turn); ef_put(&d, g->current_player_index); ef_put(&d, g->current_unit_id);
    ef_put(&d, g->needs_routes); ef_put(&d, g->current_state);
    ef_put(&d, (int64_t)g->current_command); ef_put(&d, (int64_t)g->decisions); ef_put(&d, (int64_t)g->commands);
    ef_put(&d, g->gameplay_seed); ef_put(&d, g->village_name_seed);
    ef_put(&d, g->has_random_hash); ef_put(&d, g->has_random_hash ? g->random_hash : 0);
    ef_put(&d, g->has_village_name_hash); ef_put(&d, g->has_village_name_hash ? g->village_name_hash : 0);
    ef_put(&d, g->naming_count - g->naming_next);
    for (int i = g->naming_next; i < g->naming_count; i++) ef_put(&d, g->naming_seeds[i]);
    ef_put(&d, g->triggers_len);
    for (int i = 0; i < g->triggers_len; i++) {
        const EfTrigger *t = &g->triggers[i];
        ef_put(&d, t->player); ef_put(&d, t->opponent); ef_put(&d, t->kind); ef_put_xy(&d, t->coordinates);
    }
    ef_put(&d, g->actions_len);
    for (int i = 0; i < g->actions_len; i++) {
        const EfAction *a = &g->actions[i];
        ef_put(&d, a->kind); ef_put(&d, a->player);
        for (int k = 0; k < 6; k++) ef_put(&d, a->i[k]);
        ef_put(&d, a->w[0]); ef_put(&d, a->w[1]);
        for (int k = 0; k < 3; k++) ef_put_xy(&d, a->p[k]);
        ef_put(&d, a->len);
    }
    for (int i = 0; i < g->player_count; i++) ef_dump_player(&d, &g->players[i]);
    for (int i = 0; i < g->tile_count; i++) ef_dump_tile(&d, g, &ef_ctiles(g)[i]);
    return d.count;
}

#endif /* EF_DUMP_H */
