/* moves.h - the Move command and action: step.rs move_action, the Move arm of
 * execute_command, attack_diplomacy, and routes.rs can_embark. */
#ifndef EF_MOVES_H
#define EF_MOVES_H

#include "explore.h"
#include "movement.h"

/* routes.rs can_embark */
EF_FN EfResult ef_can_embark(EfGame *g, const EfTables *t, int32_t p, int *out) {
    EF_TRY(ef_refresh_tech_cache(g, t, p));
    int i;
    EF_TRY(ef_player(g, p, &i));
    const EfTechTree *tree = ef_tribe_tree(t, g->players[i].tribe);
    if (!tree) return ef_err(EF_ERR_UNKNOWN_TRIBE, 0);
    const EfPlayer *x = &g->players[i];
    if (!x->has_tech_cache) return ef_err(EF_ERR_NULL_TECH_CACHE, 0);
    *out = 0;
    for (int k = 0; k < x->tech_cache_len; k++) {
        int32_t tech = x->tech_cache[k];
        if (tech < 0 || tech >= t->tech_count) return ef_err(EF_ERR_UNKNOWN_TECH, 0);
        if (tree->links[tech].embark_unlock_error) return ef_err(EF_ERR_TABLE, tree->links[tech].embark_unlock_error - 1);
        if (tree->links[tech].embark_unlock) { *out = 1; return ef_ok(); }
    }
    return ef_ok();
}

/* step.rs attack_diplomacy: record the attack turn on both relations and queue removal of
 * both embassies */
EF_FN EfResult ef_attack_diplomacy(EfGame *g, int32_t actor, int32_t opponent) {
    if (opponent == 0) return ef_ok();
    int32_t turn = g->current_turn;
    int a, o;
    EF_TRY(ef_player(g, actor, &a));
    EfRelation *r;
    EF_TRY(ef_relation_entry(g, a, opponent, &r));
    r->previous_attack_turn = r->last_attack_turn;
    r->last_attack_turn = turn;
    EF_TRY(ef_player(g, opponent, &o));
    EF_TRY(ef_relation_entry(g, o, actor, &r));
    r->previous_attack_turn = turn;
    EfAction x = ef_action(EF_A_DESTROY_EMBASSY, actor);
    x.i[0] = opponent;
    EF_TRY(ef_push(g, x));
    EfAction y = ef_action(EF_A_DESTROY_EMBASSY, opponent);
    y.i[0] = actor;
    return ef_push(g, y);
}

/* step.rs move_action; the path is destination first */
EF_BIG EfResult ef_move_action(EfGame *g, const EfTables *t, int32_t p, uint32_t unit_id, const EfXy *path, int32_t path_len,
                              int32_t reason) {
    if (reason < 0 || reason > 2) return ef_err(EF_ERR_MOVE_REASON, 0);
    int32_t node = ef_find_unit(g, unit_id);
    if (node == EF_NONE) return ef_ok();
    if (ef_player_index(g, p) == EF_NONE) return ef_ok();
    EfUnit unit = ef_cunits(g)[node];
    const EfUnitData *d = ef_unit_data(t, unit.kind_n);
    if (!d) return ef_ok();
    if (path_len < 1) return ef_err(EF_ERR_EMPTY_PATH, 0);
    EfXy dest = path[0], origin = path[path_len - 1];
#define EF_UHAS(ab) ef_has(d->abilities, (ab))
    if (unit.kind_n == 13) return ef_err(EF_ERR_MOVE_BUNNY, 0);
    /* moved |= (reason != Push and, for an attack advance, the unit lacks Escape) */
    if (reason == 0 || (reason == 1 && !EF_UHAS(EF_AB_ESCAPE))) ef_units(g)[node].moved_n = 1; /* narrow: a flag or constant */
    /* a skater that ends off the ice is spent */
    if (EF_UHAS(EF_AB_SKATE) && reason != 2) {
        const EfTile *end = ef_tile_c(g, dest);
        if (!end) return ef_err(EF_ERR_MOVE_DESTINATION, 0);
        if (end->terrain_n != EF_ICE) {
            ef_units(g)[node].moved_n = 1; /* narrow: a flag or constant */
            ef_units(g)[node].attacked_n = 1; /* narrow: a flag or constant */
        }
    }
    int32_t from_i = ef_tile_index(g, origin);
    if (from_i == EF_NONE) return ef_err(EF_ERR_MOVE_ORIGIN, 0);
    int32_t to_i = ef_tile_index(g, dest);
    if (to_i == EF_NONE) return ef_err(EF_ERR_MOVE_DESTINATION, 0);
    ef_tiles(g)[from_i].unit_n = EF_NONE; /* narrow: EF_NONE */
    ef_tiles(g)[to_i].unit_n = (int16_t)node; /* narrow: a unit slot (state.h assert) */
    ef_units(g)[node].coordinates = dest;
    if (path_len > 1) {
        int32_t dir = ef_facing_direction(path[1], dest);
        ef_units(g)[node].direction_n = (int8_t)dir; /* narrow: ef_facing_direction gives 0 to 8 */
        ef_units(g)[node].flipped_n = dir < 4; /* narrow: a flag or constant */
    }
    EF_TRY(ef_poison_tile(g, t, node, dest));
    EF_TRY(ef_refresh_tech_cache(g, t, p));   /* the player-ability query for the bubble check */
    int32_t mover = ef_mover(g);
    /* an amphibious unit of a Bubble player gains Bubble on a flooded tile */
    if (ef_player_ability(g, t, p, EF_AB_BUBBLE) && ef_flooded(&ef_ctiles(g)[to_i]) && EF_UHAS(EF_AB_AMPHIBIOUS)
        && !ef_unit_effect(g, &ef_cunits(g)[node], EF_BUBBLE)) {
        EfAction b = ef_action(EF_A_BUBBLE, mover);
        b.i[0] = (int32_t)unit.id;
        b.i[1] = 1;
        EF_TRY(ef_push(g, b));
    }
    if (ef_contains_i8(ef_ctiles(g)[to_i].effects_n, ef_ctiles(g)[to_i].effects_len_n, EF_TENTACLE_EFFECT))
        EF_TRY(ef_tentacle_strike(g, t, node, p, dest));
    if (reason != 2 && EF_UHAS(EF_AB_AUTOFREEZE)) {
        EfAction f = ef_action_at(EF_A_FREEZE_AREA, p, dest);
        f.i[0] = 1;
        f.i[1] = 1;
        f.i[2] = 0;
        EF_TRY(ef_push(g, f));
    }
    if (reason != 2 && (EF_UHAS(EF_AB_STOMP) || EF_UHAS(EF_AB_TENTACLES))) EF_TRY(ef_area_damage(g, t, node, dest));
    if (reason != 2 && EF_UHAS(EF_AB_AUTOHEAL)) EF_TRY(ef_push(g, ef_action_at(EF_A_MOVE_HEAL_OTHERS, p, dest)));
    if (reason != 2 && EF_UHAS(EF_AB_AUTOFLOOD)) EF_TRY(ef_push(g, ef_action_at(EF_A_FLOOD, p, dest)));
    /* the Island leaves algae on the water it crosses (AlgaeSpawn at both rules versions) */
    if (reason != 2 && EF_UHAS(EF_AB_ALGAE)) {
        if (t->algae_spawn_error) return ef_err(EF_ERR_TABLE, t->algae_spawn_error - 1);
        for (int k = 0; k < path_len; k++) {
            const EfTile *x = ef_tile_c(g, path[k]);
            if (!x) return ef_err(EF_ERR_ALGAE_PATH_TILE, 0);
            if (ef_is_water_tile(x) && !ef_contains_i8(x->effects_n, x->effects_len_n, EF_ALGAE)) {
                EfAction b = ef_action_at(EF_A_BUILD, ef_cunits(g)[node].owner_n, path[k]);
                b.i[0] = t->algae_spawn;
                b.i[1] = 0;
                EF_TRY(ef_push(g, b));
            }
        }
    }
    /* ... and loses it on dry land */
    int amphibious = EF_UHAS(EF_AB_SWIM) && !d->has_movement_terrain && !EF_UHAS(EF_AB_CARRY);
    {
        const EfTile *x = &ef_ctiles(g)[to_i];
        int land = x->terrain_n == 3 || x->terrain_n == 4 || x->terrain_n == 5 || x->terrain_n == 6;
        if (ef_unit_effect(g, &ef_cunits(g)[node], EF_BUBBLE) && land && !ef_wetland(x) && amphibious) {
            EfAction b = ef_action(EF_A_BUBBLE, mover);
            b.i[0] = (int32_t)unit.id;
            b.i[1] = 0;
            EF_TRY(ef_push(g, b));
        }
    }
    if (EF_UHAS(EF_AB_TENTACLES)) EF_TRY(ef_update_tentacles(g, t));
    int32_t sight = 1 + EF_UHAS(EF_AB_SCOUT);
    for (int k = 0; k < path_len; k++) EF_TRY(ef_explore_from(g, p, path[k], sight));
    /* will the unit embark at the destination */
    EfTile tile = ef_ctiles(g)[to_i];
    int water = tile.terrain_n == EF_WATER || tile.terrain_n == EF_OCEAN || ef_wetland(&tile);
    int land_unit = !EF_UHAS(EF_AB_SWIM) && !EF_UHAS(EF_AB_FLY) && unit.leader == 0 && unit.follower == 0;
    int embark = 0;
    if (land_unit && water && tile.has_improvement_n && ef_improvement_has(t, tile.improvement.kind_n, EF_AB_EMBARK)) {
        int can;
        EF_TRY(ef_can_embark(g, t, unit.owner_n, &can));
        if (can) {
            if (tile.owner_n == unit.owner_n) {
                embark = 1;
            } else if (tile.owner_n != 0) {
                int i;
                EF_TRY(ef_player(g, unit.owner_n, &i));
                EfRelation *r;
                EF_TRY(ef_relation_entry(g, i, tile.owner_n, &r));
                embark = r->state == EF_PEACE;
            }
        }
    }
    int city = tile.has_improvement_n && tile.improvement.kind_n == EF_CITY;
    if (embark) {
        EF_TRY(ef_push(g, ef_action_at(EF_A_EMBARK, p, dest)));
    } else if (EF_UHAS(EF_AB_CARRY)
               && (city || ((tile.terrain_n == 3 || tile.terrain_n == 4 || tile.terrain_n == 5 || tile.terrain_n == 6) && !ef_wetland(&tile)))) {
        EF_TRY(ef_push(g, ef_action_at(EF_A_DISEMBARK, p, dest)));
    }
    /* a follower two or more tiles behind steps into the head's old tile */
    uint32_t follower = ef_cunits(g)[node].follower;
    if (follower != 0) {
        int32_t f = ef_find_unit(g, follower);
        if (f != EF_NONE) {
            EfXy at = ef_cunits(g)[f].coordinates;
            int32_t dx = at.x - dest.x, dy = at.y - dest.y;
            if (dx < 0) dx = -dx;
            if (dy < 0) dy = -dy;
            if ((dx > dy ? dx : dy) >= 2) {
                EfAction m = ef_action(EF_A_MOVE, p);
                m.i[0] = (int32_t)follower;
                m.i[1] = 1;
                m.i[2] = 0;
                m.len = 2;
                m.list.path[0] = origin;
                m.list.path[1] = at;
                EF_TRY(ef_push(g, m));
            }
        }
    }
#undef EF_UHAS
    if (city && tile.owner_n != 0 && tile.owner_n != p) EF_TRY(ef_attack_diplomacy(g, p, tile.owner_n));
    return ef_ok();
}

/* the Move arm of step.rs execute_command */
EF_BIG EfResult ef_move_command(EfGame *g, const EfTables *t, int32_t p, uint32_t id, EfXy from, EfXy to) {
    EF_TRY(ef_base(g, p, 6));
    int32_t node = ef_find_unit(g, id);
    if (node == EF_NONE) return ef_ok();
    int32_t kind = ef_cunits(g)[node].kind_n;
    const EfUnitData *d = ef_unit_data(t, kind);
    if (!d) return ef_ok();
    const EfTile *dest = ef_tile_c(g, to);
    if (!dest) return ef_err(EF_ERR_MOVE_DESTINATION, 0);
    const int occupied = dest->unit_n != EF_NONE;
    /* the path first, before anything is written: a Move with none is refused, as the move
     * move rules refuse it before it runs (MovementError::NoPath); the search does not read
     * `moved` */
    EfUnit unit;
    EfPath path;
    if (!occupied) {
        EF_TRY(ef_refresh_tech_cache(g, t, p));
        unit = ef_cunits(g)[node];
        int found;
        EF_TRY(ef_unit_path(g, t, &unit, to, &path, &found));
        if (!found || path.len < 2) return ef_err(EF_ERR_MOVE_NO_PATH, 0);
    }
    ef_units(g)[node].moved_n = 1; /* narrow: a flag or constant */
    EfAction reselect = ef_action(EF_A_RESELECT, p);
    reselect.i[0] = (int32_t)id;
    EF_TRY(ef_push(g, reselect));
    if (occupied) {
        /* a hidden enemy is there: the move becomes a Reveal */
        ef_units(g)[node].moved_n = 0; /* narrow: a flag or constant */
        EfAction reveal = ef_action_at(EF_A_REVEAL, p, to);
        reveal.p[1] = ef_cunits(g)[node].coordinates;
        reveal.i[0] = 1;
        return ef_push(g, reveal);
    }
    unit = ef_cunits(g)[node];
    EfAction move = ef_action(EF_A_MOVE, p);
    move.i[0] = (int32_t)id;
    move.i[1] = 1;
    move.i[2] = 0;
    move.len = path.len;
    for (int k = 0; k < path.len; k++) move.list.path[k] = path.tiles[k];
    EF_TRY(ef_push(g, move));
    if (ef_has(d->abilities, EF_AB_HIDE) && !ef_unit_effect(g, &unit, EF_INVISIBLE_EFFECT))
        EF_TRY(ef_push(g, ef_action_at(EF_A_HIDE, p, from)));
    return ef_ok();
}

#endif /* EF_MOVES_H */
