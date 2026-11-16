/* build.h - the rest of engine-fast/src/economy.rs: Build, DestroyImprovement, the other
 * improvement actions (road, resources, levels) and the City actions (population, level-up). */
#ifndef EF_BUILD_H
#define EF_BUILD_H

#include "economy.h"

#define EF_ROAD 3
#define EF_METROPOLIS_TASK EF_TASK_METROPOLIS
#define EF_CITY_LEVEL_UP 1

EF_FN EfAction ef_score_child(int32_t player, int32_t amount, EfXy c) {
    EfAction a = ef_action_at(EF_A_INCREASE_SCORE, player, c);
    a.i[0] = amount;
    return a;
}

EF_FN EfResult ef_update_connections_of(EfGame *g, int32_t player) {
    EfAction u = ef_action(EF_A_UPDATE_CITY_CONNECTIONS, player);
    u.len = 1;
    u.list.players[0] = player;
    return ef_push(g, u);
}

/* economy.rs build: the children from the improvement on are
 * buffered behind the surrounding-area ones */
EF_BIG EfResult ef_build(EfGame *g, const EfTables *t, int32_t player, int32_t kind, EfXy c, int pay) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_ok();
    const EfImprovementData *d = ef_economy_data(t, kind);
    if (!d) return ef_ok();
    int p = ef_player_index(g, player);
    if (p == EF_NONE) return ef_ok();
    int consumed = ef_has(d->abilities, EF_AB_CONSUMED), temporary = ef_has(d->abilities, EF_AB_DISCRETE);
    int changes_route = 0;
    for (int k = 0; k < d->creates_len; k++) {
        changes_route |= d->creates[k].terrain != EF_NONE || d->creates[k].terrain_error != 0 || d->creates[k].effect;
    }
    int32_t start = g->actions_len;
    if (kind != EF_ROAD && !consumed && !temporary) {
        EfImprovement *m = &ef_tiles(g)[i].improvement;
        memset(m, 0, sizeof *m);
        ef_tiles(g)[i].has_improvement_n = 1; /* narrow: a flag */
        EF_PUT(m->kind_n, kind, int8_t);
        EF_PUT(m->founder_n, player, uint8_t);
        EF_PUT(m->founded_n, g->current_turn, int16_t);
        m->production_n = 1; /* narrow: the constant 1 */
        EF_PUT(m->base_score_n, d->reward_score, uint16_t);
        EF_PUT(m->border_size_n, d->has_border_size ? d->border_size : 0, int8_t);
        if (ef_has(d->abilities, EF_AB_PATINA)) {
            int32_t level;
            EF_TRY(ef_level_for(g, t, i, &level));
            EF_PUT(ef_tiles(g)[i].improvement.level_n, level, uint16_t);
        }
        int32_t amount = ef_improvement_score(t, g, i);
        if (amount > 0) EF_TRY(ef_push(g, ef_score_child(player, amount, c)));
    }
    if (d->named == EF_NAMED_BRIDGE) {
        int32_t j;
#define EF_BANCHOR(dx, dy) ((j = ef_tile_index(g, ef_xy(c.x + (dx), c.y + (dy)))) != EF_NONE && ef_ctiles(g)[j].terrain_n != 1 && ef_ctiles(g)[j].terrain_n != 2)
        int horizontal = !(EF_BANCHOR(0, -1) && EF_BANCHOR(0, 1)) && EF_BANCHOR(1, 0) && EF_BANCHOR(-1, 0);
#undef EF_BANCHOR
        if (!ef_ctiles(g)[i].has_improvement_n) return ef_err(EF_ERR_BRIDGE_IMPROVEMENT, 0);
        ef_tiles(g)[i].improvement.level_n = (uint16_t)horizontal; /* narrow: 0 or 1 */
    }
    if (consumed && kind != 52 && ef_ctiles(g)[i].has_resource_n) EF_TRY(ef_push(g, ef_action_at(EF_A_DESTROY_RESOURCE, player, c)));
    if (kind == EF_ROAD) EF_TRY(ef_push(g, ef_action_at(EF_A_BUILD_ROAD, player, c)));
    if (kind == EF_ROAD || d->has_routes || ef_has(d->abilities, EF_AB_BRIDGE) || ef_has(d->abilities, EF_AB_FILL) || changes_route) {
        if (kind != EF_ROAD) EF_TRY(ef_push(g, ef_action(EF_A_UPDATE_ROUTES, player)));
        EF_TRY(ef_update_connections_of(g, player));
    }
    for (int32_t k = 0; k < d->reward_currency; k++) {
        EfAction a = ef_action_at(EF_A_INCREASE_CURRENCY, player, c);
        a.i[0] = 1;
        a.i[1] = 40;
        EF_TRY(ef_push(g, a));
    }
    if (pay) {
        int32_t cur = ef_wsub(g->players[p].currency, d->has_cost ? d->cost : 0);
        g->players[p].currency = cur > 0 ? cur : 0;
    }
    EfXy ruling = ef_ctiles(g)[i].ruling_city;
    if (ef_tile_index(g, ruling) != EF_NONE) {
        for (int32_t k = 0; k < d->reward_population; k++) {
            EfAction a = ef_action_at(EF_A_INCREASE_POPULATION, player, c);
            a.p[1] = ruling;
            a.i[0] = 60;
            EF_TRY(ef_push(g, a));
        }
    }
    if (ef_has(d->abilities, EF_AB_FLOOD)) EF_TRY(ef_push(g, ef_action_at(EF_A_FLOOD, player, c)));
    if (ef_has(d->abilities, EF_AB_FILL)) ef_drain_tile(&ef_tiles(g)[i]);
    for (int k = 0; k < d->creates_len; k++) {
        const EfCreates *e = &d->creates[k];
        if (e->terrain_error) return ef_err(EF_ERR_TABLE, e->terrain_error - 1);
        if (e->terrain != EF_NONE) {
            EF_PUT(ef_tiles(g)[i].terrain_n, e->terrain, int8_t);
            ef_tiles(g)[i].has_resource_n = 0; /* narrow: a flag */
            ef_tiles(g)[i].resource_n = 0; /* narrow: none */
        }
        if (e->resource != EF_NONE || e->resource_error) {
            /* created unless the player already sees a resource there (infiltrate.rs
             * visible_resource) */
            const EfTile *x = &ef_ctiles(g)[i];
            int explored = player == EF_NATURE || ef_contains_u8(x->explorers_n, x->explorers_len_n, player);
            int seen = 0;
            if (explored && x->has_resource_n && !x->has_improvement_n) EF_TRY(ef_resource_revealed(g, t, player, x->resource_n, &seen));
            if (!seen) {
                if (e->resource_error) return ef_err(EF_ERR_TABLE, e->resource_error - 1);
                EfAction a = ef_action_at(EF_A_CREATE_RESOURCE, player, c);
                a.i[0] = e->resource;
                a.i[1] = 0;
                EF_TRY(ef_push(g, a));
            }
        }
        /* EnchantAnimal: a free unit of the named kind (not resolved through the tribe) */
        if (e->unit_error) return ef_err(EF_ERR_TABLE, e->unit_error - 1);
        if (e->unit != EF_NONE) {
            EfAction a = ef_action_at(EF_A_TRAIN, player, c);
            a.i[0] = e->unit;
            a.i[1] = 0;
            a.p[1] = ef_xy(-1, -1);
            a.i[2] = 0;
            EF_TRY(ef_push(g, a));
        }
        /* the Algae build's effect; Foam joins Algae on the Cute game skin */
        if (e->effect) {
            if (e->effect_kind_error) return ef_err(EF_ERR_TABLE, e->effect_kind_error - 1);
            EfTile *x = &ef_tiles(g)[i];
            EF_TRY(ef_tile_effect_add(x, e->effect_kind));
            int32_t skin = g->game_skin == 0 ? g->players[p].skin : g->game_skin;
            if (e->effect_kind == EF_ALGAE && skin == EF_CUTE_SKIN) EF_TRY(ef_tile_effect_add(x, EF_FOAM));
        }
    }
    if (ef_has(d->abilities, EF_AB_MANUAL)) {
        int32_t n = ef_ctiles(g)[i].unit_n;
        if (n != EF_NONE) {
            if (ef_unit_effect(g, &ef_cunits(g)[n], EF_INVISIBLE_EFFECT)) {
                EfAction reveal = ef_action_at(EF_A_REVEAL, player, c);
                reveal.p[1] = ef_xy(-1, -1);
                EF_TRY(ef_push(g, reveal));
            }
            ef_units(g)[n].moved_n = 1; /* narrow: a flag or constant */
            ef_units(g)[n].attacked_n = 1; /* narrow: a flag or constant */
        }
    }
    int32_t buffered = g->actions_len;
    EF_TRY(ef_surrounding(g, t, player, c));
    if (d->named == EF_NAMED_ICEBANK) EF_TRY(ef_ice_bank_levels(g, t));
    if (ef_has(d->abilities, EF_AB_UNIQUE)) {
        EfPlayer *x = &g->players[p];
        if (x->built_unique_len >= 8) return ef_err(EF_ERR_SHORT_LIST_REFUSED, 8);
        x->built_unique[x->built_unique_len++] = kind;
    }
    ef_buffered_last(g, start, buffered);
    return ef_ok();
}

/* economy.rs population_at_level */
EF_FN int32_t ef_population_at_level(const EfImprovementData *d, int32_t level) {
    int32_t v = d->reward_population;
    if ((d->has_max_level ? d->max_level : 0) > 0)
        for (int k = 0; k < d->growth_len; k++) v = ef_wadd(v, ef_wmul(d->growth_population[k], level));
    return v;
}

/* economy.rs destroy */
EF_BIG EfResult ef_destroy(EfGame *g, const EfTables *t, int32_t player, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_DESTROY_TILE, 0);
    EfAction score = ef_action(EF_A_DECREASE_SCORE, player);
    score.i[0] = ef_improvement_score(t, g, i);
    EF_TRY(ef_push(g, score));
    if (!ef_ctiles(g)[i].has_improvement_n) return ef_err(EF_ERR_DESTROY_NONE, 0);
    EfImprovement m = ef_ctiles(g)[i].improvement;
    int32_t start = g->actions_len;
    const EfImprovementData *d = ef_economy_data(t, m.kind_n);
    if (d) {
        EfXy ruling = ef_ctiles(g)[i].ruling_city;
        int32_t n = ef_population_at_level(d, m.level_n);
        for (int32_t k = 0; k < (n > 0 ? n : 0); k++) {
            EfAction a = ef_action_at(EF_A_DECREASE_POPULATION, player, ruling);
            a.i[0] = 200;
            EF_TRY(ef_push(g, a));
        }
        for (int k = 0; k < d->requirement_len; k++) {
            if (d->requirements[k].resource_error) return ef_err(EF_ERR_TABLE, d->requirements[k].resource_error - 1);
            if (d->requirements[k].resource != EF_NONE) {
                ef_tiles(g)[i].has_resource_n = 1; /* narrow: a flag */
                EF_PUT(ef_tiles(g)[i].resource_n, d->requirements[k].resource, int8_t);
            }
        }
        if (d->has_routes || ef_has(d->abilities, EF_AB_BRIDGE)) {
            EF_TRY(ef_push(g, ef_action(EF_A_UPDATE_ROUTES, player)));
            EF_TRY(ef_update_connections_of(g, player));
            ef_tiles(g)[i].has_road_n = 0; /* narrow: a flag */
        }
    }
    ef_tiles(g)[i].has_improvement_n = 0; /* narrow: a flag */
    memset(&ef_tiles(g)[i].improvement, 0, sizeof ef_tiles(g)[i].improvement);
    int32_t buffered = g->actions_len;
    EF_TRY(ef_surrounding(g, t, player, c));
    ef_buffered_last(g, start, buffered);
    return ef_ok();
}

/* economy.rs economy_action for the improvement actions other than Build */
EF_BIG EfResult ef_improvement_action(EfGame *g, const EfTables *t, const EfAction *a) {
    int32_t player = a->player;
    EfXy c = a->p[0];
    switch (a->kind) {
    case EF_A_BUILD_ROAD: {
        int32_t i = ef_tile_index(g, c);
        if (i == EF_NONE) return ef_err(EF_ERR_ROAD_TILE, 0);
        ef_tiles(g)[i].has_road_n = 1; /* narrow: a flag */
        return ef_ok();
    }
    case EF_A_DESTROY_RESOURCE: {
        int32_t i = ef_tile_index(g, c);
        if (i == EF_NONE) return ef_err(EF_ERR_RESOURCE_TILE, 0);
        if (!ef_ctiles(g)[i].has_resource_n) return ef_ok();
        EF_TRY(ef_surrounding(g, t, player, c));
        ef_tiles(g)[i].has_resource_n = 0; /* narrow: a flag */
        ef_tiles(g)[i].resource_n = 0; /* narrow: none */
        return ef_ok();
    }
    case EF_A_CREATE_RESOURCE: {
        int32_t i = ef_tile_index(g, c);
        if (i == EF_NONE) return ef_ok();
        ef_tiles(g)[i].has_resource_n = 1; /* narrow: a flag */
        EF_PUT(ef_tiles(g)[i].resource_n, a->i[0], int8_t);
        return ef_surrounding(g, t, player, c);
    }
    default: {
        int down = a->kind == EF_A_LEVEL_DOWN_IMPROVEMENT;
        int32_t i = ef_tile_index(g, c);
        if (i == EF_NONE) return ef_err(EF_ERR_LEVEL_TILE, 0);
        if (!ef_ctiles(g)[i].has_improvement_n || ef_ctiles(g)[i].improvement.kind_n == 0) return ef_ok();
        int32_t current = ef_ctiles(g)[i].improvement.level_n, calculated;
        EF_TRY(ef_level_for(g, t, i, &calculated));
        if ((down && calculated >= current) || (!down && current >= calculated)) return ef_ok();
        int32_t amount = down ? a->i[0] : 1;
        EfImprovement *m = &ef_tiles(g)[i].improvement;
        if (down && m->level_n == 0) return ef_ok();
        m->level_n = down ? (uint16_t)(m->level_n - (uint16_t)(uint32_t)amount) /* narrow: the engine's u16 wrapping */
                          : (uint16_t)(m->level_n + 1u);                        /* narrow: the engine's u16 wrapping */
        int32_t kind = m->kind_n;
        EfXy ruling = ef_ctiles(g)[i].ruling_city;
        if (ef_improvement_known(t, kind)) {
            const EfImprovementData *d = &t->improvements[kind];
            for (int k = 0; k < d->growth_len; k++) {
                int32_t population = d->growth_population[k];
                if (population > 0 && (down || !(ruling.x == -1 && ruling.y == -1))) {
                    for (int32_t q = 0; q < population; q++) {
                        EfAction x;
                        if (down) {
                            x = ef_action_at(EF_A_DECREASE_POPULATION, player, ruling);
                            x.i[0] = 200;
                        } else {
                            x = ef_action_at(EF_A_INCREASE_POPULATION, player, c);
                            x.p[1] = ruling;
                            x.i[0] = 60;
                        }
                        EF_TRY(ef_push(g, x));
                    }
                }
                if (!down && d->growth_score_each[k] > 0) EF_TRY(ef_push(g, ef_score_child(player, d->growth_score_each[k], c)));
            }
        }
        return ef_surrounding(g, t, player, c);
    }
    }
}

/* economy.rs city_action: population changes and the level-up with its trigger */
EF_BIG EfResult ef_city_action(EfGame *g, const EfTables *t, int32_t kind, int32_t player, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_CITY_ACTION_TILE, 0);
    int32_t owner = ef_ctiles(g)[i].owner_n;
    int is_city = ef_ctiles(g)[i].has_improvement_n && ef_ctiles(g)[i].improvement.kind_n == EF_CITY;
    if (is_city && kind != EF_A_INCREASE_POPULATION && ef_player_index(g, owner) == EF_NONE) return ef_err(EF_ERR_CITY_ACTION_OWNER, 0);
    if (!is_city) return ef_ok();
    if (kind == EF_A_INCREASE_POPULATION || kind == EF_A_DECREASE_POPULATION) {
        int up = kind == EF_A_INCREASE_POPULATION;
        int32_t before = ef_improvement_score(t, g, i);
        EfImprovement *m = &ef_tiles(g)[i].improvement;
        EF_PUT(m->xp_n, ef_wadd(m->xp_n, up ? 1 : -1), int16_t);
        EF_PUT(m->population_n, ef_wadd(m->population_n, up ? 1 : -1), int16_t);
        int32_t after = ef_improvement_score(t, g, i);
        if (up) {
            EF_TRY(ef_push(g, ef_score_child(player, ef_wsub(after, before), c)));
            const EfImprovement *x = &ef_ctiles(g)[i].improvement;
            if (x->level_n < x->xp_n && owner == ef_mover(g)) EF_TRY(ef_push(g, ef_action_at(EF_A_LEVEL_UP, player, c)));
        } else {
            int p;
            EF_TRY(ef_player(g, owner, &p));
            g->players[p].score = (int64_t)((uint32_t)(uint64_t)g->players[p].score - (uint32_t)ef_wsub(before, after));
        }
        return ef_ok();
    }
    /* LevelUp */
    EfImprovement *m = &ef_tiles(g)[i].improvement;
    if (m->level_n >= m->xp_n) return ef_ok();
    EF_PUT(m->level_n, m->level_n + 1, uint16_t);
    EF_PUT(m->xp_n, ef_wsub(m->xp_n, m->level_n), int16_t);
    m->upgrade_n = (uint16_t)(m->upgrade_n - 1u); /* narrow: the engine's u16 wrapping */
    EF_TRY(ef_push(g, ef_score_child(owner, 50 - m->level_n * 5, c)));
    int p;
    EF_TRY(ef_player(g, owner, &p));
    if (ef_task_slot(&g->players[p], EF_METROPOLIS_TASK) < 0) {
        EfAction e = ef_action(EF_A_ENABLE_TASK, owner);
        e.i[0] = EF_METROPOLIS_TASK;
        EF_TRY(ef_push(g, e));
    }
    if (g->triggers_len >= EF_MAX_TRIGGERS) return ef_err(EF_ERR_TRIGGERS_FULL, 0);
    EfTrigger *tr = &g->triggers[g->triggers_len++];
    tr->player = owner;
    tr->opponent = 0;
    tr->kind = EF_CITY_LEVEL_UP;
    tr->coordinates = c;
    return ef_check_task(g, t, p, EF_METROPOLIS_TASK);
}

#endif /* EF_BUILD_H */
