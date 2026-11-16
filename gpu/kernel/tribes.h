/* tribes.h - the special tribes' rules: ports of engine-fast/src/aquarion.rs, cymanti.rs,
 * polaris.rs, elyrion.rs and linked.rs (the parts the rules call; forward declarations in
 * core.h). Included after infiltrate.h, so every rule the others call is defined. */
#ifndef EF_TRIBES_H
#define EF_TRIBES_H

#include "infiltrate.h"


/* cymanti.rs poison_tile: a poison improvement poisons the unit that stands on it, unless its
 * owner's tribe resists or it flies (Combat::Poison for the tile's owner) */
EF_BIG EfResult ef_poison_tile(EfGame *g, const EfTables *t, int32_t node, EfXy c) {
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile || !tile->has_improvement_n) return ef_ok();
    if (!ef_improvement_has(t, tile->improvement.kind_n, EF_AB_POISON)) return ef_ok();
    const EfUnit *u = &ef_cunits(g)[node];
    int pi = ef_player_index(g, u->owner_n);
    if (pi == EF_NONE) return ef_ok();
    int resists = ef_tribe_has(t, g->players[pi].tribe, EF_AB_POISONRESIST);
    const EfUnitData *d = ef_unit_data(t, u->kind_n);
    if (resists || (d && ef_has(d->abilities, EF_AB_FLY))) return ef_ok();
    EfAction a = ef_action_at(EF_A_POISON, tile->owner_n, c);
    a.p[1] = c;
    return ef_push(g, a);
}

/* aquarion.rs update_tentacles: the Tentacle effect on every tile next to an unfrozen
 * Tentacles unit, and nowhere else */
EF_BIG EfResult ef_update_tentacles(EfGame *g, const EfTables *t) {
    for (int i = 0; i < g->tile_count; i++) ef_tile_effect_remove(&ef_tiles(g)[i], EF_TENTACLE_EFFECT);
    for (int i = 0; i < g->tile_count; i++) {
        const EfTile *x = &ef_ctiles(g)[i];
        if (x->unit_n == EF_NONE) continue;
        const EfUnit *u = &ef_cunits(g)[x->unit_n];
        if (!ef_unit_has(t, u->kind_n, EF_AB_TENTACLES) || ef_unit_effect(g, u, EF_FROZEN)) continue;
        EfXy c = x->coordinates;
        for (int32_t xx = c.x - 1; xx <= c.x + 1; xx++)
            for (int32_t yy = c.y - 1; yy <= c.y + 1; yy++) {
                if (xx == c.x && yy == c.y) continue;
                int32_t k = ef_tile_index(g, ef_xy(xx, yy));
                if (k != EF_NONE) EF_TRY(ef_tile_effect_add(&ef_tiles(g)[k], EF_TENTACLE_EFFECT));
            }
    }
    return ef_ok();
}

/* aquarion.rs strike: a unit of `actor` that enters a Tentacle tile is hit by the healthiest
 * adjacent unfrozen Tentacles unit (the first on a tie) of another player not at peace with it */
EF_BIG EfResult ef_tentacle_strike(EfGame *g, const EfTables *t, int32_t node, int32_t actor, EfXy target) {
    EfUnit defender = ef_cunits(g)[node];
    int found = 0;
    EfXy origin = ef_xy(-1, -1);
    EfUnit attacker;
    memset(&attacker, 0, sizeof attacker);
    for (int32_t x = target.x - 1; x <= target.x + 1; x++)
        for (int32_t y = target.y - 1; y <= target.y + 1; y++) {
            EfXy o = ef_xy(x, y);
            if (ef_xy_eq(o, target)) continue;
            const EfTile *tile = ef_tile_c(g, o);
            if (!tile || tile->unit_n == EF_NONE) continue;
            EfUnit u = ef_cunits(g)[tile->unit_n];
            if (u.owner_n == EF_NATURE || u.owner_n == actor) continue;
            int32_t r;
            EF_TRY(ef_relation_state_of(g, actor, u.owner_n, &r));
            if (r == EF_PEACE || r == EF_COOL_DOWN) continue;
            if (!ef_unit_has(t, u.kind_n, EF_AB_TENTACLES) || ef_unit_effect(g, &u, EF_FROZEN)) continue;
            if (!found || u.health_n > attacker.health_n) {
                found = 1;
                origin = o;
                attacker = u;
            }
        }
    if (!found) return ef_ok();
    int32_t damage, back;
    int advance, reveal;
    EF_TRY(ef_battle_full(g, t, &attacker, &defender, &damage, &back, &advance, &reveal));
    EfAction a = ef_action_at(EF_A_ATTACK, attacker.owner_n, origin);
    a.p[1] = target;
    a.i[0] = damage;
    a.i[1] = 3;
    a.i[2] = 0;
    a.i[3] = 100;
    return ef_push(g, a);
}

/* ---- polaris.rs: freezing tiles and units, breaking ice, the climate ---- */
#define EF_MAGMA_SKIN 18

/* a unit effect added unless present (polaris.rs add_effect), on the unit's own list */
EF_FN EfResult ef_unit_effect_add(EfGame *g, int32_t node, int32_t effect) {
    EfEffectList *l = &ef_lists(g)[ef_units(g)[node].effects_n];
    if (ef_has_effect(l, effect)) return ef_ok();
    if (l->len_n >= 9) return ef_short_list_full(9);
    l->items_n[l->len_n] = (int8_t)effect; /* narrow: a unit effect id below 15 */
    l->len_n = (int8_t)(l->len_n + 1); /* narrow: below 9, checked above */
    return ef_ok();
}

/* polaris.rs spreading: the tribe spreads its climate (tribe ability Spread) */
EF_FN int ef_spreading(const EfTables *t, int32_t tribe) { return ef_tribe_has(t, tribe, EF_AB_SPREAD); }
/* polaris.rs climate_of: the lazy default of the climate, unset reads as the tribe */
EF_FN int32_t ef_climate_of(const EfPlayer *p) { return p->climate == 0 ? p->tribe : p->climate; }
/* polaris.rs settle_climate: the lazy write of an unset climate */
EF_FN void ef_settle_climate(EfGame *g, int i) {
    if (g->players[i].climate == 0) g->players[i].climate = g->players[i].tribe;
}
/* polaris.rs at_peace (read only) */
EF_FN int ef_unit_at_peace(const EfGame *g, int32_t actor, const EfUnit *u) {
    if (u->owner_n == actor) return 1;
    int32_t r = ef_relation(g, actor, u->owner_n);
    return r == EF_PEACE || r == EF_COOL_DOWN;
}

/* polaris.rs freezable: whether a tile is freezable without its lazy climate write; *writes says
 * whether the game reaches that write */
EF_FN EfResult ef_freezable(const EfGame *g, int32_t actor, EfXy c, int only_owned, int *out, int *writes) {
    *out = 0;
    *writes = 0;
    int pi = ef_player_index(g, actor);
    if (pi == EF_NONE) return ef_err(EF_ERR_FREEZE_ACTOR, 0);
    const EfPlayer *p = &g->players[pi];
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_FREEZE_TILE, 0);
    if (only_owned && tile->owner_n != actor) return ef_ok();
    if (actor != EF_NATURE && !ef_contains_u8(tile->explorers_n, tile->explorers_len_n, actor)) return ef_ok();
    if (ef_is_water_tile(tile) || ef_flooded(tile)) { *out = 1; return ef_ok(); }
    *writes = 1;
    if (tile->climate_n != ef_climate_of(p) || tile->skin_n != p->skin) { *out = 1; return ef_ok(); }
    const EfUnit *u = ef_visible_unit(g, tile, actor);
    *out = u && !ef_unit_at_peace(g, actor, u) && !ef_unit_effect(g, u, EF_FROZEN);
    return ef_ok();
}
/* polaris.rs freezable_mut: with the lazy climate write applied */
EF_FN EfResult ef_freezable_mut(EfGame *g, int32_t actor, EfXy c, int only_owned, int *out) {
    int writes;
    EF_TRY(ef_freezable(g, actor, c, only_owned, out, &writes));
    if (writes) {
        int i;
        EF_TRY(ef_player(g, actor, &i));
        ef_settle_climate(g, i);
    }
    return ef_ok();
}

/* polaris.rs area_without_centre: the area (radius, diagonal, no centre), x-major */
EF_FN int ef_area_without_centre(const EfGame *g, EfXy c, int32_t r, EfXy out[8]) {
    int32_t h = g->tile_count / g->width, n = 0;
    for (int32_t x = c.x - r < 0 ? 0 : c.x - r; x <= (c.x + r < g->width - 1 ? c.x + r : g->width - 1); x++)
        for (int32_t y = c.y - r < 0 ? 0 : c.y - r; y <= (c.y + r < h - 1 ? c.y + r : h - 1); y++)
            if (!(x == c.x && y == c.y)) out[n++] = ef_xy(x, y);
    return n;
}

/* polaris.rs can_freeze_area: whether the unit can freeze its area (radius 1), read only */
EF_FN EfResult ef_can_freeze_area(const EfGame *g, const EfTables *t, const EfUnit *u, int *out) {
    *out = 0;
    if (u->moved_n || u->attacked_n || !ef_unit_has(t, u->kind_n, EF_AB_FREEZEAREA) || ef_player_index(g, u->owner_n) == EF_NONE)
        return ef_ok();
    EfXy area[8];
    int n = ef_area_without_centre(g, u->coordinates, 1, area);
    for (int k = 0; k < n; k++) {
        int ok, writes;
        EF_TRY(ef_freezable(g, u->owner_n, area[k], 0, &ok, &writes));
        if (ok) { *out = 1; return ef_ok(); }
    }
    return ef_ok();
}

/* polaris.rs admit_freeze_area: can_freeze_area with the validator's writes */
EF_FN EfResult ef_admit_freeze_area(EfGame *g, const EfTables *t, EfXy c) {
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile || tile->unit_n == EF_NONE) return ef_err(EF_ERR_FREEZE_NO_UNIT, 0);
    EfUnit u = ef_cunits(g)[tile->unit_n];
    if (u.moved_n || u.attacked_n || !ef_unit_has(t, u.kind_n, EF_AB_FREEZEAREA) || ef_player_index(g, u.owner_n) == EF_NONE)
        return ef_err(EF_ERR_FREEZE_REFUSED, 0);
    EfXy area[8];
    int n = ef_area_without_centre(g, u.coordinates, 1, area);
    for (int k = 0; k < n; k++) {
        int ok;
        EF_TRY(ef_freezable_mut(g, u.owner_n, area[k], 0, &ok));
        if (ok) return ef_ok();
    }
    return ef_err(EF_ERR_FREEZE_REFUSED, 0);
}

/* polaris.rs spend: the unit on c is spent; its slot, or EF_NONE */
EF_FN int32_t ef_spend(EfGame *g, EfXy c) {
    const EfTile *x = ef_tile_c(g, c);
    if (!x || x->unit_n == EF_NONE) return EF_NONE;
    int32_t n = x->unit_n;
    ef_units(g)[n].moved_n = 1; /* narrow: a flag or constant */
    ef_units(g)[n].attacked_n = 1; /* narrow: a flag or constant */
    return n;
}

/* polaris.rs freeze_area_command */
EF_FN EfResult ef_freeze_area_command(EfGame *g, int32_t p, EfXy c) {
    EF_TRY(ef_base(g, p, 17));
    ef_spend(g, c);
    EfAction a = ef_action_at(EF_A_FREEZE_AREA, p, c);
    a.i[0] = 1;
    a.i[1] = 1;
    a.i[2] = 0;
    return ef_push(g, a);
}

/* the UpdateCityConnections (for `actor` alone) and UpdateRoutes pair the area actions queue */
EF_FN EfResult ef_push_connections_and_routes(EfGame *g, int32_t actor) {
    EfAction u = ef_action(EF_A_UPDATE_CITY_CONNECTIONS, actor);
    u.len = 1;
    u.list.players[0] = actor;
    EF_TRY(ef_push(g, u));
    return ef_push(g, ef_action(EF_A_UPDATE_ROUTES, actor));
}

/* polaris.rs freeze_area: connections and routes first, then from the far end of the sorted
 * area FreezeTile where freezable and FreezeUnit on any unit not at peace and not frozen */
EF_BIG EfResult ef_freeze_area(EfGame *g, int32_t actor, EfXy center, int32_t radius, int freeze_units, int only_owned) {
    EfArea area;
    EF_TRY(ef_area_sorted(g, center, radius, &area));
    if (area.len == 0) return ef_ok();
    EF_TRY(ef_push_connections_and_routes(g, actor));
    int ai;
    EF_TRY(ef_player(g, actor, &ai));
    for (int k = area.len - 1; k >= 0; k--) {
        int32_t i = area.tiles[k];
        EfXy c = ef_ctiles(g)[i].coordinates;
        int ok;
        EF_TRY(ef_freezable_mut(g, actor, c, only_owned, &ok));
        if (ok) EF_TRY(ef_push(g, ef_action_at(EF_A_FREEZE_TILE, actor, c)));
        if (freeze_units && ef_ctiles(g)[i].unit_n != EF_NONE) {
            EfUnit u = ef_cunits(g)[ef_ctiles(g)[i].unit_n];
            int peace;
            EF_TRY(ef_at_peace(g, actor, u.owner_n, &peace));
            if (!peace && !ef_unit_effect(g, &u, EF_FROZEN)) {
                EfAction a = ef_action(EF_A_FREEZE_UNIT, actor);
                a.p[0] = ef_xy(-1, -1);
                a.p[1] = c;
                a.i[0] = 0;
                EF_TRY(ef_push(g, a));
            }
        }
    }
    return ef_ok();
}

/* polaris.rs ice_bank_levels: every ice bank to the current level,
 * one level action at a time, in tile order, for the tile's owner */
EF_FN EfResult ef_ice_bank_levels(EfGame *g, const EfTables *t) {
    int any = 0;
    for (int i = 0; i < g->tile_count && !any; i++)
        any = ef_ctiles(g)[i].has_improvement_n && ef_ctiles(g)[i].improvement.kind_n == EF_ICE_BANK;
    if (!any || !ef_improvement_known(t, EF_ICE_BANK)) return ef_ok();
    int32_t level;
    EF_TRY(ef_ice_bank_level(g, t, &level));
    for (int i = 0; i < g->tile_count; i++) {
        const EfTile *x = &ef_ctiles(g)[i];
        if (!x->has_improvement_n || x->improvement.kind_n != EF_ICE_BANK) continue;
        int32_t current = x->improvement.level_n;
        if (level < current) {
            for (int32_t k = 0; k < current - level; k++) {
                EfAction a = ef_action_at(EF_A_LEVEL_DOWN_IMPROVEMENT, x->owner_n, x->coordinates);
                a.i[0] = 1;
                EF_TRY(ef_push(g, a));
            }
        } else {
            for (int32_t k = 0; k < level - current; k++)
                EF_TRY(ef_push(g, ef_action_at(EF_A_LEVEL_UP_IMPROVEMENT, x->owner_n, x->coordinates)));
        }
    }
    return ef_ok();
}

/* polaris.rs freeze_tile: freezing a tile (shorelines are not in the block) */
EF_BIG EfResult ef_freeze_tile(EfGame *g, const EfTables *t, int32_t actor, EfXy c) {
    int ok;
    EF_TRY(ef_freezable_mut(g, actor, c, 0, &ok));
    if (!ok) return ef_ok();
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_FREEZE_TILE_TILE, 0);
    EfTile *x = &ef_tiles(g)[i];
    if (ef_is_water_tile(x)) x->terrain_n = EF_ICE; /* narrow: a terrain id */
    if (ef_flooded(x)) ef_tile_effect_remove(x, EF_SWAMPED);
    int pi;
    EF_TRY(ef_player(g, actor, &pi));
    ef_settle_climate(g, pi);
    EF_PUT(x->climate_n, g->players[pi].climate, int8_t);
    EF_PUT(x->skin_n, g->players[pi].skin, int8_t);
    return ef_ice_bank_levels(g, t);
}

/* polaris.rs freeze_unit: the target is spent and Frozen (and Petrified on the Magma skin); a
 * freeze from an attacker deals its damage first */
EF_BIG EfResult ef_freeze_unit(EfGame *g, const EfTables *t, int32_t actor, EfXy origin, EfXy target, int32_t damage) {
    const EfTile *x = ef_tile_c(g, target);
    if (!x || x->unit_n == EF_NONE) return ef_err(EF_ERR_FREEZE_UNIT_TARGET, 0);
    int32_t n = x->unit_n;
    int32_t owner = ef_cunits(g)[n].owner_n;
    int ai, oi;
    EF_TRY(ef_player(g, actor, &ai));
    EF_TRY(ef_player(g, owner, &oi));
    EF_TRY(ef_attack_diplomacy(g, actor, owner));
    if (!(origin.x == -1 && origin.y == -1)) EF_TRY(ef_perform_attack(g, t, actor, origin, target, damage));
    ef_units(g)[n].moved_n = 1; /* narrow: a flag or constant */
    ef_units(g)[n].attacked_n = 1; /* narrow: a flag or constant */
    EF_TRY(ef_unit_effect_add(g, n, EF_FROZEN));
    if (g->players[ai].skin == EF_MAGMA_SKIN) EF_TRY(ef_unit_effect_add(g, n, EF_PETRIFIED));
    ef_reset_peace(g, ai);
    if (g->achievements_active) return ef_err(EF_ERR_ACHIEVEMENTS, 0);
    if (ef_unit_has(t, ef_cunits(g)[n].kind_n, EF_AB_TENTACLES)) EF_TRY(ef_update_tentacles(g, t));
    return ef_ok();
}

/* polaris.rs breakable: breakable ice, ice that is empty, holds a swimmer or flyer, a
 * unit `viewer` cannot see, algae or a bridge */
EF_FN int ef_breakable(const EfGame *g, const EfTables *t, EfXy c, int32_t viewer) {
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile || tile->terrain_n != EF_ICE) return 0;
    int forced = ef_contains_i8(tile->effects_n, tile->effects_len_n, EF_ALGAE)
                 || (tile->has_improvement_n && tile->improvement.kind_n == EF_BRIDGE);
    if (tile->unit_n == EF_NONE) return 1;
    const EfUnit *u = &ef_cunits(g)[tile->unit_n];
    int free = ef_unit_has(t, u->kind_n, EF_AB_SWIM) || ef_unit_has(t, u->kind_n, EF_AB_FLY)
               || (ef_unit_effect(g, u, EF_INVISIBLE_EFFECT) && u->owner_n != viewer && ef_relation(g, viewer, u->owner_n) != EF_PEACE);
    return free || forced;
}

/* polaris.rs can_break_ice: whether the unit can break ice */
EF_FN int ef_can_break_ice(const EfGame *g, const EfTables *t, const EfUnit *u) {
    if (u->leader != 0 || (u->moved_n && !ef_unit_has(t, u->kind_n, EF_AB_DASH)) || u->attacked_n
        || ef_unit_has(t, u->kind_n, EF_AB_AUTOFREEZE))
        return 0;
    EfXy c = u->coordinates;
    for (int32_t x = c.x - 1; x <= c.x + 1; x++)
        for (int32_t y = c.y - 1; y <= c.y + 1; y++) {
            if (x == c.x && y == c.y) continue;
            if (ef_tile_c(g, ef_xy(x, y)) && ef_breakable(g, t, ef_xy(x, y), u->owner_n)) return 1;
        }
    return 0;
}

/* polaris.rs break_ice_command */
EF_FN EfResult ef_break_ice_command(EfGame *g, int32_t p, EfXy c) {
    EF_TRY(ef_base(g, p, 18));
    int32_t n = ef_spend(g, c);
    if (n != EF_NONE && ef_unit_effect(g, &ef_cunits(g)[n], EF_INVISIBLE_EFFECT)) {
        EfAction r = ef_action_at(EF_A_REVEAL, p, c);
        r.p[1] = ef_xy(-1, -1);
        r.i[0] = 0;
        EF_TRY(ef_push(g, r));
    }
    EfAction a = ef_action_at(EF_A_BREAK_ICE_AREA, p, c);
    a.i[0] = 1;
    return ef_push(g, a);
}

/* polaris.rs break_ice_area: breaking the ice of an area */
EF_BIG EfResult ef_break_ice_area(EfGame *g, const EfTables *t, int32_t p, EfXy center, int32_t radius) {
    EfArea area;
    EF_TRY(ef_area_sorted(g, center, radius, &area));
    if (area.len == 0) return ef_ok();
    EF_TRY(ef_push_connections_and_routes(g, p));
    for (int k = area.len - 1; k >= 0; k--) {
        EfXy c = ef_ctiles(g)[area.tiles[k]].coordinates;
        if (ef_breakable(g, t, c, p)) EF_TRY(ef_push(g, ef_action_at(EF_A_BREAK_ICE, p, c)));
    }
    return ef_ok();
}

/* polaris.rs break_ice: breaking the ice of one tile (shorelines are not in the block) */
EF_BIG EfResult ef_break_ice(EfGame *g, const EfTables *t, int32_t p, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_BREAK_ICE_TILE, 0);
    if (ef_ctiles(g)[i].unit_n != EF_NONE) {
        EfUnit u = ef_cunits(g)[ef_ctiles(g)[i].unit_n];
        if (ef_unit_effect(g, &u, EF_INVISIBLE_EFFECT)) {
            int pi;
            EF_TRY(ef_player(g, p, &pi));
            int hostile = u.owner_n != p;
            if (hostile) {
                int32_t r;
                EF_TRY(ef_relation_state_of(g, p, u.owner_n, &r));
                hostile = r != EF_PEACE;
            }
            if (hostile) {
                EfAction a = ef_action_at(EF_A_REVEAL, p, c);
                a.p[1] = ef_xy(-1, -1);
                a.i[0] = 1;
                return ef_push(g, a);
            }
        }
    }
    if (ef_breakable(g, t, c, p)) {
        EfTile *x = &ef_tiles(g)[i];
        x->terrain_n = EF_OCEAN; /* narrow: a terrain id */
        const EfXy beside[4] = {ef_xy(c.x - 1, c.y), ef_xy(c.x, c.y - 1), ef_xy(c.x, c.y + 1), ef_xy(c.x + 1, c.y)};
        for (int k = 0; k < 4; k++) {
            const EfTile *d = ef_tile_c(g, beside[k]);
            if (d && (d->terrain_n == EF_FIELD || d->terrain_n == EF_MOUNTAIN || d->terrain_n == EF_FOREST)) {
                x->terrain_n = EF_WATER; /* narrow: a terrain id */
                break;
            }
        }
        x->skin_n = 0; /* narrow: a flag or constant */
    }
    return ef_ice_bank_levels(g, t);
}

/* polaris.rs restore_climate: a tile of a spreading climate goes back to player i's climate, if
 * land (not ice, unless i's tribe spreads too, and then only on another skin); *has 0 for none */
EF_FN EfResult ef_restore_climate(EfGame *g, const EfTables *t, int i, EfXy c, int *has, EfAction *out) {
    *has = 0;
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_CLIMATE_TILE, 0);
    int32_t climate = tile->climate_n, terrain = tile->terrain_n, skin = tile->skin_n;
    if (!ef_spreading(t, climate)) return ef_ok();
    ef_settle_climate(g, i);
    int32_t mine = g->players[i].climate;
    int land = terrain == EF_FIELD || terrain == EF_MOUNTAIN || terrain == EF_FOREST || terrain == EF_ICE;
    if (!ef_spreading(t, mine)) {
        if (!land || terrain == EF_ICE) return ef_ok();
    } else if (!land || skin == g->players[i].skin) {
        return ef_ok();
    }
    *has = 1;
    *out = ef_action_at(EF_A_CHANGE_CLIMATE, g->players[i].id, c);
    out->i[0] = mine;
    return ef_ok();
}

/* polaris.rs claim_area: the climate step of a city area `actor` (index i) takes over, per tile
 * from the end of `area` (both callers hand the Rust a reversed area), each followed by the
 * economy's surrounding children */
EF_BIG EfResult ef_claim_area(EfGame *g, const EfTables *t, int32_t actor, int i, const int32_t *area, int32_t n) {
    int spreads = ef_spreading(t, g->players[i].tribe);
    for (int32_t k = n - 1; k >= 0; k--) {
        EfXy c = ef_ctiles(g)[area[k]].coordinates;
        if (spreads) {
            if (g->players[i].tribe == EF_POLARIS_TRIBE) {
                EF_TRY(ef_push(g, ef_action_at(EF_A_FREEZE_TILE, actor, c)));
            } else {
                ef_settle_climate(g, i);
                EfAction a = ef_action_at(EF_A_CHANGE_CLIMATE, actor, c);
                a.i[0] = g->players[i].climate;
                EF_TRY(ef_push(g, a));
            }
        } else if (ef_spreading(t, ef_ctiles(g)[area[k]].climate_n)) {
            int has;
            EfAction a;
            EF_TRY(ef_restore_climate(g, t, i, c, &has, &a));
            if (has) EF_TRY(ef_push(g, a));
        }
        EF_TRY(ef_surrounding(g, t, actor, c));
    }
    return ef_ok();
}

/* polaris.rs change_climate: the tile takes the climate and the player's
 * skin (Swamped joins a flooded tile on the Swamp skin) */
EF_FN EfResult ef_change_climate(EfGame *g, int32_t p, EfXy c, int32_t climate) {
    int pi;
    EF_TRY(ef_player(g, p, &pi));
    int32_t skin = g->players[pi].skin;
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_CLIMATE_CHANGE_TILE, 0);
    EfTile *x = &ef_tiles(g)[i];
    EF_PUT(x->climate_n, climate, int8_t);
    EF_PUT(x->skin_n, skin, int8_t);
    if (ef_flooded(x) && skin == EF_SWAMP_SKIN && !ef_contains_i8(x->effects_n, x->effects_len_n, EF_SWAMPED))
        EF_TRY(ef_tile_effect_add(x, EF_SWAMPED));
    return ef_ok();
}

/* ---- aquarion.rs: flooding, Bubble, clearing a tile effect ---- */

/* aquarion.rs flood_tile: Flooded once, and Swamped too when the actor has the Swamp skin */
EF_FN EfResult ef_flood_tile(EfGame *g, int32_t actor, EfXy c) {
    EfTile *x = ef_tile(g, c);
    if (!x) return ef_err(EF_ERR_FLOOD_TILE, 0);
    EF_TRY(ef_tile_effect_add(x, EF_FLOODED));
    int pi;
    EF_TRY(ef_player(g, actor, &pi));
    if (g->players[pi].skin == EF_SWAMP_SKIN) EF_TRY(ef_tile_effect_add(x, EF_SWAMPED));
    return ef_ok();
}

/* aquarion.rs flood: the tile floods; an amphibious unit on it gains Bubble if the actor has
 * the Bubble ability; then connections and routes. A tile that is not floodable is left. */
EF_BIG EfResult ef_flood(EfGame *g, const EfTables *t, int32_t actor, EfXy c) {
    const EfTile *x = ef_tile_c(g, c);
    if (!x || !ef_floodable(x)) return ef_ok();
    EF_TRY(ef_flood_tile(g, actor, c));
    if (x->unit_n != EF_NONE) {
        /* the game's ability query refreshes the actor's tech cache */
        EF_TRY(ef_refresh_tech_cache(g, t, actor));
        const EfUnit *u = &ef_cunits(g)[x->unit_n];
        if (ef_player_ability(g, t, actor, EF_AB_BUBBLE) && ef_unit_has(t, u->kind_n, EF_AB_AMPHIBIOUS)
            && !ef_unit_effect(g, u, EF_BUBBLE)) {
            EfAction b = ef_action(EF_A_BUBBLE, ef_mover(g));
            b.i[0] = (int32_t)u->id;
            b.i[1] = 1;
            EF_TRY(ef_push(g, b));
        }
    }
    return ef_push_connections_and_routes(g, actor);
}

/* aquarion.rs bubble: the unit with id `id`, wherever it stands, gains or loses Bubble */
EF_FN EfResult ef_bubble(EfGame *g, uint32_t id, int add) {
    for (int i = 0; i < g->tile_count; i++) {
        int32_t n = ef_ctiles(g)[i].unit_n;
        if (n == EF_NONE || ef_cunits(g)[n].id != id) continue;
        if (add) return ef_unit_effect_add(g, n, EF_BUBBLE);
        ef_remove_effect(g, n, EF_BUBBLE);
        return ef_ok();
    }
    return ef_ok();
}

/* aquarion.rs clear_options: Algae for a unit that can act and swims, flies or is amphibious;
 * Flooded for a unit that can move and attack, of a tribe without Flooded, that does not swim
 * unless amphibious. -1 where none. */
EF_FN void ef_clear_options(const EfGame *g, const EfTables *t, int32_t mover, const EfUnit *u, const EfTile *x, int can_move,
                            int can_attack, int32_t out[2]) {
    int32_t k = u->kind_n;
    int amphibious = ef_unit_has(t, k, EF_AB_AMPHIBIOUS), swim = ef_unit_has(t, k, EF_AB_SWIM);
    int algae = ef_contains_i8(x->effects_n, x->effects_len_n, EF_ALGAE) && (can_attack || can_move)
                && (amphibious || swim || ef_unit_has(t, k, EF_AB_FLY));
    int pi = ef_player_index(g, mover);
    int floods = pi != EF_NONE && ef_tribe_has(t, g->players[pi].tribe, EF_AB_FLOODED);
    int flood = ef_flooded(x) && can_attack && can_move && !floods && (amphibious || !swim);
    out[0] = algae ? EF_ALGAE : -1;
    out[1] = flood ? EF_FLOODED : -1;
}

/* aquarion.rs clear_tile_effect: the first such effect goes (Foam with Algae), connections and
 * routes follow, and a unit that cleared it is spent */
EF_FN EfResult ef_clear_tile_effect(EfGame *g, int32_t p, EfXy c, int32_t effect, int triggered) {
    EfTile *x = ef_tile(g, c);
    if (!x) return ef_err(EF_ERR_CLEAR_TILE, 0);
    ef_tile_effect_remove(x, effect);
    if (effect == EF_ALGAE) ef_tile_effect_remove(x, EF_FOAM);
    EF_TRY(ef_push_connections_and_routes(g, p));
    if (triggered && x->unit_n != EF_NONE) EF_TRY(ef_make_exhausted(g, x->unit_n));
    return ef_ok();
}

/* step.rs Command::Flood: never offered by the menu, but admitted: the command base, the unit
 * spent, the flood */
EF_FN EfResult ef_flood_command(EfGame *g, int32_t p, EfXy c) {
    EF_TRY(ef_base(g, p, 37));
    ef_spend(g, c);
    return ef_push(g, ef_action_at(EF_A_FLOOD, p, c));
}

/* ---- cymanti.rs: Swarm and Boost, Decompose, poison and spores, Explode ---- */
#define EF_CHARMED 8

/* cymanti.rs boost_options: own units around `c` (x-major, not the centre) that are neither
 * Boosted nor Swift and lead no chain; their count, written to `out` when given */
EF_FN int ef_boost_options(const EfGame *g, int32_t p, EfXy c, EfXy out[8]) {
    int n = 0;
    for (int32_t x = c.x - 1; x <= c.x + 1; x++)
        for (int32_t y = c.y - 1; y <= c.y + 1; y++) {
            EfXy d = ef_xy(x, y);
            if (ef_xy_eq(d, c)) continue;
            const EfTile *tile = ef_tile_c(g, d);
            if (!tile || tile->unit_n == EF_NONE) continue;
            const EfUnit *u = &ef_cunits(g)[tile->unit_n];
            if (u->owner_n != p || ef_unit_effect(g, u, EF_BOOSTED) || ef_unit_effect(g, u, EF_SWIFT) || u->leader != 0) continue;
            if (out) out[n] = d;
            n++;
        }
    return n;
}

/* cymanti.rs swarm_command (Swarm and Boost): the unit is spent and BoostOthers is queued; no
 * command base */
EF_FN EfResult ef_swarm_command(EfGame *g, int32_t p, EfXy c) {
    ef_spend(g, c);
    return ef_push(g, ef_action_at(EF_A_BOOST_OTHERS, p, c));
}

/* cymanti.rs boost_others: one Boost per boost option, in order */
EF_FN EfResult ef_boost_others(EfGame *g, int32_t p, EfXy c) {
    EfXy options[8];
    int n = ef_boost_options(g, p, c, options);
    for (int k = 0; k < n; k++) EF_TRY(ef_push(g, ef_action_at(EF_A_BOOST, p, options[k])));
    return ef_ok();
}

/* cymanti.rs boost: the unit there, if any, becomes Swift */
EF_FN EfResult ef_boost(EfGame *g, EfXy c) {
    const EfTile *x = ef_tile_c(g, c);
    if (!x || x->unit_n == EF_NONE) return ef_ok();
    return ef_unit_effect_add(g, x->unit_n, EF_SWIFT);
}

/* cymanti.rs decompose_command: queued only on an own improvement the catalog knows; no
 * command base */
EF_FN EfResult ef_decompose_command(EfGame *g, const EfTables *t, int32_t p, EfXy c) {
    const EfTile *x = ef_tile_c(g, c);
    if (!x) return ef_err(EF_ERR_DECOMPOSE_TILE, 0);
    if (!x->has_improvement_n || x->owner_n != p || x->improvement.kind_n == 0 || !ef_improvement_data(t, x->improvement.kind_n))
        return ef_ok();
    return ef_push(g, ef_action_at(EF_A_DECOMPOSE, p, c));
}

/* cymanti.rs decompose: the improvement is marked decomposing */
EF_FN EfResult ef_decompose(EfGame *g, EfXy c) {
    EfTile *x = ef_tile(g, c);
    if (!x) return ef_err(EF_ERR_DECOMPOSE_TILE, 0);
    if (!x->has_improvement_n) return ef_err(EF_ERR_DECOMPOSE_NONE, 0);
    EfImprovement *m = &x->improvement;
    if (ef_contains_i8(m->effects_n, m->effects_len_n, EF_DECOMPOSING)) return ef_ok();
    if (m->effects_len_n >= EF_IMPROVEMENT_EFFECTS) return ef_short_list_full(EF_IMPROVEMENT_EFFECTS);
    m->effects_n[m->effects_len_n] = EF_DECOMPOSING; /* narrow: an improvement effect id, 0 */
    m->effects_len_n = (int8_t)(m->effects_len_n + 1); /* narrow: below the capacity, checked above */
    return ef_ok();
}

/* cymanti.rs poison: the unit on `target` (and its passenger) is Poisoned; Charmed too when the
 * actor has the Cute skin (on the carrier, as the game writes it) */
EF_FN EfResult ef_poison(EfGame *g, int32_t actor, EfXy target) {
    const EfTile *x = ef_tile_c(g, target);
    if (!x || x->unit_n == EF_NONE) return ef_ok();
    int32_t n = x->unit_n;
    EF_TRY(ef_unit_effect_add(g, n, EF_POISONED));
    int ai;
    EF_TRY(ef_player(g, actor, &ai));
    int cute = g->players[ai].skin == EF_CUTE_SKIN;
    if (cute) EF_TRY(ef_unit_effect_add(g, n, EF_CHARMED));
    int32_t passenger = ef_cunits(g)[n].passenger_n;
    if (passenger != EF_NONE) EF_TRY(ef_unit_effect_add(g, passenger, EF_POISONED));
    return ef_ok();
}

/* cymanti.rs spores: spores for the current player, and on water an algae build (the Algae
 * improvement in rules123, AlgaeSpawn in rules124) */
EF_FN EfResult ef_spores(EfGame *g, const EfTables *t, EfXy c) {
    const EfTile *x = ef_tile_c(g, c);
    if (!x) return ef_err(EF_ERR_SPORE_TILE, 0);
    if (t->spores_error) return ef_err(EF_ERR_TABLE, t->spores_error - 1);
    int32_t actor = ef_mover(g);
    EfAction r = ef_action_at(EF_A_CREATE_RESOURCE, actor, c);
    r.i[0] = t->spores;
    r.i[1] = 0;
    EF_TRY(ef_push(g, r));
    if (x->terrain_n != EF_WATER && x->terrain_n != EF_OCEAN) return ef_ok();
    int32_t kind = g->version == 123 ? t->algae : t->algae_spawn;
    int32_t error = g->version == 123 ? t->algae_error : t->algae_spawn_error;
    if (error) return ef_err(EF_ERR_TABLE, error - 1);
    EfAction b = ef_action_at(EF_A_BUILD, actor, c);
    b.i[0] = kind;
    b.i[1] = 0;
    return ef_push(g, b);
}

/* cymanti.rs explode: the carrier's passenger (or the unit) explodes; its follower explodes
 * first and its leader lets go; spores; every unit around not at peace takes half the blast,
 * is poisoned when it survives, and the actor's diplomacy and Pacifist task react; the actor's
 * own unit costs it score; the tile is emptied */
EF_BIG EfResult ef_explode(EfGame *g, const EfTables *t, int32_t actor, EfXy c) {
    EfTile *tile = ef_tile(g, c);
    if (!tile) return ef_err(EF_ERR_EXPLODE_TILE, 0);
    if (tile->unit_n == EF_NONE) return ef_ok();
    int32_t carrier = tile->unit_n, selected = carrier;
    if (ef_unit_has(t, ef_cunits(g)[carrier].kind_n, EF_AB_CARRY)) {
        selected = ef_cunits(g)[carrier].passenger_n;
        if (selected == EF_NONE) return ef_err(EF_ERR_EXPLODE_PASSENGER, 0);
    }
    EfUnit u = ef_cunits(g)[selected];
    if (u.follower != 0) {
        int32_t f = ef_find_unit(g, u.follower);
        if (f != EF_NONE) {
            EfAction e = ef_action_at(EF_A_EXPLODE, actor, ef_cunits(g)[f].coordinates);
            e.p[1] = ef_xy(0, 0);
            EF_TRY(ef_push(g, e));
        }
    }
    if (u.leader != 0) {
        int32_t l = ef_find_unit(g, u.leader);
        if (l != EF_NONE) ef_units(g)[l].follower = 0;
    }
    EF_TRY(ef_spores(g, t, c));
    EfArea area;
    EF_TRY(ef_area_sorted(g, c, 1, &area));
    int ai;
    EF_TRY(ef_player(g, actor, &ai));
    for (int k = 0; k < area.len; k++) {
        int32_t i = area.tiles[k];
        EfXy target = ef_ctiles(g)[i].coordinates;
        int32_t n = ef_ctiles(g)[i].unit_n;
        if (n == EF_NONE) continue;
        EfUnit d = ef_cunits(g)[n];
        int peace;
        EF_TRY(ef_at_peace(g, actor, d.owner_n, &peace));
        if (peace) continue;
        int32_t dealt, back;
        int advance, reveal;
        EF_TRY(ef_battle_full(g, t, &u, &d, &dealt, &back, &advance, &reveal));
        int32_t damage = ef_half_damage(dealt);
        if (damage < d.health_n) {
            EfAction a = ef_action_at(EF_A_POISON, actor, target);
            a.p[1] = target;
            EF_TRY(ef_push(g, a));
        }
        EF_TRY(ef_push(g, ef_attack_action(actor, target, target, damage, 2, 0, 100)));
        int di;
        EF_TRY(ef_player(g, d.owner_n, &di));
        EF_TRY(ef_attack_diplomacy(g, actor, d.owner_n));
        if (g->achievements_active) return ef_err(EF_ERR_ACHIEVEMENTS, 0);
        ef_reset_peace(g, ai);
    }
    if (actor == u.owner_n) {
        EfAction s = ef_action(EF_A_DECREASE_SCORE, actor);
        s.i[0] = ef_unit_score(g, t, &u);
        EF_TRY(ef_push(g, s));
    }
    tile->unit_n = EF_NONE; /* narrow: a flag or constant */
    return ef_ok();
}

/* cymanti.rs can_explode: an Explode unit not frozen, not trained this turn (unless it follows
 * a leader) */
EF_FN int ef_can_explode(const EfGame *g, const EfTables *t, const EfUnit *u) {
    return ef_unit_has(t, u->kind_n, EF_AB_EXPLODE) && (g->current_turn != u->created_turn_n || u->leader != 0)
           && !ef_unit_effect(g, u, EF_FROZEN);
}

/* step.rs Command::Explode: rules124 runs the command base first; rules123 queues the
 * explosion alone */
EF_FN EfResult ef_explode_command(EfGame *g, int32_t p, EfXy c) {
    if (g->version >= 124) EF_TRY(ef_base(g, p, 24));
    EfAction e = ef_action_at(EF_A_EXPLODE, p, c);
    e.p[1] = ef_xy(0, 0);
    return ef_push(g, e);
}

/* ---- elyrion.rs ---- */
/* elyrion.rs attract: an Attract improvement (the sanctuary) of `p` at `c`, founded on turn
 * `founded`: every growthRate turns of its age it puts game on the first adjacent tile
 * (x-major from the north-west corner) that `p` owns, holds no improvement and no resource,
 * and is forest */
EF_FN EfResult ef_attract(EfGame *g, const EfTables *t, int32_t p, EfXy c, int32_t kind, int32_t founded) {
    const EfImprovementData *d = ef_improvement_data(t, kind);
    int32_t rate = d && d->has_growth_rate ? d->growth_rate : 0;
    if (rate == 0) return ef_err(EF_ERR_ATTRACT_RATE, 0);
    int32_t age = ef_wsub(g->current_turn, founded);
    if (rate == -1 && age == INT32_MIN) return ef_err(EF_ERR_ATTRACT_AGE, 0);
    if (age % rate != 0) return ef_ok();
    for (int32_t x = c.x - 1; x <= c.x + 1; x++)
        for (int32_t y = c.y - 1; y <= c.y + 1; y++) {
            const EfTile *tile = ef_tile_c(g, ef_xy(x, y));
            if (!tile || tile->owner_n != p || tile->has_improvement_n || tile->has_resource_n || tile->terrain_n != EF_FOREST)
                continue;
            if (t->game_error) return ef_err(EF_ERR_TABLE, t->game_error - 1);
            EfAction a = ef_action_at(EF_A_CREATE_RESOURCE, p, tile->coordinates);
            a.i[0] = t->game;
            /* the frozen rules123 build records the attraction reason in the queued frame */
            a.i[1] = g->version == 123;
            return ef_push(g, a);
        }
    return ef_ok();
}

/* ---- linked.rs: Eat and the Segment chain ---- */

/* linked.rs eat_after_kill: after a kill by an Eat unit (not of Nature), the chain's tail eats */
EF_FN EfResult ef_eat_after_kill(EfGame *g, int32_t p, int32_t head) {
    int32_t tail = head, steps = 0, f = head;
    for (;;) {
        EF_TRY(ef_next_follower(g, f, &steps, &f));
        if (f == EF_NONE) break;
        if (f == head) return ef_err(EF_ERR_CYCLIC_CHAIN, 0);
        tail = f;
    }
    EfAction a = ef_action_at(EF_A_EAT, p, ef_xy(0, 0));
    a.i[0] = (int32_t)ef_cunits(g)[tail].id;
    return ef_push(g, a);
}

/* linked.rs eat: a Segment is trained behind the eater (the first free tile a
 * Segment led by it could stand on, fanning out from the opposite of its facing), linked to
 * it, explores, takes the surrounding-area levels and adds its score */
EF_BIG EfResult ef_eat(EfGame *g, const EfTables *t, int32_t actor, uint32_t eater, EfXy spawn) {
    const int8_t fx[8] = { -1, -1, -1, 0, 1, 1, 1, 0 };
    const int8_t fy[8] = { -1, 0, 1, 1, 1, 0, -1, -1 };
    int32_t n = ef_find_unit(g, eater);
    if (n == EF_NONE) return ef_ok();
    EfUnit u = ef_cunits(g)[n];
    int32_t opposite = u.direction_n < 4 ? u.direction_n + 4 : u.direction_n - 4;
    if (t->segment_error) return ef_err(EF_ERR_TABLE, t->segment_error - 1);
    int32_t segment = t->segment;
    int found = 0;
    EfXy target = ef_xy(0, 0);
    for (int32_t i = 1; i <= 8 && !found; i++) {
        int32_t offset = ((i - 1) & 1) == 0 ? i / 2 : -(i >> 1);
        int32_t d = (offset + opposite + 8) % 8;
        target = ef_xy(ef_wadd(u.coordinates.x, fx[d]), ef_wadd(u.coordinates.y, fy[d]));
        const EfTile *tile = ef_tile_c(g, target);
        if (!tile || tile->unit_n != EF_NONE) continue;
        EfUnit temporary;
        EfMover mover;
        EF_TRY(ef_profile_unit(t, actor, segment, spawn, &temporary));
        temporary.leader = eater;
        EF_PUT(temporary.created_turn_n, g->current_turn, int16_t);
        EF_TRY(ef_placement_with(g, t, &temporary, target, &mover, &found));
    }
    if (!found || ef_player_index(g, actor) == EF_NONE) return ef_ok();
    int32_t s;
    EF_TRY(ef_create_unit(g, t, actor, segment, target, &s));
    ef_units(g)[s].leader = eater;
    ef_units(g)[n].follower = ef_cunits(g)[s].id;
    EF_TRY(ef_explore_from(g, actor, target, 1 + ef_unit_has(t, segment, EF_AB_SCOUT)));
    EF_TRY(ef_surrounding(g, t, actor, target));
    EfAction a = ef_action_at(EF_A_INCREASE_SCORE, actor, target);
    a.i[0] = ef_unit_score(g, t, &ef_cunits(g)[s]);
    return ef_push(g, a);
}

/* ---- ruins.rs lost_city (Aquarion's ruin at sea) ---- */
#define EF_WHALE 4

/* candidate.rs resource_probability: the continent's weight for resource `kind` */
EF_FN float ef_resource_probability(const EfContinent *c, int32_t kind) {
    /* weights: crop, fish, fruit, game, metal, whale, spores, aquacrop, ... */
    const int8_t slot[10] = { -1, 3, 0, 1, 5, 4, 2, 6, -1, 7 };
    return kind >= 0 && kind < 10 && slot[kind] >= 0 ? ef_bits_f32(c->weights[slot[kind]]) : 0.0f;
}

/* candidate.rs place_resource with hash `hash`: one hash per tile decides
 * which resources pass and which is kept; a tile without a continent turns to ice */
EF_BIG EfResult ef_place_resource(EfGame *g, const EfTables *t, int32_t i, uint32_t hash) {
    EfTile *tile = &ef_tiles(g)[i];
    if (tile->has_improvement_n) return ef_ok();
    if (!tile->has_continent_n) {
        tile->terrain_n = EF_ICE; /* narrow: a terrain constant */
        return ef_ok();
    }
    if (tile->continent_n < 0 || tile->continent_n >= g->continent_count) return ef_err(EF_ERR_RESOURCE_CONTINENT, 0);
    const EfContinent *continent = &g->continents[tile->continent_n];
    EfXy c = tile->coordinates;
    int32_t terrain = tile->terrain_n;
    int32_t candidates[EF_MAX_RESOURCES], n = 0;
    int32_t h = g->tile_count / g->width;
    for (int32_t counter = 0; counter < t->resource_list_len; counter++) {
        int32_t kind = t->resource_list[counter];
        float probability = ef_resource_probability(continent, kind);
        uint32_t words[3] = { (uint32_t)c.x, (uint32_t)c.y, (uint32_t)counter };
        if (probability == 0.0f || !(ef_hash_value(ef_hash_words(hash, words, 3)) <= probability)) continue;
        if (kind == EF_WHALE) {
            int near = 0;
            for (int32_t dx = -1; dx <= 1 && !near; dx++)
                for (int32_t dy = -1; dy <= 1; dy++) {
                    if (dx == 0 && dy == 0) continue;
                    int32_t x = c.x + dx, y = c.y + dy;
                    if (x < 0 || y < 0 || x >= g->width || y >= h) continue;
                    const EfTile *o = ef_tile_c(g, ef_xy(x, y));
                    if ((o->has_resource_n && o->resource_n == EF_WHALE) || (o->has_improvement_n && o->improvement.kind_n == EF_CITY)) {
                        near = 1;
                        break;
                    }
                }
            if (near) continue;
        }
        if (t->resource_terrains_error[kind]) return ef_err(EF_ERR_TABLE, t->resource_terrains_error[kind] - 1);
        uint32_t allowed = (uint32_t)t->resource_terrains[kind];
        if (allowed == 0 || (allowed >> terrain & 1u)) candidates[n++] = kind;
    }
    if (n == 0) return ef_ok();
    uint32_t words[3] = { (uint32_t)c.x, (uint32_t)c.y, (uint32_t)t->resource_list_len };
    int32_t chosen = ef_hash_range_i32(ef_hash_words(hash, words, 3), 0, n);
    tile->has_resource_n = 1; /* narrow: a flag */
    EF_PUT(tile->resource_n, candidates[chosen], int8_t);
    return ef_ok();
}

/* ruins.rs lost_city: the cardinal tiles become water (not ice), a level-1 city rises on a
 * flooded field, a new (detached) continent of the player's takes the area's unclaimed tiles,
 * resources are placed around it with the gameplay hash, and the player captures it */
EF_BIG EfResult ef_lost_city(EfGame *g, const EfTables *t, int32_t p, int pi, EfXy target) {
    int32_t center = ef_tile_index(g, target);
    if (center == EF_NONE) return ef_err(EF_ERR_LOST_CITY_CENTRE, 0);
    int32_t h = g->tile_count / g->width;
    int32_t x0 = target.x - 2 < 0 ? 0 : target.x - 2, x1 = target.x + 2 > g->width - 1 ? g->width - 1 : target.x + 2;
    int32_t y0 = target.y - 2 < 0 ? 0 : target.y - 2, y1 = target.y + 2 > h - 1 ? h - 1 : target.y + 2;
#define EF_LOST_NEAR(a, b) ((a) >= target.x - 1 && (a) <= target.x + 1 && (b) >= target.y - 1 && (b) <= target.y + 1)
    for (int32_t x = x0; x <= x1; x++)
        for (int32_t y = y0; y <= y1; y++) {
            if (!EF_LOST_NEAR(x, y) || (x == target.x && y == target.y) || (x != target.x && y != target.y)) continue;
            EfTile *o = ef_tile(g, ef_xy(x, y));
            if (o->terrain_n != EF_ICE) {
                o->terrain_n = EF_WATER; /* narrow: a terrain constant */
                o->altitude_n = -1; /* narrow: a constant */
            }
        }
    EfTile *x = &ef_tiles(g)[center];
    memset(&x->improvement, 0, sizeof x->improvement);
    x->has_improvement_n = 1; /* narrow: a flag */
    x->improvement.kind_n = EF_CITY; /* narrow: a constant */
    x->improvement.level_n = 1; /* narrow: a constant */
    x->improvement.production_n = 1; /* narrow: a constant */
    x->improvement.border_size_n = 1; /* narrow: a constant */
    x->terrain_n = EF_FIELD; /* narrow: a terrain constant */
    EF_TRY(ef_tile_effect_add(x, EF_FLOODED));
    if (g->continent_count < 0) return ef_err(EF_ERR_CONTINENTS, 0);
    if (g->continent_count >= EF_MAX_CONTINENTS) return ef_err(EF_ERR_CONTINENTS_FULL, 0);
    /* candidate.rs fresh_continent and set_continent_player (not the fallback) */
    int32_t tribe = g->players[pi].tribe;
    EfContinent c = t->fresh_continent;
    if (tribe > 0 && tribe < EF_MAX_TRIBES) {
        if (t->tribe_continent_error[tribe]) return ef_err(EF_ERR_TABLE, t->tribe_continent_error[tribe] - 1);
        if (t->tribe_continent_known[tribe]) {
            c = t->tribe_continent[tribe];
            ef_settle_climate(g, pi);
            c.climate = g->players[pi].climate;
            c.skin = g->players[pi].skin;
            c.has_start = 1;
            c.start = g->players[pi].start_tile;
        }
    }
    c.detached = 1;
    int32_t id = g->continent_count;
    g->continents[id] = c;
    g->continent_count = id + 1;
    uint32_t hash = (uint32_t)ef_random_hash(g);
    /* the 3x3 around the city at threshold 1, then the 5x5 less the 3x3 at 1/3 (f32 bits) */
    for (int pass = 0; pass < 2; pass++) {
        float threshold = pass == 0 ? 1.0f : ef_bits_f32(0x3eaaaaabu);
        int32_t r = pass == 0 ? 1 : 2;
        for (int32_t xx = target.x - r < 0 ? 0 : target.x - r; xx <= (target.x + r > g->width - 1 ? g->width - 1 : target.x + r); xx++)
            for (int32_t yy = target.y - r < 0 ? 0 : target.y - r; yy <= (target.y + r > h - 1 ? h - 1 : target.y + r); yy++) {
                if (xx == target.x && yy == target.y) continue;
                if (pass == 1 && EF_LOST_NEAR(xx, yy)) continue;
                int32_t i = ef_tile_index(g, ef_xy(xx, yy));
                EfTile *o = &ef_tiles(g)[i];
                uint32_t words[2] = { (uint32_t)xx, (uint32_t)yy };
                if (ef_hash_value(ef_hash_words(hash, words, 2)) <= threshold && !o->has_resource_n && !o->has_improvement_n
                    && o->owner_n == 0) {
                    if (!o->has_continent_n) {
                        o->has_continent_n = 1; /* narrow: a flag */
                        EF_PUT(o->continent_n, id, int16_t);
                    }
                    EF_TRY(ef_place_resource(g, t, i, hash));
                }
            }
    }
#undef EF_LOST_NEAR
    EF_TRY(ef_push_connections_and_routes(g, p));
    EfAction a = ef_action_at(EF_A_CAPTURE_CITY, p, target);
    a.i[0] = 0;
    return ef_push(g, a);
}

#endif /* EF_TRIBES_H */
