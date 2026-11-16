/* reward.h - port of engine-fast/src/reward.rs: the CityReward command and action, Modify,
 * ExpandCity and the explorer's scout walk (its fog map over an 11 x 11 window). */
#ifndef EF_REWARD_H
#define EF_REWARD_H

#include "units.h"
#include "routes.h"

#define EF_LIGHTHOUSE_KIND 47
#define EF_SCOUT_RADIUS 5
#define EF_SCOUT_WIDTH 11
#define EF_SCOUT_SIZE 121

/* reward.rs reward_command: the level-up trigger is consumed first */
EF_FN EfResult ef_reward_command(EfGame *g, int32_t p, int32_t reward, EfXy target) {
    int n = -1;
    for (int k = g->triggers_len - 1; k >= 0; k--)
        if (g->triggers[k].player == p && g->triggers[k].kind == EF_CITY_LEVEL_UP) { n = k; break; }
    if (n < 0) return ef_err(EF_ERR_REWARD_TRIGGER, 0);
    for (int k = n; k + 1 < g->triggers_len; k++) g->triggers[k] = g->triggers[k + 1];
    g->triggers_len -= 1;
    EF_TRY(ef_base(g, p, 11));
    EfAction a = ef_action_at(EF_A_CITY_REWARD, p, target);
    a.i[0] = reward;
    return ef_push(g, a);
}

/* reward.rs city_reward (the CityReward action) */
EF_BIG EfResult ef_city_reward(EfGame *g, const EfTables *t, int32_t p, int64_t reward, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_ok();
    if (!ef_ctiles(g)[i].has_improvement_n || ef_player_index(g, p) == EF_NONE) return ef_ok();
    if (reward < INT32_MIN || reward > INT32_MAX) return ef_err(EF_ERR_REWARD_RANGE, 0);
    int32_t r = (int32_t)reward;
    if (r < 0 || r >= 14) return ef_err(EF_ERR_COUNT_OUTSIDE, r);
    if (ef_ctiles(g)[i].improvement.rewards_n[r] >= 255) return ef_err(EF_ERR_COUNT_FULL, r);
    ef_tiles(g)[i].improvement.rewards_n[r] += 1; /* narrow: below 255, checked above */
    if (r == 3 || r == 2) {
        EfAction a = ef_action_at(EF_A_MODIFY_PRODUCTION, p, c);
        a.i[0] = 1;
        EF_TRY(ef_push(g, a));
    }
    switch (r) {
    case 2: {
        EfAction a = ef_action_at(EF_A_MODIFY_SCORE, p, c);
        a.i[0] = 250;
        EF_TRY(ef_push(g, a));
        EfAction s = ef_action_at(EF_A_INCREASE_SCORE, p, c);
        s.i[0] = 250;
        s.i[1] = 50;
        return ef_push(g, s);
    }
    case 4: {
        g->current_unit_id += 1u;
        uint32_t words[2] = { (uint32_t)c.x, (uint32_t)c.y };
        uint32_t seed = ef_hash_words((uint32_t)ef_random_hash(g), words, 2);
        EfAction a = ef_action_at(EF_A_SCOUT, p, c);
        a.i[0] = (int32_t)g->current_unit_id;
        a.w[0] = 12;
        a.i[1] = (int32_t)seed;
        a.i[2] = 0;
        a.len = 0;
        return ef_push(g, a);
    }
    case 5: {
        EF_PUT(ef_tiles(g)[i].improvement.border_size_n, ef_tiles(g)[i].improvement.border_size_n + 1, int8_t);
        int32_t others[EF_MAX_PLAYERS], n = 0;
        for (int k = 0; k < g->player_count; k++) {
            int32_t q = g->players[k].id;
            if (q != EF_NATURE && q != p) others[n++] = q;
        }
        int pi;
        EF_TRY(ef_player(g, p, &pi));
        /* pushed in reverse of the game's buffer, which ends with Polaris's freeze */
        if (g->players[pi].tribe == EF_POLARIS_TRIBE) {
            EfAction f = ef_action_at(EF_A_FREEZE_BORDER, p, c);
            f.i[0] = ef_ctiles(g)[i].improvement.border_size_n;
            f.i[1] = 0;
            f.i[2] = 1;
            EF_TRY(ef_push(g, f));
        }
        EfAction u = ef_action(EF_A_UPDATE_CITY_CONNECTIONS, p);
        u.len = n;
        for (int k = 0; k < n; k++) u.list.players[k] = others[k];
        EF_TRY(ef_push(g, u));
        EF_TRY(ef_push(g, ef_action(EF_A_UPDATE_ROUTES, p)));
        return ef_push(g, ef_action_at(EF_A_EXPAND_CITY, p, c));
    }
    case 6: return ef_train_on_occupied(g, t, p, EF_GIANT, c);
    case 7: {
        const int32_t delays[5] = { 40, 40, 40, 40, 190 };
        for (int k = 0; k < 5; k++) {
            EfAction a = ef_action_at(EF_A_INCREASE_CURRENCY, p, c);
            a.i[0] = 1;
            a.i[1] = delays[k];
            EF_TRY(ef_push(g, a));
        }
        return ef_ok();
    }
    case 8:
        for (int k = 0; k < 3; k++) {
            EfAction a = ef_action_at(EF_A_INCREASE_POPULATION, p, c);
            a.p[1] = c;
            a.i[0] = 0;
            EF_TRY(ef_push(g, a));
        }
        return ef_ok();
    case 1:
    case 3: return ef_ok();
    default: return ef_err(EF_ERR_CITY_REWARD_KIND, r);
    }
}

/* reward.rs modify: ModifyProduction / ModifyScore on the target improvement (u16 arithmetic) */
EF_FN EfResult ef_modify(EfGame *g, int production, EfXy target, int32_t amount) {
    int32_t i = ef_tile_index(g, target);
    if (i == EF_NONE) return ef_err(EF_ERR_MODIFY_TILE, 0);
    if (!ef_ctiles(g)[i].has_improvement_n) return ef_err(EF_ERR_MODIFY_NONE, 0);
    EfImprovement *m = &ef_tiles(g)[i].improvement;
    uint16_t *field = production ? &m->production_n : &m->base_score_n;
    *field = (uint16_t)(*field + (uint16_t)(uint32_t)amount); /* narrow: the engine's u16 wrapping */
    return ef_ok();
}

/* reward.rs expand_city */
EF_BIG EfResult ef_expand_city(EfGame *g, const EfTables *t, int32_t actor, EfXy city) {
    int32_t center = ef_tile_index(g, city);
    if (center == EF_NONE) return ef_err(EF_ERR_EXPAND_TILE, 0);
    if (!ef_ctiles(g)[center].has_improvement_n) return ef_err(EF_ERR_EXPAND_NONE, 0);
    int32_t radius = ef_ctiles(g)[center].improvement.border_size_n;
    int pi;
    EF_TRY(ef_player(g, actor, &pi));
    int32_t h = g->tile_count / g->width;
    int32_t x0 = city.x - radius < 0 ? 0 : city.x - radius, x1 = city.x + radius > g->width - 1 ? g->width - 1 : city.x + radius;
    int32_t y0 = city.y - radius < 0 ? 0 : city.y - radius, y1 = city.y + radius > h - 1 ? h - 1 : city.y + radius;
    /* the tiles claimed, marked by index; the second pass walks the box in the same order */
    uint32_t claimed[EF_BIT_WORDS(EF_MAX_TILES)];
    memset(claimed, 0, sizeof claimed);
    for (int32_t xx = x0; xx <= x1; xx++) {
        for (int32_t yy = y0; yy <= y1; yy++) {
            int32_t i = ef_tile_index(g, ef_xy(xx, yy));
            EfXy r = ef_ctiles(g)[i].ruling_city;
            if (!((r.x == -1 && r.y == -1) || ef_xy_eq(r, city))) continue;
            if (ef_ctiles(g)[i].owner_n == actor) continue;
            int32_t amount = 20;
            if (ef_ctiles(g)[i].has_improvement_n) amount = ef_wadd(amount, ef_improvement_score(t, g, i));
            EfAction a = ef_action_at(EF_A_INCREASE_SCORE, actor, ef_ctiles(g)[i].coordinates);
            a.i[0] = amount;
            a.i[1] = 100;
            EF_TRY(ef_push(g, a));
            EF_PUT(ef_tiles(g)[i].owner_n, actor, uint8_t);
            ef_tiles(g)[i].ruling_city = city;
            ef_bit_set(claimed, i);
        }
    }
    for (int32_t xx = x0; xx <= x1; xx++) {
        for (int32_t yy = y0; yy <= y1; yy++) {
            int32_t i = ef_tile_index(g, ef_xy(xx, yy));
            if (!ef_bit(claimed, i)) continue;
            const EfTile *tile = &ef_ctiles(g)[i];
            int32_t population = 0;
            if (tile->has_improvement_n && ef_improvement_known(t, tile->improvement.kind_n))
                population = ef_population_at_level(&t->improvements[tile->improvement.kind_n], tile->improvement.level_n);
            for (int32_t q = 0; q < population; q++) {
                EfAction a = ef_action_at(EF_A_INCREASE_POPULATION, actor, tile->coordinates);
                a.p[1] = city;
                a.i[0] = 60;
                EF_TRY(ef_push(g, a));
            }
        }
    }
    EfArea area;
    EF_TRY(ef_area_sorted(g, city, radius, &area));
    int32_t sorted[EF_AREA_MAX], ns = 0;
    for (int k = 0; k < area.len; k++) {
        EfXy r = ef_ctiles(g)[area.tiles[k]].ruling_city;
        if ((r.x == -1 && r.y == -1) || ef_xy_eq(r, city)) sorted[ns++] = area.tiles[k];
    }
    EF_TRY(ef_claim_area(g, t, actor, pi, sorted, ns));
    for (int32_t xx = x0; xx <= x1; xx++) {
        for (int32_t yy = y0; yy <= y1; yy++) {
            EfXy c = ef_xy(xx, yy);
            const EfTile *x = ef_tile_c(g, c);
            if (!ef_contains_u8(x->explorers_n, x->explorers_len_n, actor)) EF_TRY(ef_push(g, ef_action_at(EF_A_EXPLORE, actor, c)));
        }
    }
    return ef_ok();
}

EF_FN int32_t ef_fog_score(const uint32_t *lookup, int index, int radius) {
    int x = index % EF_SCOUT_WIDTH, y = index / EF_SCOUT_WIDTH;
    int height = EF_SCOUT_SIZE / EF_SCOUT_WIDTH;
    int32_t fog = 0;
    int lighthouse = 0;
    int y0 = y - radius < 0 ? 0 : y - radius, y1 = y + radius > height - 1 ? height - 1 : y + radius;
    int x0 = x - radius < 0 ? 0 : x - radius, x1 = x + radius > EF_SCOUT_WIDTH - 1 ? EF_SCOUT_WIDTH - 1 : x + radius;
    for (int yy = y0; yy <= y1; yy++) {
        for (int xx = x0; xx <= x1; xx++) {
            uint32_t f = lookup[yy * EF_SCOUT_WIDTH + xx];
            fog += (f & 8u) != 0;
            lighthouse |= (f & 0x28u) == 0x28u;
        }
    }
    if (radius == 2 && fog == 0 && !lighthouse) return 9999;
    return (lighthouse ? 40 : 73) - 10 * (fog < 4 ? fog : 4);
}

EF_FN EfXy ef_scout_coordinate(EfXy center, int index) {
    return ef_xy(center.x - EF_SCOUT_RADIUS + index % EF_SCOUT_WIDTH, center.y - EF_SCOUT_RADIUS + index / EF_SCOUT_WIDTH);
}

EF_FN void ef_scout_lookup(const EfGame *g, const EfTables *t, int32_t p, EfXy center, uint32_t allowed, uint32_t *lookup) {
    for (int index = 0; index < EF_SCOUT_SIZE; index++) {
        const EfTile *tile = ef_tile_c(g, ef_scout_coordinate(center, index));
        if (!tile) { lookup[index] = 0; continue; }
        int explored = p == EF_NATURE || ef_contains_u8(tile->explorers_n, tile->explorers_len_n, p);
        int has = tile->has_improvement_n;
        int32_t kind = tile->improvement.kind_n;
        int city = has && kind == EF_CITY;
        int route = has && ef_improvement_has(t, kind, EF_AB_BRIDGE);
        int movable = (city || route) ? city : ef_has_terrain(allowed, tile->terrain_n);
        lookup[index] = 1u | (movable ? 6u : 0u) | (!explored ? 8u : 0u) | (route ? 16u : 0u)
                        | ((has && kind == EF_LIGHTHOUSE_KIND) ? 32u : 0u) | ((explored && tile->terrain_n == EF_MOUNTAIN) ? 64u : 0u);
    }
}

/* reward.rs can_cross */
EF_FN EfResult ef_scout_can_cross(EfGame *g, int32_t p, EfXy center, int from, int to, uint32_t allowed, int *out) {
    EfXy origin = ef_scout_coordinate(center, from), target = ef_scout_coordinate(center, to);
    const EfTile *tp = ef_tile_c(g, target);
    if (!tp) return ef_err(EF_ERR_SCOUT_TILE, 0);
    EfTile tile = *tp;
    int pi;
    EF_TRY(ef_player(g, p, &pi));
    EfRelation *r;
    if (tile.owner_n != 0 && tile.owner_n != p) EF_TRY(ef_relation_entry(g, pi, tile.owner_n, &r));
    *out = 0;
    if (!ef_contains_u8(tile.explorers_n, tile.explorers_len_n, p)) return ef_ok();
    if (tile.unit_n != EF_NONE) {
        int32_t owner = ef_cunits(g)[tile.unit_n].owner_n;
        if (owner != p && ef_unit_effect(g, &ef_cunits(g)[tile.unit_n], EF_INVISIBLE_EFFECT)) EF_TRY(ef_relation_entry(g, pi, owner, &r));
    }
    if (tile.has_improvement_n && tile.improvement.kind_n == EF_BRIDGE && !ef_has_terrain(allowed, tile.terrain_n)) {
        const EfTile *n;
#define EF_SANCHOR(dx, dy) ((n = ef_tile_c(g, ef_xy(target.x + (dx), target.y + (dy)))) && n->terrain_n != 1 && n->terrain_n != 2)
        int vertical = EF_SANCHOR(0, -1) && EF_SANCHOR(0, 1);
        int horizontal = !vertical && EF_SANCHOR(1, 0) && EF_SANCHOR(-1, 0);
#undef EF_SANCHOR
        if ((vertical && origin.y == target.y && origin.x != target.x) || (horizontal && origin.x == target.x && origin.y != target.y))
            return ef_ok();
    }
    *out = 1;
    return ef_ok();
}

/* reward.rs fog_map */
EF_BIG EfResult ef_fog_map(EfGame *g, int32_t p, EfXy center, uint32_t allowed, uint32_t *lookup, int32_t *map) {
    uint8_t queue[EF_SCOUT_SIZE + 499 * 8];
    int queued = 0;
    for (int i = 0; i < EF_SCOUT_SIZE; i++) map[i] = (lookup[i] & 2u) ? 1000 : 9999;
    for (int i = 0; i < EF_SCOUT_SIZE; i++)
        if (lookup[i] & 8u) { map[i] = 0; queue[queued++] = (uint8_t)i; }
    for (int i = 0; i < EF_SCOUT_SIZE; i++) {
        if ((lookup[i] & 0x4au) == 0x42u) {
            int32_t score = ef_fog_score(lookup, i, 2);
            if (score != 9999) {
                map[i] = score + 100;
                queue[queued++] = (uint8_t)i;
                lookup[i] |= 128u;
            }
        }
    }
    for (int cursor = 0; cursor < queued && cursor <= 498; cursor++) {
        int from = queue[cursor];
        int32_t step0 = map[from] + 100;
        int32_t index = from;
        int32_t right = from % EF_SCOUT_WIDTH < EF_SCOUT_WIDTH - 1 ? 1 : 9999;
        int32_t left = from % EF_SCOUT_WIDTH > 0 ? -1 : -9999;
        int32_t tos[8] = { index + 11, index - 11, index + right, index + left, index + 11 + left, index + 11 + right,
                           index - 11 + left, index - 11 + right };
        for (int k = 0; k < 8; k++) {
            int32_t to = tos[k];
            if (to < 0 || to >= EF_SCOUT_SIZE) continue;
            int32_t step = step0;
            if (map[to] <= step || (lookup[to] & 128u)) continue;
            int crossed = 0;
            if (lookup[to] & 16u) EF_TRY(ef_scout_can_cross(g, p, center, from, to, allowed, &crossed));
            if (!crossed && (3u & ~lookup[to]) != 0) continue;
            if (step == 100 && !(lookup[to] & 8u)) step = ef_fog_score(lookup, to, 1) + 100;
            map[to] = step;
            queue[queued++] = (uint8_t)to;
        }
    }
    return ef_ok();
}

/* reward.rs scout: one step toward fog, the next one queued */
EF_BIG EfResult ef_scout(EfGame *g, const EfTables *t, const EfAction *a) {
    int32_t p = a->player;
    uint32_t unit = (uint32_t)a->i[0];
    int64_t remaining = a->w[0];
    uint32_t action_seed = (uint32_t)a->i[1];
    EfXy from = a->p[0];
    if (a->i[2] != 0) return ef_err(EF_ERR_TUTORIAL_SCOUT, 0);
    EfXy path[EF_PATH_TILES];
    int32_t path_len = a->len;
    for (int k = 0; k < path_len; k++) path[k] = a->list.path[k];
    if (path_len == 0) path[path_len++] = from;
    uint32_t allowed;
    EF_TRY(ef_route_movements(g, t, p, &allowed));
    uint32_t lookup[EF_SCOUT_SIZE];
    int32_t map[EF_SCOUT_SIZE];
    ef_scout_lookup(g, t, p, from, allowed, lookup);
    EF_TRY(ef_fog_map(g, p, from, allowed, lookup, map));
    /* the backtracking penalty */
    int32_t penalty = 1;
    for (int k = 0; k < path_len; k++) {
        int32_t dx = path[k].x - (from.x - EF_SCOUT_RADIUS), dy = path[k].y - (from.y - EF_SCOUT_RADIUS);
        if (dx >= 0 && dx < 11 && dy >= 0 && dy < 11) map[dy * EF_SCOUT_WIDTH + dx] += penalty;
        penalty += path_len < 9 ? 1 : (k & 1);
    }
    const int8_t ox[8] = { -1, -1, -1, 0, 0, 1, 1, 1 };
    const int8_t oy[8] = { -1, 0, 1, -1, 1, -1, 0, 1 };
    int32_t best = INT32_MAX;
    int candidates[8], nc = 0;
    for (int k = 0; k < 8; k++) {
        int32_t score = map[60 + ox[k] + oy[k] * 11];
        if (score < best) { best = score; nc = 0; candidates[nc++] = k; }
        else if (score == best) candidates[nc++] = k;
    }
    if (best > 9998 || nc == 0) return ef_ok();
    int32_t hash = ef_random_hash(g);
    int pick = 0;
    if (nc > 1) {
        uint32_t word = action_seed;
        pick = ef_hash_range_i32(ef_hash_words((uint32_t)hash, &word, 1), 0, nc);
    }
    EfXy to = ef_xy(from.x + ox[candidates[pick]], from.y + oy[candidates[pick]]);
    const EfTile *target = ef_tile_c(g, to);
    if (!target) return ef_ok();
    if (path_len >= EF_PATH_TILES) return ef_err(EF_ERR_SHORT_LIST_REFUSED, EF_PATH_TILES);
    path[path_len++] = to;
    int32_t sight = target->terrain_n == EF_MOUNTAIN ? 2 : 1;
    if (remaining >= 2) {
        uint32_t word = action_seed;
        EfAction next = ef_action_at(EF_A_SCOUT, p, to);
        next.i[0] = (int32_t)unit;
        next.w[0] = remaining - 1;
        next.i[1] = (int32_t)ef_hash_words((uint32_t)hash, &word, 1);
        next.i[2] = 0;
        next.len = path_len;
        for (int k = 0; k < path_len; k++) next.list.path[k] = path[k];
        EF_TRY(ef_push(g, next));
    }
    return ef_explore_from(g, p, to, sight);
}

#endif /* EF_REWARD_H */
