/* infiltrate.h - port of engine-fast/src/infiltrate.rs: the Infiltrate and InfiltrationReward
 * combat actions (a cloak's rebellion), with the placement and resource-visibility queries
 * they use. */
#ifndef EF_INFILTRATE_H
#define EF_INFILTRATE_H

#include "capture.h"

#define EF_REBELLION 11
#define EF_POISON_REWARD 13
#define EF_CLOAK 38
#define EF_POISON_UNIT 60   /* the unit a poison infiltration places */
#define EF_MOTH 61
#define EF_REBELLION_EFFECT 1   /* improvement effect */
#define EF_MAX_REBEL_SPOTS ((2 * 5 + 1) * (2 * 5 + 1))

/* infiltrate.rs infiltrate (the Infiltrate action) */
EF_FN EfResult ef_infiltrate(EfGame *g, const EfTables *t, int32_t p, EfXy origin, EfXy target) {
    const EfTile *tile = ef_tile_c(g, origin);
    if (!tile) return ef_err(EF_ERR_INFILTRATE_ORIGIN, 0);
    /* a Cloak when no unit stands on the origin */
    int32_t kind = tile->unit_n == EF_NONE ? EF_CLOAK : ef_cunits(g)[tile->unit_n].kind_n;
    EF_TRY(ef_kill(g, t, origin));
    EfAction a = ef_action_at(EF_A_INFILTRATION_REWARD, p, target);
    /* the Moth's infiltration poisons the city instead of raising a rebellion */
    a.w[0] = kind == EF_MOTH ? EF_POISON_REWARD : EF_REBELLION;
    a.i[0] = kind;
    return ef_push(g, a);
}

/* infiltrate.rs resource_revealed: whether the player's tech (its unlocked-tech cache, then
 * the techs unlockable next) reveals resource `kind`; refreshes the cache */
EF_BIG EfResult ef_resource_revealed(EfGame *g, const EfTables *t, int32_t p, int32_t kind, int *out) {
    *out = 0;
    int i = ef_player_index(g, p);
    if (i == EF_NONE) return ef_ok();
    EF_TRY(ef_refresh_tech_cache(g, t, p));
    if (kind == 0) return ef_ok();
    const EfTechTree *tree = ef_tribe_tree(t, g->players[i].tribe);
    if (!tree) return ef_err(EF_ERR_UNKNOWN_TRIBE, 0);
    const EfPlayer *x = &g->players[i];
    if (!x->has_tech_cache) return ef_err(EF_ERR_NULL_TECH_CACHE, 0);
    for (int k = 0; k < x->tech_cache_len; k++) {
        int32_t tech = x->tech_cache[k];
        if (tech < 0 || tech >= t->tech_count) return ef_err(EF_ERR_UNKNOWN_TECH, 0);
        if (tree->links[tech].reveals_error) return ef_err(EF_ERR_TABLE, tree->links[tech].reveals_error - 1);
        if (kind >= 0 && kind < 32 && ((tree->links[tech].reveals >> kind) & 1)) {
            *out = 1;
            return ef_ok();
        }
    }
    int32_t list[EF_MAX_TECHS], n;
    EF_TRY(ef_unlockable_techs(g, t, p, list, &n));
    for (int k = 0; k < n; k++) {
        int32_t tech = list[k];
        if (tree->links[tech].reveals_error) return ef_err(EF_ERR_TABLE, tree->links[tech].reveals_error - 1);
        if (kind >= 0 && kind < 32 && ((tree->links[tech].reveals >> kind) & 1)) {
            *out = 1;
            return ef_ok();
        }
    }
    return ef_ok();
}

/* infiltrate.rs visible_resource: whether tile c shows the player a resource */
EF_FN EfResult ef_visible_resource(EfGame *g, const EfTables *t, int32_t p, EfXy c, int *out) {
    *out = 0;
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_RESOURCE_TILE, 0);
    if ((p != EF_NATURE && !ef_tile_explored(tile, p)) || tile->has_improvement_n) return ef_ok();
    if (!tile->has_resource_n) return ef_ok();
    return ef_resource_revealed(g, t, p, tile->resource_n, out);
}

/* infiltrate.rs rebellion_weight */
EF_BIG EfResult ef_rebellion_weight(EfGame *g, const EfTables *t, int32_t p, EfXy c, float *out) {
    uint32_t seed = (uint32_t)ef_random_hash(g);
    uint32_t words[3] = { (uint32_t)c.x, (uint32_t)c.y, (uint32_t)g->current_turn };
    uint32_t h = ef_hash_words(seed, words, 3);
    float weight = ef_rebellion_jitter(ef_hash_words(seed, &h, 1));
    const EfTile *tile0 = ef_tile_c(g, c);
    if (!tile0) return ef_err(EF_ERR_REBELLION_TILE, 0);
    EfTile tile = *tile0;
    if (tile.has_improvement_n && tile.improvement.kind_n == EF_CITY) weight += 2.0f;
    if (tile.terrain_n == EF_WATER || tile.terrain_n == EF_OCEAN) weight += -0.1f;
    int visible;
    EF_TRY(ef_visible_resource(g, t, p, c, &visible));
    if (visible) weight += 0.5f;
    EF_TRY(ef_refresh_tech_cache(g, t, p));
    int i;
    EF_TRY(ef_player(g, p, &i));
    const EfPlayer *x = &g->players[i];
    if (!x->has_tech_cache) return ef_err(EF_ERR_NULL_TECH_CACHE, 0);
    int32_t terrain = tile.terrain_n;
    if (terrain < 0 || terrain >= t->terrain_count || !t->terrain_known[terrain]) return ef_err(EF_ERR_CATALOG_ENTRY, terrain);
    for (int k = 0; k < x->tech_cache_len; k++) {
        const EfTechData *d = ef_tech_data(t, x->tech_cache[k]);
        if (!d) return ef_err(EF_ERR_UNKNOWN_TECH, 0);
        if (d->defence_terrains & ef_terrain_bit(terrain)) {
            if (d->defence_value[terrain] > 1) weight += 1.0f;
            break;
        }
    }
    if (tile.has_improvement_n && ef_improvement_has(t, tile.improvement.kind_n, EF_AB_POISON)
        && !ef_tribe_has(t, x->tribe, EF_AB_POISONRESIST))
        weight += -0.5f;
    *out = weight;
    return ef_ok();
}

/* infiltrate.rs raw_production */
EF_FN EfResult ef_raw_production(const EfGame *g, const EfTables *t, EfXy c, int32_t *out) {
    *out = 0;
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_PRODUCTION_TILE, 0);
    if (!tile->has_improvement_n) return ef_ok();
    const EfImprovement *m = &tile->improvement;
    const EfImprovementData *d = m->kind_n != 0 ? ef_improvement_data(t, m->kind_n) : 0;
    if (!d) return ef_ok();
    int32_t result = 0;
    if (m->kind_n == EF_CITY && tile->owner_n != 0) {
        int pi = ef_player_index(g, tile->owner_n);
        if (pi == EF_NONE) return ef_ok();
        result = m->level_n + m->production_n - !ef_xy_eq(g->players[pi].start_tile, c);
    }
    int32_t work = d->has_work ? d->work : 0;
    if (work > 0 && d->adjacency_len > 0) {
        int32_t r = work * m->level_n;
        result = r > 0 ? r : 0;
    }
    *out = result;
    return ef_ok();
}

/* infiltrate.rs profile_unit: a unit of `kind` for placement and battle, never on the board
 * (its effect list is the empty one) */
EF_FN EfResult ef_profile_unit(const EfTables *t, int32_t p, int32_t kind, EfXy c, EfUnit *u) {
    const EfUnitData *d = ef_unit_data(t, kind);
    if (!d) return ef_err(EF_ERR_PLACEMENT_KIND, 0);
    memset(u, 0, sizeof *u);
    u->used_n = 1; /* narrow: a flag or constant */
    EF_PUT(u->owner_n, p, uint8_t);
    EF_PUT(u->kind_n, kind, int8_t);
    u->coordinates = c;
    u->home = c;
    u->passenger_n = EF_NONE; /* narrow: a flag or constant */
    EF_PUT(u->health_n, d->health, uint16_t);
    u->effects_n = 0; /* narrow: a flag or constant */
    return ef_ok();
}

/* movement.rs placement_accessible */
EF_BIG EfResult ef_placement_with(const EfGame *g, const EfTables *t, const EfUnit *unit, EfXy c, EfMover *mover, int *ok) {
    EF_TRY(ef_mover_context(g, t, unit->owner_n, mover));
    EfProfile s;
    int32_t range;
    EF_TRY(ef_profile_with(g, t, unit, mover, &s, &range));
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_PLACEMENT_TILE, 0);
    *ok = ef_profile_accessible(&s, tile, tile);
    return ef_ok();
}

/* infiltrate.rs placement: movement.rs placement_accessible after the tech-cache refresh */
EF_BIG EfResult ef_placement(EfGame *g, const EfTables *t, const EfUnit *unit, EfXy c, EfMover *mover, int *ok) {
    EF_TRY(ef_refresh_tech_cache(g, t, unit->owner_n));
    return ef_placement_with(g, t, unit, c, mover, ok);
}

/* infiltrate.rs reward (the InfiltrationReward action): the city's production as stars, the
 * rebellion effect, daggers in the best spots of its border, then the battle with its unit */
EF_BIG EfResult ef_infiltration_reward(EfGame *g, const EfTables *t, int32_t actor, int64_t reward, EfXy city, int32_t kind) {
    if (reward != EF_REBELLION && reward != EF_POISON_REWARD) return ef_err(EF_ERR_INFILTRATION_REWARD, reward);
    int poison = reward == EF_POISON_REWARD;
    int32_t ci = ef_tile_index(g, city);
    if (ci == EF_NONE || !ef_ctiles(g)[ci].has_improvement_n) return ef_ok();
    EfImprovement improvement = ef_ctiles(g)[ci].improvement;
    int pi = ef_player_index(g, actor);
    if (pi == EF_NONE) return ef_ok();
    EfUnit temporary;
    EF_TRY(ef_profile_unit(t, actor, kind, city, &temporary));
    temporary.home = ef_xy(0, 0);
    temporary.direction_n = 7; /* narrow: a flag or constant */
    int32_t production;
    EF_TRY(ef_raw_production(g, t, city, &production));
    if (!poison) {
        EfAction stars = ef_action_at(EF_A_INCREASE_CURRENCY, actor, city);
        stars.i[0] = production;
        stars.i[1] = 10;
        EF_TRY(ef_push(g, stars));
    }
    EfImprovement *m = &ef_tiles(g)[ci].improvement;
    if (!ef_contains_i8(m->effects_n, m->effects_len_n, EF_REBELLION_EFFECT)) {
        if (m->effects_len_n >= 2) return ef_err(EF_ERR_SHORT_LIST_FULL, 2);
        m->effects_n[m->effects_len_n++] = EF_REBELLION_EFFECT; /* narrow: the constant 1, below 2 entries (checked above) */
    }
    int32_t here = ef_ctiles(g)[ci].unit_n;
    int has_defender = here != EF_NONE;
    EfUnit defender;
    int32_t dealt = 0;
    if (has_defender) {
        defender = ef_cunits(g)[here];
        int32_t back;
        int advance, reveal;
        EF_TRY(ef_battle_full(g, t, &temporary, &defender, &dealt, &back, &advance, &reveal));
    }
    int survives = has_defender && dealt < defender.health_n;
    if (survives && poison) {
        EfAction a = ef_action_at(EF_A_POISON, actor, city);
        a.p[1] = city;
        EF_TRY(ef_push(g, a));
    }
    /* the dagger (a poison reward's unit 60) as the actor's tribe trains it; the pirate is not
     * resolved, and a poison reward places no pirate */
    int32_t dagger;
    EF_TRY(ef_tribe_unit(t, g->players[pi].tribe, poison ? EF_POISON_UNIT : EF_DAGGER, &dagger));
    uint32_t movements;
    EF_TRY(ef_route_movements(g, t, actor, &movements));
    int water = ef_has_terrain(movements, EF_WATER) && !poison;
    EfUnit profile, sea;
    EF_TRY(ef_profile_unit(t, actor, dagger, city, &profile));
    if (water) EF_TRY(ef_profile_unit(t, actor, EF_PIRATE, city, &sea));
    EfMover mover;
    int32_t r = improvement.border_size_n;
    EfXy candidates[EF_MAX_REBEL_SPOTS + 1];
    int n = 0;
    for (int32_t xx = city.x - r; xx <= city.x + r; xx++) {
        for (int32_t yy = city.y - r; yy <= city.y + r; yy++) {
            EfXy c = ef_xy(xx, yy);
            if (ef_xy_eq(c, city)) continue;
            const EfTile *tile = ef_tile_c(g, c);
            if (!tile || !ef_xy_eq(tile->ruling_city, city) || tile->unit_n != EF_NONE) continue;
            int ok;
            EF_TRY(ef_placement(g, t, &profile, c, &mover, &ok));
            if (!ok && water) EF_TRY(ef_placement(g, t, &sea, c, &mover, &ok));
            if (ok) {
                if (n >= EF_MAX_REBEL_SPOTS) return ef_err(EF_ERR_REBEL_SPOTS_FULL, 0);
                candidates[n++] = c;
            }
        }
    }
    if (!survives) candidates[n++] = city;
    if (n > 1) {
        EfSlot slots[EF_MAX_REBEL_SPOTS + 1];
        for (int k = 0; k < n; k++) {
            float w;
            EF_TRY(ef_rebellion_weight(g, t, actor, candidates[k], &w));
            slots[k].key = -w;
            slots[k].slot = k;
        }
        ef_sort_slots(slots, n);
        EfXy sorted[EF_MAX_REBEL_SPOTS + 1];
        for (int k = 0; k < n; k++) sorted[k] = candidates[slots[k].slot];
        for (int k = 0; k < n; k++) candidates[k] = sorted[k];
    }
    int count = improvement.level_n < 5 ? improvement.level_n : 5;
    if (count > n) count = n;
    if (count < 0) count = 0;
    for (int k = count - 1; k >= 0; k--) {
        EfXy c = candidates[k];
        const EfTile *tile = ef_tile_c(g, c);
        if (water && tile && (tile->terrain_n == EF_WATER || tile->terrain_n == EF_OCEAN)) {
            int ok;
            EF_TRY(ef_placement(g, t, &profile, c, &mover, &ok));
            if (!ok) EF_TRY(ef_push(g, ef_action_at(EF_A_EMBARK, actor, c)));
        }
        EfAction train = ef_action_at(EF_A_TRAIN, actor, c);
        train.i[0] = dagger;
        train.i[1] = 0;
        train.p[1] = city;
        train.i[2] = 0;
        EF_TRY(ef_push(g, train));
    }
    if (has_defender) EF_TRY(ef_perform_attack(g, t, actor, ef_xy(-1, -1), city, dealt));
    return ef_ok();
}

#endif /* EF_INFILTRATE_H */
