/* ruins.h - port of engine-fast/src/ruins.rs: ExamineRuins, its seeded reward choice and the
 * Ruins action. */
#ifndef EF_RUINS_H
#define EF_RUINS_H

#include "combat.h"

#define EF_RUIN_RESOURCES 1
#define EF_RUIN_POPULATION 2
#define EF_RUIN_EXPLORER 3
#define EF_RUIN_FREE_TECH 4
#define EF_RUIN_BATTLESHIP 6
#define EF_RUIN_SWORDSMAN 8

#define EF_RUIN_LOST_CITY 9
#define EF_RUIN_IMPROVEMENT 2

EF_FN int ef_land(int32_t terrain) { return terrain >= 3 && terrain <= 6; }

/* ruins.rs valid_lost_city: an unowned ocean tile off the board's edge with no city or ruin
 * within two tiles */
EF_FN int ef_valid_lost_city(const EfGame *g, int32_t i) {
    const EfTile *x = &ef_ctiles(g)[i];
    EfXy c = x->coordinates;
    int32_t h = g->tile_count / g->width;
    if (c.x <= 0 || c.y <= 0 || c.x >= g->width - 1 || c.y >= h - 1 || x->owner_n != 0 || x->terrain_n != EF_OCEAN) return 0;
    for (int32_t xx = c.x - 2 < 0 ? 0 : c.x - 2; xx <= (c.x + 2 > g->width - 1 ? g->width - 1 : c.x + 2); xx++)
        for (int32_t yy = c.y - 2 < 0 ? 0 : c.y - 2; yy <= (c.y + 2 > h - 1 ? h - 1 : c.y + 2); yy++) {
            if (xx == c.x && yy == c.y) continue;
            const EfTile *o = ef_tile_c(g, ef_xy(xx, yy));
            if (o->has_improvement_n && (o->improvement.kind_n == EF_CITY || o->improvement.kind_n == EF_RUIN_IMPROVEMENT)) return 0;
        }
    return 1;
}

/* ruins.rs select_reward: a seeded shuffle of the six rewards, the first valid one wins */
EF_BIG EfResult ef_select_reward(EfGame *g, const EfTables *t, int32_t p, EfXy c, int64_t *out) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_RUINS_TILE, 0);
    int pi;
    EF_TRY(ef_player(g, p, &pi));
    int32_t terrain = ef_ctiles(g)[i].terrain_n;
    if (!ef_land(terrain) && ef_tribe_has(t, g->players[pi].tribe, EF_AB_FLOODED) && ef_valid_lost_city(g, i)) {
        *out = EF_RUIN_LOST_CITY;
        return ef_ok();
    }
    int64_t candidates[6] = { EF_RUIN_RESOURCES, EF_RUIN_POPULATION, EF_RUIN_SWORDSMAN, EF_RUIN_BATTLESHIP, EF_RUIN_EXPLORER, EF_RUIN_FREE_TECH };
    uint32_t hash = (uint32_t)ef_random_hash(g);
    for (int32_t k = 0; k < 5; k++) {
        uint32_t words[3] = { (uint32_t)c.x, (uint32_t)c.y, (uint32_t)k };
        int32_t j = ef_hash_range_i32(ef_hash_words(hash, words, 3), k, 6);
        int64_t x = candidates[k]; candidates[k] = candidates[j]; candidates[j] = x;
    }
    int32_t h = g->tile_count / g->width;
    for (int n = 0; n < 6; n++) {
        int64_t candidate = candidates[n];
        int valid = 1;
        if (candidate == EF_RUIN_EXPLORER) {
            int count = 0;
            for (int32_t x = c.x - 1 < 0 ? 0 : c.x - 1; x <= (c.x + 1 > g->width - 1 ? g->width - 1 : c.x + 1); x++) {
                for (int32_t y = c.y - 1 < 0 ? 0 : c.y - 1; y <= (c.y + 1 > h - 1 ? h - 1 : c.y + 1); y++) {
                    if (x == c.x && y == c.y) continue;
                    uint32_t allowed;
                    EF_TRY(ef_route_movements(g, t, p, &allowed));
                    const EfTile *x0 = ef_tile_c(g, ef_xy(x, y));
                    count += ef_has_terrain(allowed, x0->terrain_n) || (x0->has_improvement_n && ef_improvement_has(t, x0->improvement.kind_n, EF_AB_BRIDGE));
                }
            }
            int unexplored = 0;
            for (int32_t x = c.x - 2 < 0 ? 0 : c.x - 2; x <= (c.x + 2 > g->width - 1 ? g->width - 1 : c.x + 2) && !unexplored; x++) {
                for (int32_t y = c.y - 2 < 0 ? 0 : c.y - 2; y <= (c.y + 2 > h - 1 ? h - 1 : c.y + 2); y++) {
                    if (x == c.x && y == c.y) continue;
                    const EfTile *x0 = ef_tile_c(g, ef_xy(x, y));
                    if (!ef_tile_explored(x0, p)) { unexplored = 1; break; }
                }
            }
            valid = count > 0 && unexplored;
        } else if (candidate == EF_RUIN_FREE_TECH) {
            int32_t list[EF_MAX_TECHS], len = 0;
            EF_TRY(ef_unlockable_techs(g, t, p, list, &len));
            valid = len > 0;
        } else if (candidate == EF_RUIN_BATTLESHIP) {
            valid = 0;
            if (terrain == EF_WATER || terrain == EF_OCEAN) {
                uint32_t allowed;
                EF_TRY(ef_route_movements(g, t, p, &allowed));
                valid = ef_has_terrain(allowed, EF_OCEAN);
            }
        } else if (candidate == EF_RUIN_SWORDSMAN) {
            valid = 0;
            if (terrain != EF_WATER && terrain != EF_OCEAN) {
                for (int k = 0; k < g->player_count; k++)
                    if (g->players[k].id != p) valid |= ef_alive(g, g->players[k].id);
            }
        }
        if (valid) { *out = candidate; return ef_ok(); }
    }
    *out = EF_RUIN_RESOURCES;
    return ef_ok();
}

/* ruins.rs examine_command */
EF_FN EfResult ef_examine_command(EfGame *g, const EfTables *t, int32_t p, EfXy c) {
    EF_TRY(ef_base(g, p, 14));
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_RUINS_TILE, 0);
    int32_t unit = tile->unit_n;
    int64_t reward;
    EF_TRY(ef_select_reward(g, t, p, c, &reward));
    EfAction a = ef_action_at(EF_A_RUINS, p, c);
    a.w[0] = reward;
    EF_TRY(ef_push(g, a));
    if (unit != EF_NONE && ef_unit_effect(g, &ef_cunits(g)[unit], EF_INVISIBLE_EFFECT)) {
        EfAction r = ef_action_at(EF_A_REVEAL, p, c);
        r.p[1] = ef_xy(-1, -1);
        EF_TRY(ef_push(g, r));
    }
    return ef_ok();
}

/* ruins.rs ruins (the Ruins action) */
EF_BIG EfResult ef_ruins(EfGame *g, const EfTables *t, int32_t p, int64_t reward, EfXy c) {
    int pi = ef_player_index(g, p);
    if (pi == EF_NONE) return ef_ok();
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_RUINS_TILE, 0);
    ef_tiles(g)[i].has_improvement_n = 0; /* narrow: a flag */
    memset(&ef_tiles(g)[i].improvement, 0, sizeof ef_tiles(g)[i].improvement);
    if (ef_ctiles(g)[i].unit_n != EF_NONE) EF_TRY(ef_make_exhausted(g, ef_ctiles(g)[i].unit_n));
    switch (reward) {
    case EF_RUIN_RESOURCES:
        for (int remaining = 10; remaining >= 1; remaining--) {
            EfAction a = ef_action_at(EF_A_INCREASE_CURRENCY, p, c);
            a.i[0] = 1;
            a.i[1] = remaining == 1 ? 190 : 40;
            EF_TRY(ef_push(g, a));
        }
        return ef_ok();
    case EF_RUIN_POPULATION: {
        EfXy capital;
        EF_TRY(ef_current_capital(g, g->players[pi].id, &capital));
        if (!(capital.x == -1 && capital.y == -1)) {
            const int32_t delays[3] = { 0, 60, 120 };
            for (int k = 0; k < 3; k++) {
                EfAction a = ef_action_at(EF_A_INCREASE_POPULATION, p, c);
                a.p[1] = capital;
                a.i[0] = delays[k];
                EF_TRY(ef_push(g, a));
            }
        }
        return ef_ok();
    }
    case EF_RUIN_EXPLORER: {
        g->current_unit_id += 1u;
        uint32_t words[2] = { (uint32_t)c.x, (uint32_t)c.y };
        uint32_t seed = ef_hash_words((uint32_t)ef_random_hash(g), words, 2);
        EfAction a = ef_action_at(EF_A_SCOUT, p, c);
        a.i[0] = (int32_t)g->current_unit_id;
        a.w[0] = 12;
        a.i[1] = (int32_t)seed;
        return ef_push(g, a);
    }
    case EF_RUIN_FREE_TECH: {
        int32_t list[EF_MAX_TECHS], len = 0;
        EF_TRY(ef_unlockable_techs(g, t, p, list, &len));
        if (len == 0) return ef_err(EF_ERR_RUIN_TECH, 0);
        uint32_t words[2] = { (uint32_t)c.x, (uint32_t)c.y };
        int32_t choice = ef_hash_range_i32(ef_hash_words((uint32_t)ef_random_hash(g), words, 2), 0, len);
        EfAction a = ef_action(EF_A_RESEARCH, p);
        a.w[0] = list[choice];
        a.i[0] = 0;
        return ef_push(g, a);
    }
    case 5: case 6: case 7: case 8: {
        int32_t kind = reward == 5 ? 12 : reward == 6 ? 44 : reward == 7 ? 16 : 11;
        if (reward == EF_RUIN_BATTLESHIP || reward == EF_RUIN_SWORDSMAN) EF_TRY(ef_push(g, ef_action_at(EF_A_PROMOTE, p, c)));
        return ef_train_on_occupied(g, t, p, kind, c);
    }
    case EF_RUIN_LOST_CITY: return ef_lost_city(g, t, p, pi, c);
    default: return ef_err(EF_ERR_RUINS_REWARD, reward);
    }
}

#endif /* EF_RUINS_H */
