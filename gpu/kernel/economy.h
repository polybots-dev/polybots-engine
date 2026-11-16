/* economy.h - port of engine-fast/src/economy.rs, the parts ported so far: improvement levels
 * (level_for, adjacency_bonus, update_level, surrounding) and the improvement score. */
#ifndef EF_ECONOMY_H
#define EF_ECONOMY_H

#include "core.h"

#define EF_PORT 7

/* economy.rs data: the catalog entry of an improvement kind other than 0 */
EF_FN const EfImprovementData *ef_economy_data(const EfTables *t, int32_t kind) {
    return kind != 0 ? ef_improvement_data(t, kind) : 0;
}

/* economy.rs improvement_score */
EF_FN int32_t ef_improvement_score(const EfTables *t, const EfGame *g, int i) {
    const EfTile *x = &ef_ctiles(g)[i];
    if (!x->has_improvement_n) return 0;
    if (x->owner_n == 0 || x->owner_n == EF_NATURE) return 0;
    const EfImprovement *m = &x->improvement;
    const EfImprovementData *d = ef_improvement_data(t, m->kind_n);
    if (!d) return 0;
    int32_t score = m->base_score_n;
    if (m->kind_n == EF_CITY) {
        score = ef_wadd(ef_wadd(score, ef_wmul(ef_wadd(m->level_n, 1), 50)), ef_wmul(m->xp_n, 5));
        if (m->xp_n < 0) {
            score = ef_wadd(score, ef_wmul(m->xp_n, 100));
            if (score < 0) score = 0;
        }
    }
    if (ef_has(d->abilities, EF_AB_PATINA)) score = ef_wadd(score, ef_wmul(d->growth_score, m->level_n));
    return score;
}

/* economy.rs adjacency_bonus */
EF_BIG EfResult ef_adjacency_bonus(const EfGame *g, int i, const EfImprovementData *d, int32_t *out) {
    int unique = ef_has(d->abilities, EF_AB_PICKY), per_tile = ef_has(d->abilities, EF_AB_SIMPLE);
    EfXy c = ef_ctiles(g)[i].coordinates;
    int32_t owner = ef_ctiles(g)[i].owner_n;
    int32_t seen[2 * EF_MAX_ADJACENCY * 8];
    int32_t seen_len = 0, total = 0;
    for (int32_t x = c.x - 1; x <= c.x + 1; x++) {
        for (int32_t y = c.y - 1; y <= c.y + 1; y++) {
            if (x == c.x && y == c.y) continue;
            int32_t j = ef_tile_index(g, ef_xy(x, y));
            if (j == EF_NONE) continue;
            const EfTile *o = &ef_ctiles(g)[j];
            if (o->owner_n != owner) continue;
            for (int k = 0; k < d->adjacency_len; k++) {
                const EfAdjacency *e = &d->adjacency[k];
                for (int part = 0; part < 2; part++) {
                    int32_t id, level;
                    if (part == 0) {
                        if (e->improvement_error) return ef_err(EF_ERR_TABLE, e->improvement_error - 1);
                        if (e->improvement == EF_NONE) continue;
                        id = e->improvement;
                        if (!(o->has_improvement_n && o->improvement.kind_n != 0 && o->improvement.kind_n == id)) continue;
                        level = o->improvement.level_n;
                    } else {
                        if (e->resource_error) return ef_err(EF_ERR_TABLE, e->resource_error - 1);
                        if (e->resource == EF_NONE) continue;
                        id = e->resource;
                        if (!(!o->has_improvement_n && o->has_resource_n && o->resource_n != 0 && o->resource_n == id)) continue;
                        level = 1;
                    }
                    /* the add closure of the Rust */
                    if (unique) {
                        if (!ef_contains(seen, seen_len, id)) {
                            if (seen_len < (int32_t)(sizeof seen / sizeof seen[0])) seen[seen_len++] = id;
                            total = ef_wadd(total, 1);
                        }
                    } else {
                        total = ef_wadd(total, per_tile ? 1 : (level > 1 ? level : 1));
                    }
                }
            }
        }
    }
    *out = total;
    return ef_ok();
}

/* polaris.rs ice_bank_level: floor(count / 20) of the tiles that are ice or Polaris land (the
 * Rust divides as f32; for a tile count below 2^24 that is the integer quotient exactly), at
 * most the ice bank's maxLevel */
#define EF_ICE_BANK 34
EF_FN EfResult ef_ice_bank_level(const EfGame *g, const EfTables *t, int32_t *out) {
    if (!ef_improvement_known(t, EF_ICE_BANK)) return ef_err(EF_ERR_NO_ICE_BANK, 0);
    const EfImprovementData *d = &t->improvements[EF_ICE_BANK];
    int32_t cap = d->has_max_level ? d->max_level : 0;
    int32_t count = 0;
    for (int k = 0; k < g->tile_count; k++) {
        const EfTile *x = &ef_ctiles(g)[k];
        count += x->terrain_n == EF_ICE || (x->terrain_n != EF_WATER && x->terrain_n != EF_OCEAN && x->climate_n == EF_POLARIS_TRIBE);
    }
    int32_t level = count / 20;
    *out = level < cap ? level : cap;
    return ef_ok();
}

/* economy.rs level_for */
EF_BIG EfResult ef_level_for(const EfGame *g, const EfTables *t, int i, int32_t *out) {
    const EfTile *x = &ef_ctiles(g)[i];
    if (!x->has_improvement_n) { *out = 1; return ef_ok(); }
    const EfImprovement *m = &x->improvement;
    int32_t named = ef_improvement_known(t, m->kind_n) ? t->improvements[m->kind_n].named : EF_NAMED_NONE;
    int32_t level = named == EF_NAMED_LIGHTHOUSE ? x->explorers_len_n + 1 : m->level_n;
    const EfImprovementData *d = ef_economy_data(t, m->kind_n);
    if (!d) { *out = 1; return ef_ok(); }
    int32_t raw_cap = d->has_max_level ? d->max_level : 0;
    /* Fungi go one level higher for a tile owner with Microbes */
    int32_t cap = ef_wadd(raw_cap, named == EF_NAMED_FUNGI && ef_player_ability(g, t, x->owner_n, EF_AB_MICROBES));
    if (ef_has(d->abilities, EF_AB_PATINA)) {
        int32_t rate = d->has_growth_rate ? d->growth_rate : 0;
        int32_t age = ef_wsub(g->current_turn, m->founded_n);
        if (rate == 0 || (age == INT32_MIN && rate == -1)) return ef_err(EF_ERR_GROWTH_RATE_ZERO, 0);
        level = age / rate;
    } else if (d->adjacency_len > 0) {
        EF_TRY(ef_adjacency_bonus(g, i, d, &level));
    }
    if (named == EF_NAMED_ICEBANK) return ef_ice_bank_level(g, t, out);
    *out = raw_cap > 0 && level > cap ? cap : level;
    return ef_ok();
}

/* economy.rs update_level: the calculated level; one level action per step to it is queued */
EF_FN EfResult ef_update_level(EfGame *g, const EfTables *t, int32_t player, EfXy c, int32_t *out) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_LEVEL_TILE, 0);
    int32_t level;
    EF_TRY(ef_level_for(g, t, i, &level));
    if (ef_ctiles(g)[i].has_improvement_n) {
        int32_t now = ef_ctiles(g)[i].improvement.level_n;
        if (level < now) {
            for (int32_t k = 0; k < now - level; k++) {
                EfAction a = ef_action_at(EF_A_LEVEL_DOWN_IMPROVEMENT, player, c);
                a.i[0] = 1;
                EF_TRY(ef_push(g, a));
            }
        } else {
            for (int32_t k = 0; k < level - now; k++) EF_TRY(ef_push(g, ef_action_at(EF_A_LEVEL_UP_IMPROVEMENT, player, c)));
        }
    }
    *out = level;
    return ef_ok();
}

/* economy.rs surrounding: level actions for adjacency improvements around `c`, center included */
EF_BIG EfResult ef_surrounding(EfGame *g, const EfTables *t, int32_t player, EfXy c) {
    if (ef_tile_index(g, c) == EF_NONE) return ef_err(EF_ERR_SURROUNDING_TILE, 0);
    for (int32_t x = c.x - 1; x <= c.x + 1; x++) {
        for (int32_t y = c.y - 1; y <= c.y + 1; y++) {
            EfXy at = ef_xy(x, y);
            int32_t i = ef_tile_index(g, at);
            if (i == EF_NONE || !ef_ctiles(g)[i].has_improvement_n) continue;
            const EfImprovementData *d = ef_improvement_data(t, ef_ctiles(g)[i].improvement.kind_n);
            if (!d || d->adjacency_len == 0) continue;
            int32_t level;
            EF_TRY(ef_update_level(g, t, player, at, &level));
        }
    }
    return ef_ok();
}

#endif /* EF_ECONOMY_H */
