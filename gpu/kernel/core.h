/* core.h - what every rule file shares: players, relations, the tech cache, the action stack,
 * tasks and scores (ports of the helpers at the top of engine-fast/src/step.rs and of a few
 * small functions in menu.rs and combat.rs). */
#ifndef EF_CORE_H
#define EF_CORE_H

#include "query.h"
#include "floats.h"

/* Coverage: the host build counts the command and action arms that run (twin.c defines
 * EF_COVERAGE); the device build counts nothing. */
#ifdef EF_COVERAGE
extern int64_t ef_coverage_commands[32];
extern int64_t ef_coverage_actions[96];
#define EF_COVER(table, k) ((void)((k) >= 0 && (k) < (int)(sizeof table / sizeof table[0]) && ++table[(k)]))
#else
#define EF_COVER(table, k) ((void)0)
#endif

/* a set of small indices as bits, for marks kept on a GPU thread's stack */
#define EF_BIT_WORDS(n) (((n) + 31) / 32)
EF_FN void ef_bit_set(uint32_t *bits, int32_t i) { bits[i >> 5] |= 1u << (i & 31); }
EF_FN int ef_bit(const uint32_t *bits, int32_t i) { return (int)((bits[i >> 5] >> (i & 31)) & 1u); }

#define EF_CITY 1
#define EF_RUIN 2
#define EF_STARTED 2
#define EF_FINAL_TURN 3
#define EF_ENDED 4
#define EF_NEUTRAL 0
#define EF_COOL_DOWN 3
#define EF_WATER 1
#define EF_OCEAN 2
#define EF_ICE 6
#define EF_FOREST 5
#define EF_MOUNTAIN 4
#define EF_POISONED 1
#define EF_DOUBLE_READY 7
#define EF_POLARIS_TRIBE 16
#define EF_DECOMPOSING 0          /* improvement effect */
#define EF_IMPROVEMENT_EFFECTS 2  /* an improvement's effect capacity (state.rs Inline<i32, 2>) */
#define EF_INVISIBLE_EFFECT 3
#define EF_PEACE_REQUEST_MESSAGE 1
#define EF_EMBASSY_MESSAGE 5
#define EF_PEACE_REQUEST_TRIGGER 2

/* the task kinds, by their index in step.rs TASKS */
enum { EF_TASK_NONE, EF_TASK_PACIFIST, EF_TASK_GENIUS, EF_TASK_NETWORK, EF_TASK_WEALTH, EF_TASK_KILLER,
       EF_TASK_METROPOLIS, EF_TASK_EXPLORER, EF_TASK_EXPLORE_LIGHTHOUSES, EF_TASK_CONVERT, EF_TASK_KINDS };

typedef struct { int32_t x, y; } EfPoint;

EF_FN EfXy ef_xy(int32_t x, int32_t y) { EfXy c; c.x = x; c.y = y; return c; }
EF_FN int ef_xy_eq(EfXy a, EfXy b) { return a.x == b.x && a.y == b.y; }

/* the player to move */
EF_FN int32_t ef_mover(const EfGame *g) { return g->players[g->current_player_index].id; }

/* step.rs player_index: the index of the player, or the engine's error */
EF_FN EfResult ef_player(const EfGame *g, int32_t id, int *index) {
    *index = ef_player_index(g, id);
    return *index == EF_NONE ? ef_err(EF_ERR_MISSING_PLAYER_ID, id) : ef_ok();
}

/* state.rs Game::tile: the tile at a coordinate, or 0 off the board (the x range is checked,
 * then the index; unlike Game::index a negative y past the start is off the board too) */
EF_FN EfTile *ef_tile(EfGame *g, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    return i == EF_NONE ? 0 : &ef_tiles(g)[i];
}
EF_FN const EfTile *ef_tile_c(const EfGame *g, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    return i == EF_NONE ? 0 : &ef_ctiles(g)[i];
}

/* a rule not ported yet (argument: 1000 + move kind in admit, 2000 + command, 3000 + action) */
#define EF_NOT_PORTED_ADMIT 1000
#define EF_NOT_PORTED_COMMAND 2000
#define EF_NOT_PORTED_ACTION 3000
EF_FN EfResult ef_not_ported(int64_t what) { return ef_err(EF_ERR_NOT_PORTED, what); }

/* attack.rs area_damage (Stomp), defined in combat.h; a move and an embark call it earlier */
EF_BIG EfResult ef_area_damage(EfGame *g, const EfTables *t, int32_t node, EfXy center);
/* the special tribes' rules (tribes.h) */
EF_BIG EfResult ef_poison_tile(EfGame *g, const EfTables *t, int32_t node, EfXy c);
EF_BIG EfResult ef_update_tentacles(EfGame *g, const EfTables *t);
EF_FN EfResult ef_unit_effect_add(EfGame *g, int32_t node, int32_t effect);
EF_FN EfResult ef_ice_bank_levels(EfGame *g, const EfTables *t);
EF_FN EfResult ef_spores(EfGame *g, const EfTables *t, EfXy c);
EF_FN EfResult ef_eat_after_kill(EfGame *g, int32_t p, int32_t head);
EF_BIG EfResult ef_lost_city(EfGame *g, const EfTables *t, int32_t p, int pi, EfXy target);
EF_BIG EfResult ef_resource_revealed(EfGame *g, const EfTables *t, int32_t p, int32_t kind, int *out);
EF_FN EfResult ef_flood_tile(EfGame *g, int32_t actor, EfXy c);
EF_BIG EfResult ef_claim_area(EfGame *g, const EfTables *t, int32_t actor, int i, const int32_t *area, int32_t n);
EF_FN EfResult ef_restore_climate(EfGame *g, const EfTables *t, int i, EfXy c, int *has, EfAction *out);
EF_FN EfResult ef_attract(EfGame *g, const EfTables *t, int32_t p, EfXy c, int32_t kind, int32_t founded);
EF_BIG EfResult ef_tentacle_strike(EfGame *g, const EfTables *t, int32_t node, int32_t actor, EfXy target);

EF_FN EfResult ef_short_list_full(int32_t capacity) { return ef_err(EF_ERR_SHORT_LIST_FULL, capacity); }

/* remove entry k (0 <= k < len) of a narrowed int8 list, keeping the order (Inline::remove),
 * and find an entry in one */
EF_FN void ef_remove_at_i8(int8_t *items, int8_t *len, int k) {
    for (int i = k; i + 1 < *len; i++) items[i] = items[i + 1]; /* narrow: a copy within the list */
    (*len)--;                                                    /* narrow: len > k >= 0 */
}
EF_FN int ef_index_of_i8(const int8_t *items, int32_t len, int32_t v) {
    for (int i = 0; i < len; i++)
        if (items[i] == v) return i;
    return -1;
}
/* remove entry k of a unit effect list (0 <= k < len) */
EF_FN void ef_effect_remove_at(EfEffectList *l, int k) {
    for (int i = k; i + 1 < l->len_n; i++) l->items_n[i] = l->items_n[i + 1]; /* narrow: a copy within the list */
    l->len_n--;                                                              /* narrow: len > k >= 0 */
}
EF_FN int ef_index_of(const int32_t *items, int32_t len, int32_t v) {
    for (int i = 0; i < len; i++)
        if (items[i] == v) return i;
    return -1;
}
/* the first entry of a unit effect list equal to `v`, or -1 */
EF_FN int ef_effect_index(const EfEffectList *l, int64_t v) {
    for (int i = 0; i < l->len_n; i++)
        if (l->items_n[i] == v) return i;
    return -1;
}

/* ---- the action stack (newest last) ---- */
EF_FN EfResult ef_push(EfGame *g, EfAction a) {
    if (g->actions_len >= EF_MAX_ACTIONS) return ef_err(EF_ERR_ACTIONS_FULL, 0);
    g->actions[g->actions_len++] = a;
    return ef_ok();
}
EF_FN void ef_reverse_actions(EfAction *a, int from, int to) {
    for (to -= 1; from < to; from++, to--) { EfAction x = a[from]; a[from] = a[to]; a[to] = x; }
}
/* step.rs buffered_last on the stack entries from `start` on: [A B] with A = [start, mid)
 * becomes [B reversed(A)], as out[start..mid].reverse() then rotate_left(mid - start) does;
 * reversing [mid, end) and then [start, end) gives exactly that, with no buffer */
EF_FN void ef_buffered_last(EfGame *g, int start, int mid) {
    ef_reverse_actions(g->actions, mid, g->actions_len);
    ef_reverse_actions(g->actions, start, g->actions_len);
}

/* step.rs base: the two actions every command queues first */
EF_FN EfResult ef_base(EfGame *g, int32_t player, int64_t command) {
    EfAction a = ef_action(EF_A_END_COMMAND, player);
    a.w[0] = command;
    EF_TRY(ef_push(g, a));
    return ef_push(g, ef_action(EF_A_RUN_DEFERRED, player));
}

/* step.rs random_hash: the gameplay hash's seed, made from the gameplay seed on first use */
EF_FN int32_t ef_random_hash(EfGame *g) {
    if (!g->has_random_hash) {
        g->has_random_hash = 1;
        g->random_hash = g->gameplay_seed;
    }
    return (int32_t)(uint32_t)(uint64_t)g->random_hash;
}

/* step.rs refresh_tech_cache: a stale unlocked-tech cache is refilled with the known techs */
EF_FN EfResult ef_refresh_tech_cache(EfGame *g, const EfTables *t, int32_t id) {
    int i;
    EF_TRY(ef_player(g, id, &i));
    EfPlayer *p = &g->players[i];
    if (!p->has_tech_cache) return ef_err(EF_ERR_NULL_TECH_CACHE, 0);
    int same = p->tech_cache_len == p->available_tech_len;
    for (int k = 0; same && k < p->tech_cache_len; k++) same = p->tech_cache[k] == p->available_tech[k];
    if (!same) {
        int32_t n = 0;
        for (int k = 0; k < p->available_tech_len; k++)
            if (ef_tech_known(t, p->available_tech[k])) p->tech_cache[n++] = p->available_tech[k];
        p->tech_cache_len = n;
    }
    return ef_ok();
}

/* the default record the game inserts for an unmet player (state.rs Relation::DEFAULT) */
EF_FN EfRelation ef_relation_default(void) {
    EfRelation r;
    r.state = EF_NEUTRAL;
    r.last_attack_turn = -100;
    r.embassy_level = 0;
    r.last_peace_broken_turn = -100;
    r.first_meet = -100;
    r.embassy_build_turn = -100;
    r.previous_attack_turn = -100;
    return r;
}

/* step.rs relation_entry: player i's relation toward `other`, a default record inserted if
 * missing */
EF_FN EfResult ef_relation_entry(EfGame *g, int i, int32_t other, EfRelation **out) {
    if (other == 0) return ef_err(EF_ERR_RELATION_PLAYER_0, 0);
    EfPlayer *p = &g->players[i];
    int n = ef_index_of(p->relation_ids, p->relations_len, other);
    if (n < 0) {
        if (p->relations_len >= EF_MAX_PLAYERS) return ef_short_list_full(EF_MAX_PLAYERS);
        n = p->relations_len++;
        p->relation_ids[n] = other;
        p->relations[n] = ef_relation_default();
    }
    *out = &p->relations[n];
    return ef_ok();
}
/* step.rs relation_of: the record through the inserting lookup; player 0 gives the default */
EF_FN EfResult ef_relation_of(EfGame *g, int i, int32_t other, EfRelation *out) {
    if (other == 0) { *out = ef_relation_default(); return ef_ok(); }
    EfRelation *r;
    EF_TRY(ef_relation_entry(g, i, other, &r));
    *out = *r;
    return ef_ok();
}
/* step.rs relation_state */
EF_FN EfResult ef_relation_state_of(EfGame *g, int32_t p, int32_t q, int32_t *state) {
    if (q == 0) { *state = ef_relation_default().state; return ef_ok(); }
    int i;
    EF_TRY(ef_player(g, p, &i));
    EfRelation r;
    EF_TRY(ef_relation_of(g, i, q, &r));
    *state = r.state;
    return ef_ok();
}

/* the attack rules' peace check: `owner` is `p` itself, or at peace or cooling down with it
 * (the relation through the inserting lookup, as step.rs relation_state) */
EF_FN EfResult ef_at_peace(EfGame *g, int32_t p, int32_t owner, int *out) {
    *out = owner == p;
    if (*out) return ef_ok();
    int32_t r;
    EF_TRY(ef_relation_state_of(g, p, owner, &r));
    *out = r == EF_PEACE || r == EF_COOL_DOWN;
    return ef_ok();
}

/* step.rs current_capital */
EF_FN EfResult ef_current_capital(const EfGame *g, int32_t id, EfXy *out) {
    int i = ef_player_index(g, id);
    if (i == EF_NONE) return ef_err(EF_ERR_MISSING_PLAYER, 0);
    EfXy start = g->players[i].start_tile;
    const EfTile *tile = ef_tile_c(g, start);
    if (!tile) return ef_err(EF_ERR_CAPITAL_TILE, 0);
    if (tile->owner_n == id) { *out = start; return ef_ok(); }
    for (int k = 0; k < g->tile_count; k++) {
        const EfTile *x = &ef_ctiles(g)[k];
        if (x->owner_n == id && x->has_improvement_n && x->improvement.kind_n == EF_CITY) { *out = x->coordinates; return ef_ok(); }
    }
    *out = ef_xy(-1, -1);
    return ef_ok();
}

/* step.rs find_unit: the slot of the board unit with this id (tile roots, in tile order) */
EF_FN int32_t ef_find_unit(const EfGame *g, uint32_t id) {
    for (int k = 0; k < g->tile_count; k++) {
        int32_t s = ef_ctiles(g)[k].unit_n;
        if (s != EF_NONE && ef_cunits(g)[s].id == id) return s;
    }
    return EF_NONE;
}

/* step.rs facing_direction */
EF_FN int32_t ef_facing_direction(EfXy from, EfXy to) {
    const int8_t vx[8] = { -1, -1, -1, 0, 1, 1, 1, 0 };
    const int8_t vy[8] = { -1, 0, 1, 1, 1, 0, -1, -1 };
    int32_t dx = ef_wsub(to.x, from.x), dy = ef_wsub(to.y, from.y);
    int32_t ax = dx < 0 ? -dx : dx, ay = dy < 0 ? -dy : dy;
    if ((ax > ay ? ax : ay) <= 1) {
        for (int i = 0; i < 8; i++)
            if (vx[i] == dx && vy[i] == dy) return i;
        return 8;
    }
    int32_t best = 0, score = 0;
    for (int i = 0; i < 8; i++) {
        int32_t weight = vx[i] * vx[i] + vy[i] * vy[i] == 1 ? 1393 : 985;
        int32_t candidate = ef_wmul(ef_wadd(ef_wmul(vx[i], dx), ef_wmul(vy[i], dy)), weight);
        if (score < candidate) { score = candidate; best = i; }
    }
    return best;
}

/* step.rs pending_trigger: the player to move has a choice to make */
EF_FN int ef_pending_trigger(const EfGame *g) {
    int32_t p = ef_mover(g);
    for (int i = 0; i < g->triggers_len; i++)
        if (g->triggers[i].player == p) return 1;
    return 0;
}

/* menu.rs player_abilities / player_ability: the abilities the player's techs unlock */
EF_FN EfMask ef_player_abilities(const EfGame *g, const EfTables *t, int32_t player) {
    EfMask m; m.lo = 0; m.hi = 0;
    int i = ef_player_index(g, player);
    if (i == EF_NONE) return m;
    const EfPlayer *p = &g->players[i];
    for (int k = 0; k < p->available_tech_len; k++)
        if (ef_tech_known(t, p->available_tech[k])) m = ef_mask_or(m, t->techs[p->available_tech[k]].abilities);
    return m;
}
EF_FN int ef_player_ability(const EfGame *g, const EfTables *t, int32_t player, int ability) {
    return ef_has(ef_player_abilities(g, t, player), ability);
}

/* menu.rs tech_price */
EF_FN int32_t ef_tech_cost_for(const EfGame *g, const EfTables *t, const EfPlayer *p, int32_t tech) {
    int32_t tier = ef_tech_known(t, tech) ? t->techs[tech].cost : 0;
    if (tier == 0) return 0;
    return ef_tech_price(p->cities, tier, ef_player_ability(g, t, p->id, EF_AB_LITERACY));
}

/* menu.rs alive: whether the player is still in the game under the death condition */
EF_FN int ef_alive(const EfGame *g, int32_t player) {
    if (player == EF_NATURE) return 1;
    int cities = 0, units = 0;
    for (int k = 0; k < g->tile_count; k++) {
        const EfTile *x = &ef_ctiles(g)[k];
        if (x->owner_n == player && x->has_improvement_n && x->improvement.kind_n == EF_CITY) cities = 1;
        if (x->unit_n != EF_NONE && ef_cunits(g)[x->unit_n].owner_n == player) units = 1;
    }
    switch (g->death_condition) {
    case 0: return cities;
    case 1: return cities && units;
    default: return 1;
    }
}

/* combat.rs max_health: the unit's (or, unless it protects, its passenger's) health with promotions */
EF_FN EfResult ef_max_health(const EfGame *g, const EfTables *t, const EfUnit *u, int32_t *out) {
    const EfUnitData *d = ef_unit_data(t, u->kind_n);
    if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    const EfUnit *selected = u;
    if (u->passenger_n != EF_NONE && !ef_has(d->abilities, EF_AB_PROTECT)) selected = &ef_cunits(g)[u->passenger_n];
    const EfUnitData *s = ef_unit_data(t, selected->kind_n);
    if (!s) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    *out = ef_wadd(s->health, ef_wmul(selected->promotion_level_n, 50));
    return ef_ok();
}

/* menu.rs unit_effect_list membership for a unit */
EF_FN int ef_unit_effect(const EfGame *g, const EfUnit *u, int64_t effect) {
    return ef_has_effect(ef_effect_list(g, u->effects_n), effect);
}

/* linked.rs followers, one step at a time: the follower after `node` in its chain, or EF_NONE
 * at the end; `*steps` counts the walk, and a chain longer than the board's units is a cycle */
EF_FN EfResult ef_next_follower(const EfGame *g, int32_t node, int32_t *steps, int32_t *out) {
    uint32_t f = ef_cunits(g)[node].follower;
    *out = f == 0 ? EF_NONE : ef_find_unit(g, f);
    if (*out != EF_NONE && ++*steps > g->unit_cap) return ef_err(EF_ERR_CYCLIC_CHAIN, 0);
    return ef_ok();
}

/* state.rs Game::records_achievements: tracking on, and only at rules124 */
EF_FN int ef_records_achievements(const EfGame *g) { return g->achievements_active && g->version == 124; }

/* turns.rs winner, defined in turns.h (add_score below needs it) */
EF_BIG EfResult ef_winner(EfGame *g, int32_t *selected_id, int *completed);

/* step.rs add_score: the score wraps as an unsigned 32-bit number; at the score limit, if the
 * match is decided, GameOver for the scorer after it */
EF_FN EfResult ef_add_score(EfGame *g, int32_t id, int64_t amount) {
    int i = ef_player_index(g, id);
    if (i == EF_NONE) return ef_ok();
    uint32_t score = (uint32_t)(uint64_t)g->players[i].score + (uint32_t)(uint64_t)amount;
    g->players[i].score = (int64_t)score;
    int64_t limit = g->score_limit;
    if (limit > 0 && (int64_t)score >= limit && g->current_state == EF_STARTED) {
        int32_t winner;
        int completed;
        EF_TRY(ef_winner(g, &winner, &completed));
        if (completed) {
            EfAction over = ef_action(EF_A_GAME_OVER, id);
            over.i[0] = winner;
            EF_TRY(ef_push(g, over));
        }
    }
    return ef_ok();
}

/* ---- tasks (step.rs task_completed, task_notification, new_task) ---- */
EF_FN int ef_task_slot(const EfPlayer *p, int32_t kind) {
    for (int n = 0; n < p->tasks_len; n++)
        if (p->tasks[n].kind == kind) return n;
    return -1;
}

EF_FN EfResult ef_task_completed(EfGame *g, const EfTables *t, int i, int n, int *done) {
    EfPlayer *p = &g->players[i];
    int32_t id = p->id;
    EfTask *task = &p->tasks[n];
    *done = 0;
    if (task->completed) return ef_ok();
    int d = 0;
    switch (task->kind) {
    case EF_TASK_WEALTH: d = p->currency >= 100; break;
    case EF_TASK_GENIUS: {
        /* step.rs: menu.rs tribe_tech, the tribe's tree order */
        const EfTechTree *tree = ef_tribe_tree(t, p->tribe);
        if (!tree) return ef_err(EF_ERR_UNKNOWN_TRIBE, 0);
        if (tree->order_error) return ef_err(EF_ERR_TABLE, tree->order_error - 1);
        d = 1;
        for (int k = 0; d && k < tree->order_len; k++) d = ef_contains(p->available_tech, p->available_tech_len, tree->order[k]);
        break;
    }
    case EF_TASK_PACIFIST:
        if (!task->has_peaceful_turns) return ef_err(EF_ERR_PEACEFUL_TURNS, 0);
        d = task->peaceful_turns >= 5;
        break;
    case EF_TASK_KILLER:
        if (!task->has_kills) return ef_err(EF_ERR_KILLS, 0);
        d = task->kills >= 10;
        break;
    case EF_TASK_CONVERT:
        if (!task->has_conversions) return ef_err(EF_ERR_CONVERSIONS, 0);
        d = task->conversions >= 3;
        break;
    case EF_TASK_METROPOLIS:
        for (int k = 0; !d && k < g->tile_count; k++) {
            const EfTile *x = &ef_ctiles(g)[k];
            d = x->owner_n == id && x->has_improvement_n && x->improvement.kind_n == EF_CITY && x->improvement.level_n > 4;
        }
        break;
    case EF_TASK_NETWORK: {
        EfXy start = p->start_tile;
        int count = 0;
        for (int k = 0; k < g->tile_count; k++) {
            const EfTile *x = &ef_ctiles(g)[k];
            if (x->owner_n == id && !ef_xy_eq(x->coordinates, start) && x->has_improvement_n && x->improvement.kind_n == EF_CITY
                && x->improvement.connected_to_capital_of_player_n == id)
                count++;
        }
        d = count >= 5;
        break;
    }
    case EF_TASK_EXPLORER: {
        int explored = 0;
        for (int k = 0; k < g->tile_count; k++)
            explored += id == EF_NATURE || ef_contains_u8(ef_ctiles(g)[k].explorers_n, ef_ctiles(g)[k].explorers_len_n, id);
        task->has_explored_tiles = 1;
        task->explored_tiles = explored;
        d = explored >= g->tile_count;
        break;
    }
    case EF_TASK_EXPLORE_LIGHTHOUSES: {
        int32_t w = g->width - 1, h = g->tile_count / g->width - 1;
        EfXy corners[4];
        corners[0] = ef_xy(w, h); corners[1] = ef_xy(0, 0); corners[2] = ef_xy(0, h); corners[3] = ef_xy(w, 0);
        int count = 0;
        for (int k = 0; k < 4; k++) {
            const EfTile *c = ef_tile_c(g, corners[k]);
            count += c && (id == EF_NATURE || ef_contains_u8(c->explorers_n, c->explorers_len_n, id));
        }
        d = count == 4;
        break;
    }
    default: return ef_err(EF_ERR_TASK_NONE, 0);
    }
    task->completed = d;
    *done = d;
    return ef_ok();
}

/* step.rs task_notification and check_task: TaskCompleted when the player's task of this kind
 * completes now */
EF_FN EfResult ef_check_task(EfGame *g, const EfTables *t, int i, int32_t kind) {
    int n = ef_task_slot(&g->players[i], kind);
    if (n < 0) return ef_ok();
    int done;
    EF_TRY(ef_task_completed(g, t, i, n, &done));
    if (!done) return ef_ok();
    EfAction a = ef_action(EF_A_TASK_COMPLETED, g->players[i].id);
    a.i[0] = kind;
    return ef_push(g, a);
}

/* step.rs new_task */
EF_FN EfResult ef_new_task(int32_t kind, EfTask *out) {
    if (kind <= EF_TASK_NONE || kind >= EF_TASK_KINDS) return ef_err(EF_ERR_TASK_TYPE, kind);
    memset(out, 0, sizeof *out);
    out->kind = kind;
    out->started = 1;
    out->completed = 0;
    out->has_peaceful_turns = kind == EF_TASK_PACIFIST;
    out->has_kills = kind == EF_TASK_KILLER;
    out->kills = kind == EF_TASK_KILLER ? 1 : 0;
    out->has_conversions = kind == EF_TASK_CONVERT;
    out->has_explored_tiles = kind == EF_TASK_EXPLORER;
    return ef_ok();
}

#endif /* EF_CORE_H */
