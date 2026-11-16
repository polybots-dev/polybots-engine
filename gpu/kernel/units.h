/* units.h - port of engine-fast/src/units.rs: unit creation (with a vehicle's Warrior), the
 * Train action, Embark, Disembark, Upgrade, Promote, make_exhausted and the push aside of a
 * unit standing where one is trained. Units and effect lists take free slots of the block. */
#ifndef EF_UNITS_H
#define EF_UNITS_H

#include "economy.h"
#include "explore.h"
#include "movement.h"

#define EF_WARRIOR 2
#define EF_POLYTAUR 15
#define EF_GIANT 12
#define EF_DAGGER 37
#define EF_CLOAK_BOAT 39
#define EF_PIRATE 40
#define EF_TRANSPORTSHIP 43
#define EF_JUGGERNAUT 45

/* units.rs has: the kind's ability, an error for a kind the catalog lacks */
EF_FN EfResult ef_kind_has(const EfTables *t, int32_t kind, int ability, int *out) {
    const EfUnitData *d = ef_unit_data(t, kind);
    if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    *out = ef_has(d->abilities, ability);
    return ef_ok();
}

/* units.rs unit_score: five per star of the unit and its direct passenger */
EF_FN int32_t ef_unit_score(const EfGame *g, const EfTables *t, const EfUnit *u) {
    const EfUnitData *d = ef_unit_data(t, u->kind_n);
    int32_t total = d ? d->cost : 0;
    if (u->passenger_n != EF_NONE) {
        const EfUnitData *p = ef_unit_data(t, ef_cunits(g)[u->passenger_n].kind_n);
        total = ef_wadd(total, p ? p->cost : 0);
    }
    return ef_wmul(total, 5);
}

/* units.rs facing: toward the map centre */
EF_FN int32_t ef_facing(const EfGame *g, EfXy c) {
    const int8_t vx[8] = { -1, -1, -1, 0, 1, 1, 1, 0 };
    const int8_t vy[8] = { -1, 0, 1, 1, 1, 0, -1, -1 };
    int32_t w = g->width, h = g->tile_count / g->width;
    int32_t dx = w / 2 - c.x, dy = h / 2 - c.y;
    int32_t best = 0, score = 0;
    for (int i = 0; i < 8; i++) {
        int32_t weight = vx[i] * vx[i] + vy[i] * vy[i] == 1 ? 1393 : 985;
        int32_t next = ef_wmul(ef_wadd(ef_wmul(vx[i], dx), ef_wmul(vy[i], dy)), weight);
        if (score < next) { best = i; score = next; }
    }
    return best;
}

/* a free unit slot and a free effect-list slot (the Rust engine appends a node and a list) */
EF_FN EfResult ef_new_unit_slots(EfGame *g, int32_t *unit, int32_t *list) {
    *unit = EF_NONE;
    for (int u = 0; u < g->unit_cap; u++)
        if (!ef_cunits(g)[u].used_n) { *unit = u; break; }
    *list = EF_NONE;
    for (int l = 1; l <= g->unit_cap; l++)
        if (!ef_clists(g)[l].used_n) { *list = l; break; }
    if (*unit == EF_NONE || *list == EF_NONE) return ef_err(EF_ERR_UNITS_FULL, 0);
    return ef_ok();
}

/* units.rs create_unit; the passenger of a vehicle is made by
 * a second pass rather than a recursive call */
EF_BIG EfResult ef_create_unit(EfGame *g, const EfTables *t, int32_t player, int32_t kind0, EfXy c, int32_t *out) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_TRAIN_TILE, 0);
    int32_t nodes[2] = { EF_NONE, EF_NONE };
    int32_t kind = kind0;
    for (int depth = 0; depth < 2; depth++) {
        const EfTile *tile = &ef_ctiles(g)[i];
        int independent;
        int city = tile->has_improvement_n && tile->improvement.kind_n == EF_CITY;
        EfXy home = ef_xy(-1, -1);
        if (city) {
            EF_TRY(ef_kind_has(t, kind, EF_AB_INDEPENDENT, &independent));
            if (!independent) home = c;
        }
        int32_t climate = 0, skin = 0;
        if (kind == EF_POLYTAUR) { climate = tile->climate_n; skin = tile->skin_n; }
        const EfUnitData *d = ef_unit_data(t, kind);
        if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
        g->current_unit_id += 1u;
        int32_t slot, list;
        EF_TRY(ef_new_unit_slots(g, &slot, &list));
        ef_lists(g)[list].used_n = 1; /* narrow: a flag */
        ef_lists(g)[list].len_n = 0;  /* narrow: empty */
        EfUnit *u = &ef_units(g)[slot];
        memset(u, 0, sizeof *u);
        u->used_n = 1; /* narrow: a flag or constant */
        u->id = g->current_unit_id;
        EF_PUT(u->owner_n, player, uint8_t);
        EF_PUT(u->kind_n, kind, int8_t);
        u->coordinates = c;
        u->home = home;
        u->passenger_n = EF_NONE; /* narrow: a flag or constant */
        EF_PUT(u->health_n, d->health, uint16_t);
        u->moved_n = 1; /* narrow: a flag or constant */
        u->attacked_n = 1; /* narrow: a flag or constant */
        EF_PUT(u->direction_n, ef_facing(g, c), int8_t);
        EF_PUT(u->created_turn_n, g->current_turn, int16_t);
        u->effects_n = (int16_t)list; /* narrow: an effect-list slot (state.h assert) */
        EF_PUT(u->birth_climate_n, climate, int8_t);
        EF_PUT(u->birth_climate_skin_n, skin, int8_t);
        nodes[depth] = slot;
        int carry;
        EF_TRY(ef_kind_has(t, kind, EF_AB_CARRY, &carry));
        if (!carry || depth == 1) {
            if (depth == 1) {
                /* back in the vehicle's call: its passenger and, unless it protects, health */
                int protect;
                EF_TRY(ef_kind_has(t, kind0, EF_AB_PROTECT, &protect));
                EfUnit *v = &ef_units(g)[nodes[0]];
                v->passenger_n = (int16_t)nodes[1]; /* narrow: a unit slot (state.h assert) */
                if (!protect) v->health_n = ef_cunits(g)[nodes[1]].health_n; /* narrow: a copy of the same field */
            }
            break;
        }
        kind = EF_WARRIOR;
    }
    ef_tiles(g)[i].unit_n = (int16_t)nodes[0]; /* narrow: a unit slot (state.h assert) */
    /* the Rust's recursion ends the passenger's call first: per unit made, the game runs
     * the poison check (a poison improvement poisons it, unless its trainer's tribe resists or
     * it flies) and, for Tentacles, the tentacle zones */
    const EfTile *tile = &ef_ctiles(g)[i];
    int toxic = tile->has_improvement_n && ef_improvement_has(t, tile->improvement.kind_n, EF_AB_POISON);
    int pi = ef_player_index(g, player);
    int resists = pi != EF_NONE && ef_tribe_has(t, g->players[pi].tribe, EF_AB_POISONRESIST);
    for (int depth = 1; depth >= 0; depth--) {
        if (nodes[depth] == EF_NONE) continue;
        int32_t k = ef_cunits(g)[nodes[depth]].kind_n;
        if (toxic && !resists && !ef_unit_has(t, k, EF_AB_FLY)) {
            EfAction a = ef_action_at(EF_A_POISON, tile->owner_n, c);
            a.p[1] = c;
            EF_TRY(ef_push(g, a));
        }
        if (ef_unit_has(t, k, EF_AB_TENTACLES)) EF_TRY(ef_update_tentacles(g, t));
    }
    *out = nodes[0];
    return ef_ok();
}

/* units.rs make_exhausted: no more moves or attacks, DoubleReady and Boosted removed; every
 * follower in the chain is spent too (flags only) */
EF_FN EfResult ef_make_exhausted(EfGame *g, int32_t node) {
    ef_units(g)[node].moved_n = 1; /* narrow: a flag or constant */
    ef_units(g)[node].attacked_n = 1; /* narrow: a flag or constant */
    EfEffectList *e = &ef_lists(g)[ef_units(g)[node].effects_n];
    const int64_t gone[2] = { 7, 2 };
    for (int k = 0; k < 2; k++) {
        int at = ef_effect_index(e, gone[k]);
        if (at >= 0) ef_effect_remove_at(e, at);
    }
    int32_t steps = 0, f = node;
    for (;;) {
        EF_TRY(ef_next_follower(g, f, &steps, &f));
        if (f == EF_NONE) break;
        if (f == node) return ef_err(EF_ERR_CYCLIC_CHAIN, 0);
        ef_units(g)[f].moved_n = 1; /* narrow: a flag or constant */
        ef_units(g)[f].attacked_n = 1; /* narrow: a flag or constant */
    }
    return ef_ok();
}

/* units.rs promote: the unit (its passenger if carried) gains a level and is healed */
EF_FN EfResult ef_promote(EfGame *g, const EfTables *t, EfXy c) {
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile || tile->unit_n == EF_NONE) return ef_err(EF_ERR_PROMOTION_NO_UNIT, 0);
    int32_t n = tile->unit_n;
    int32_t selected = ef_cunits(g)[n].passenger_n != EF_NONE ? ef_cunits(g)[n].passenger_n : n;
    EF_PUT(ef_units(g)[selected].promotion_level_n, ef_units(g)[selected].promotion_level_n + 1, int8_t);
    EfUnit copy = ef_cunits(g)[n];
    int32_t max;
    EF_TRY(ef_max_health(g, t, &copy, &max));
    ef_units(g)[n].health_n = (uint16_t)(uint32_t)max; /* narrow: the engine's u16 */
    return ef_ok();
}

/* movement.rs unit_path_budget: the unit's path with an explicit budget */
EF_FN EfResult ef_unit_path_budget(const EfGame *g, const EfTables *t, const EfUnit *unit, EfXy to, int32_t budget, EfPath *path, int *found) {
    EfMover mover;
    EF_TRY(ef_mover_context(g, t, unit->owner_n, &mover));
    EfProfile s;
    int32_t range;
    EF_TRY(ef_profile_with(g, t, unit, &mover, &s, &range));
    (void)range;   /* the budget, not the range, bounds this search */
    return ef_scratch_path(t, &s, to, budget, path, found);
}

/* units.rs average: opposite directions give the first turned a
 * quarter; any other sum the direction with the strictly greatest weighted dot product (the
 * first on a tie) */
EF_FN int32_t ef_average_direction(int32_t first, int32_t second) {
    const int8_t vx[8] = { -1, -1, -1, 0, 1, 1, 1, 0 };
    const int8_t vy[8] = { -1, 0, 1, 1, 1, 0, -1, -1 };
    int32_t ax = first >= 0 && first < 8 ? vx[first] : 0, ay = first >= 0 && first < 8 ? vy[first] : 0;
    int32_t bx = second >= 0 && second < 8 ? vx[second] : 0, by = second >= 0 && second < 8 ? vy[second] : 0;
    int32_t x = ax + bx, y = ay + by;
    if (x == 0 && y == 0) return (first + 2) % 8;
    int32_t best = 0, score = 0;
    for (int32_t i = 0; i < 8; i++) {
        int32_t weight = vx[i] * vx[i] + vy[i] * vy[i] == 1 ? 1393 : 985;
        int32_t dot = (vx[i] * x + vy[i] * y) * weight;
        if (score < dot) { score = dot; best = i; }
    }
    return best;
}

/* units.rs try_push: the occupant steps aside if it can; *has = 0 when it cannot */
EF_BIG EfResult ef_try_push(EfGame *g, const EfTables *t, int32_t actor, EfXy c, EfAction *push, int *has) {
    const int8_t vx[8] = { -1, -1, -1, 0, 1, 1, 1, 0 };
    const int8_t vy[8] = { -1, 0, 1, 1, 1, 0, -1, -1 };
    *has = 0;
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_PUSH_TILE, 0);
    int32_t n = ef_ctiles(g)[i].unit_n;
    if (n == EF_NONE) return ef_err(EF_ERR_PUSH_NO_UNIT, 0);
    EfUnit unit = ef_cunits(g)[n];
    int32_t direction = unit.owner_n == actor ? unit.direction_n : unit.direction_n < 4 ? unit.direction_n + 4 : unit.direction_n - 4;
    /* a chain member is pushed along the chain: toward its leader, its follower, or between them */
    int32_t toward[2] = { 0, 0 };
    const uint32_t linked[2] = { unit.leader, unit.follower };
    for (int k = 0; k < 2; k++) {
        int32_t l = linked[k] != 0 ? ef_find_unit(g, linked[k]) : EF_NONE;
        if (l != EF_NONE) toward[k] = ef_facing_direction(c, ef_cunits(g)[l].coordinates);
    }
    if (unit.leader != 0 && unit.follower != 0) direction = ef_average_direction(toward[0], toward[1]);
    else if (unit.leader != 0) direction = toward[0];
    else if (unit.follower != 0) direction = toward[1];
    int vehicle;
    EF_TRY(ef_kind_has(t, unit.kind_n, EF_AB_CARRY, &vehicle));
    int source_water = ef_ctiles(g)[i].terrain_n == 1 || ef_ctiles(g)[i].terrain_n == 2;
    int has_fallback = 0;
    EfXy fallback = ef_xy(0, 0);
    const int offsets[8] = { 0, -1, 1, -2, 2, -3, 3, -4 };
    for (int k = 0; k <= 8; k++) {
        EfXy target;
        if (k < 8) {
            int32_t d = ((direction + offsets[k] + 8) % 8 + 8) % 8;
            target = ef_xy(c.x + vx[d], c.y + vy[d]);
            const EfTile *next = ef_tile_c(g, target);
            if (!next || next->unit_n != EF_NONE) continue;
            if (source_water && vehicle && !(next->terrain_n == 1 || next->terrain_n == 2) && !has_fallback) {
                has_fallback = 1;
                fallback = target;
                continue;
            }
        } else {
            if (!has_fallback) break;
            target = fallback;
        }
        EF_TRY(ef_refresh_tech_cache(g, t, unit.owner_n));
        EfPath path;
        int found;
        EF_TRY(ef_unit_path_budget(g, t, &unit, target, 1, &path, &found));
        if (found) {
            *push = ef_action(EF_A_MOVE, unit.owner_n);
            push->i[0] = (int32_t)unit.id;
            push->i[1] = 1;
            push->i[2] = 2;
            push->len = path.len;
            for (int q = 0; q < path.len; q++) push->list.path[q] = path.tiles[q];
            *has = 1;
            return ef_ok();
        }
    }
    return ef_ok();
}

/* units.rs train_on_occupied: a free Train queued, the occupant pushed (or killed) first */
EF_BIG EfResult ef_train_on_occupied(EfGame *g, const EfTables *t, int32_t player, int32_t kind, EfXy c) {
    int pi;
    EF_TRY(ef_player(g, player, &pi));
    /* the kind is resolved through the player's tribe again */
    EF_TRY(ef_tribe_unit(t, g->players[pi].tribe, kind, &kind));
    EfAction train = ef_action_at(EF_A_TRAIN, player, c);
    train.i[0] = kind;
    train.p[1] = ef_xy(-1, -1);
    EF_TRY(ef_push(g, train));
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_TRAIN_TILE, 0);
    int32_t occupant = tile->unit_n;
    if (occupant == EF_NONE) return ef_ok();
    EfAction push;
    int has;
    EF_TRY(ef_try_push(g, t, player, c, &push, &has));
    if (has) {
        EF_TRY(ef_push(g, push));
    } else {
        EF_TRY(ef_push(g, ef_action_at(EF_A_KILL, player, c)));
        /* rules124 adds a Houdini action after the Kill; rules123 has none */
        if (g->version == 124) EF_TRY(ef_push(g, ef_action_at(EF_A_HOUDINI, player, c)));
    }
    EfUnit u = ef_cunits(g)[occupant];
    if (ef_unit_effect(g, &u, EF_INVISIBLE_EFFECT) && u.owner_n != player) {
        int peace = 0;
        if (u.owner_n != 0) {
            EfRelation *r;
            EF_TRY(ef_relation_entry(g, pi, u.owner_n, &r));
            peace = r->state == EF_PEACE;
        }
        if (!peace) {
            EfAction reveal = ef_action_at(EF_A_REVEAL, player, c);
            reveal.p[1] = ef_xy(-1, -1);
            EF_TRY(ef_push(g, reveal));
        }
    }
    return ef_ok();
}

/* units.rs train (the Train economy action) */
EF_BIG EfResult ef_train(EfGame *g, const EfTables *t, int32_t player, int32_t kind, EfXy c, int32_t cost, EfXy facing_target) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE || !ef_unit_data(t, kind)) return ef_ok();
    int p = ef_player_index(g, player);
    if (p == EF_NONE) return ef_ok();
    if (ef_ctiles(g)[i].unit_n != EF_NONE) {
        int32_t cur = ef_wsub(g->players[p].currency, cost);
        g->players[p].currency = cur > 0 ? cur : 0;
        return ef_train_on_occupied(g, t, player, kind, c);
    }
    int scout;
    EF_TRY(ef_kind_has(t, kind, EF_AB_SCOUT, &scout));
    int32_t sight = 1 + scout;
    int32_t cur = ef_wsub(g->players[p].currency, cost);
    g->players[p].currency = cur > 0 ? cur : 0;
    int32_t node;
    EF_TRY(ef_create_unit(g, t, player, kind, c, &node));
    if (!(facing_target.x == -1 && facing_target.y == -1)) {
        int32_t d = ef_facing_direction(c, facing_target);
        ef_units(g)[node].direction_n = (int8_t)d; /* narrow: ef_facing_direction gives 0 to 8 */
        ef_units(g)[node].flipped_n = d < 4; /* narrow: a flag or constant */
    }
    int32_t amount = ef_unit_score(g, t, &ef_cunits(g)[node]);
    if (ef_unit_has(t, kind, EF_AB_TENTACLES)) {
        EF_TRY(ef_area_damage(g, t, node, c));
        EF_TRY(ef_update_tentacles(g, t));
    }
    EF_TRY(ef_explore_from(g, player, c, sight));
    EF_TRY(ef_surrounding(g, t, player, c));
    EfAction score = ef_action_at(EF_A_INCREASE_SCORE, player, c);
    score.i[0] = amount;
    return ef_push(g, score);
}

/* units.rs embark: the land unit becomes the passenger of a new vehicle on its tile */
EF_BIG EfResult ef_embark(EfGame *g, const EfTables *t, int32_t player, EfXy c) {
    if (ef_player_index(g, player) == EF_NONE) return ef_ok();
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_EMBARK_TILE, 0);
    int32_t old = ef_ctiles(g)[i].unit_n;
    if (old == EF_NONE) return ef_err(EF_ERR_EMBARK_NO_UNIT, 0);
    int32_t kind = ef_cunits(g)[old].kind_n, vehicle;
    if (kind == EF_GIANT) vehicle = EF_JUGGERNAUT;
    else if (kind == EF_DAGGER) vehicle = EF_PIRATE;
    else {
        int hide;
        EF_TRY(ef_kind_has(t, kind, EF_AB_HIDE, &hide));
        vehicle = hide ? EF_CLOAK_BOAT : EF_TRANSPORTSHIP;
    }
    int32_t made;
    EF_TRY(ef_create_unit(g, t, player, vehicle, c, &made));
    int protect;
    EF_TRY(ef_kind_has(t, vehicle, EF_AB_PROTECT, &protect));
    EfUnit p = ef_cunits(g)[old];
    EfUnit *v = &ef_units(g)[made];
    if (!protect) v->health_n = p.health_n; /* narrow: a copy of the same field */
    v->home = p.home;
    v->direction_n = p.direction_n; /* narrow: a copy of the same field */
    v->flipped_n = p.flipped_n; /* narrow: a copy of the same field */
    v->passenger_n = (int16_t)old; /* narrow: a unit slot (state.h assert) */
    v->effects_n = p.effects_n; /* narrow: a copy of the same field */
    v->moved_n = 1; /* narrow: a flag or constant */
    v->attacked_n = 1; /* narrow: a flag or constant */
    int stomp;
    EF_TRY(ef_kind_has(t, vehicle, EF_AB_STOMP, &stomp));
    if (stomp) EF_TRY(ef_area_damage(g, t, made, c));
    return ef_ok();
}

/* units.rs upgradable: the units `player`'s techs unlock, through
 * its tribe, whose upgradesFrom is `base`, in tech order (hidden units count; a unique one only
 * while the player owns none), at most `cap` of them into `out` */
EF_FN EfResult ef_upgradable(const EfGame *g, const EfTables *t, int32_t player, int32_t base, int32_t *out, int32_t cap,
                             int32_t *len) {
    *len = 0;
    int pi = ef_player_index(g, player);
    if (pi == EF_NONE) return ef_err(EF_ERR_MISSING_PLAYER, 0);
    if (!ef_unit_data(t, base)) return ef_ok();
    const EfPlayer *p = &g->players[pi];
    const EfTechTree *tree;
    EF_TRY(ef_known_tribe(t, p, &tree));
    for (int k = 0; k < p->available_tech_len; k++) {
        int32_t tech = p->available_tech[k];
        if (tech < 0 || tech >= t->tech_count) continue;
        const int32_t *list;
        int32_t unlocked;
        EF_TRY(ef_tech_unlocks(t, tree, tech, 1, &list, &unlocked));
        for (int q = 0; q < unlocked; q++) {
            int32_t id = list[q];
            if (ef_unit_has(t, id, EF_AB_UNIQUE) && ef_owns_kind(g, player, id)) continue;
            if (t->units[id].upgrades_from != base) continue;
            out[(*len)++] = id;
            if (*len == cap) return ef_ok();
        }
    }
    return ef_ok();
}

/* units.rs disembark: the passenger takes the vehicle's tile, health and facing */
EF_BIG EfResult ef_disembark(EfGame *g, const EfTables *t, int32_t player, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_DISEMBARK_TILE, 0);
    int32_t vehicle = ef_ctiles(g)[i].unit_n;
    if (vehicle == EF_NONE) return ef_ok();
    EfUnit v = ef_cunits(g)[vehicle];
    if (v.passenger_n == EF_NONE) return ef_ok();
    int32_t passenger = v.passenger_n;
    int protect;
    EF_TRY(ef_kind_has(t, v.kind_n, EF_AB_PROTECT, &protect));
    EfUnit *p = &ef_units(g)[passenger];
    if (!protect) p->health_n = v.health_n; /* narrow: a copy of the same field */
    p->direction_n = v.direction_n; /* narrow: a copy of the same field */
    p->flipped_n = v.flipped_n; /* narrow: a copy of the same field */
    p->moved_n = 1; /* narrow: a flag or constant */
    p->attacked_n = 1; /* narrow: a flag or constant */
    p->coordinates = c;
    ef_tiles(g)[i].unit_n = (int16_t)passenger; /* narrow: a unit slot or EF_NONE (state.h assert) */
    int32_t kind = ef_cunits(g)[passenger].kind_n;
    if (ef_unit_has(t, kind, EF_AB_GROW)) {
        /* a Grow passenger older than two turns comes ashore grown, if its tribe can train the
         * next stage */
        EF_TRY(ef_refresh_tech_cache(g, t, player));
        uint32_t age = (uint32_t)g->current_turn - (uint32_t)ef_cunits(g)[passenger].created_turn_n;
        int32_t next, found;
        EF_TRY(ef_upgradable(g, t, player, kind, &next, 1, &found));
        if (found && age > 2) {
            EfAction a = ef_action_at(EF_A_UPGRADE, player, c);
            a.i[0] = next;
            a.i[1] = 0;
            EF_TRY(ef_push(g, a));
        }
    }
    int32_t difference = ef_wsub(ef_unit_score(g, t, &v), ef_unit_score(g, t, &ef_cunits(g)[passenger]));
    EfAction a = ef_action(EF_A_DECREASE_SCORE, player);
    a.i[0] = difference;
    return ef_push(g, a);
}

/* units.rs upgrade: a new unit of `kind` replaces the tile's unit, keeping its identity */
EF_BIG EfResult ef_upgrade(EfGame *g, const EfTables *t, int32_t player, EfXy c, int32_t kind, int32_t cost) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE || !ef_unit_data(t, kind)) return ef_ok();
    int p = ef_player_index(g, player);
    if (p == EF_NONE) return ef_ok();
    int32_t original = ef_ctiles(g)[i].unit_n;
    if (original == EF_NONE) return ef_err(EF_ERR_UPGRADE_NO_UNIT, 0);
    int32_t cur = ef_wsub(g->players[p].currency, cost);
    g->players[p].currency = cur > 0 ? cur : 0;
    int32_t made;
    EF_TRY(ef_create_unit(g, t, player, kind, c, &made));
    EfUnit old = ef_cunits(g)[original];
    EfUnit *n = &ef_units(g)[made];
    if (old.passenger_n != EF_NONE) n->passenger_n = old.passenger_n; /* narrow: a copy of the same field */
    n->home = old.home;
    n->moved_n = old.moved_n; /* narrow: a copy of the same field */
    n->attacked_n = old.attacked_n; /* narrow: a copy of the same field */
    n->flipped_n = old.flipped_n; /* narrow: a copy of the same field */
    n->direction_n = old.direction_n; /* narrow: a copy of the same field */
    n->follower = old.follower;
    n->id = old.id;
    n->effects_n = old.effects_n; /* narrow: a copy of the same field */
    int32_t new_health = n->health_n, health;
    int carry;
    EF_TRY(ef_kind_has(t, kind, EF_AB_CARRY, &carry));
    if (carry) {
        health = old.health_n;
    } else {
        int32_t max;
        EF_TRY(ef_max_health(g, t, &old, &max));
        int32_t missing = max - old.health_n;
        int32_t cap = new_health - 1;
        health = new_health - (missing < cap ? missing : cap);
    }
    EF_PUT(ef_units(g)[made].health_n, health, uint16_t);
    int32_t score = ef_wsub(ef_unit_score(g, t, &ef_cunits(g)[made]), ef_unit_score(g, t, &old));
    EfAction a = ef_action_at(EF_A_INCREASE_SCORE, player, c);
    a.i[0] = score;
    EF_TRY(ef_push(g, a));
    int scout;
    EF_TRY(ef_kind_has(t, kind, EF_AB_SCOUT, &scout));
    return ef_explore_from(g, player, c, 1 + scout);
}

#endif /* EF_UNITS_H */
