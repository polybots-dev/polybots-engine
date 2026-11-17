/* step.h - port of engine-fast/src/step.rs: admit (the chosen move as a command, with the
 * validator's state writes), the command and action bodies, the command manager loop and
 * Nature's continuation. A command or action whose rules are not ported yet stops with
 * EF_ERR_NOT_PORTED (argument: 1000 + move kind in admit, 2000 + command, 3000 + action),
 * so a lockstep run can count what it could not compare. */
#ifndef EF_STEP_H
#define EF_STEP_H

#include "routes.h"
#include "units.h"
#include "build.h"
#include "reward.h"
#include "combat.h"
#include "tribes.h"
#include "infiltrate.h"


/* step.rs route_children: connections for both players, then both players' routes */
EF_FN EfResult ef_route_children(EfGame *g, int32_t p, int32_t q) {
    EfAction a = ef_action(EF_A_UPDATE_CITY_CONNECTIONS, p);
    a.len = 2;
    a.list.players[0] = p;
    a.list.players[1] = q;
    EF_TRY(ef_push(g, a));
    EF_TRY(ef_push(g, ef_action(EF_A_UPDATE_ROUTES, p)));
    return ef_push(g, ef_action(EF_A_UPDATE_ROUTES, q));
}

/* step.rs embassy_cost: 4 + 2 per player with an embassy toward p (lookups insert records) */
EF_FN EfResult ef_embassy_cost(EfGame *g, int32_t p, int32_t *out) {
    int32_t count = 0;
    for (int i = 0; i < g->player_count; i++) {
        EfRelation *r;
        EF_TRY(ef_relation_entry(g, i, p, &r));
        count += r->embassy_level > 0;
    }
    *out = count * 2 + 4;
    return ef_ok();
}

/* step.rs build_validation_writes */
EF_FN EfResult ef_build_validation_writes(EfGame *g, const EfTables *t, int32_t p, int32_t kind, EfXy c) {
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_BUILD_TILE, 0);
    int32_t terrain = tile->terrain_n;
    int explored = ef_tile_explored(tile, p);
    if (tile->unit_n != EF_NONE && explored && ef_cunits(g)[tile->unit_n].owner_n != p) {
        int32_t o = ef_cunits(g)[tile->unit_n].owner_n;
        int i;
        EF_TRY(ef_player(g, p, &i));
        EfRelation *r;
        EF_TRY(ef_relation_entry(g, i, o, &r));
    }
    const EfImprovementData *d = ef_improvement_data(t, kind);
    if (!d) return ef_err(EF_ERR_UNKNOWN_IMPROVEMENT, 0);
    /* every "field" requirement refreshes the cache when the terrain asks for it (the Rust
     * loops over the requirements; refreshing twice is refreshing once) */
    int water_exempt = kind == 3 || kind == 17 || kind == 52 || kind == 14;
    if (d->field_requirement && (terrain == EF_FOREST || ((terrain == EF_WATER || terrain == EF_OCEAN) && !water_exempt)))
        EF_TRY(ef_refresh_tech_cache(g, t, p));
    return ef_ok();
}

/* step.rs admit */
EF_BIG EfResult ef_admit(EfGame *g, const EfTables *t, const EfMove *m, EfCommand *c) {
    int32_t p = ef_mover(g);
    memset(c, 0, sizeof *c);
#define EF_NEED(flag, code) do { if (!m->flag) return ef_err(code, 0); } while (0)
    switch (m->kind) {
    case 8: {
        EF_NEED(has_option, EF_ERR_NO_OPTION);
        int64_t technology = m->option;
        if (ef_tech_known(t, (int32_t)technology) && t->techs[technology].cost != 0) EF_TRY(ef_refresh_tech_cache(g, t, p));
        c->kind = EF_C_RESEARCH;
        c->w = technology;
        return ef_ok();
    }
    case 27: {
        EF_NEED(has_opponent, EF_ERR_NO_OPPONENT);
        int32_t opponent = m->opponent;
        EF_TRY(ef_refresh_tech_cache(g, t, p));
        if (opponent != 0) {
            int i;
            EF_TRY(ef_player(g, p, &i));
            EfRelation *r;
            EF_TRY(ef_relation_entry(g, i, opponent, &r));
        }
        EfXy capital;
        EF_TRY(ef_current_capital(g, opponent, &capital));
        c->kind = EF_C_PEACE_TREATY;
        c->a = opponent;
        return ef_ok();
    }
    case 1: {
        EF_NEED(has_option, EF_ERR_NO_OPTION);
        EF_NEED(has_target, EF_ERR_NO_TARGET);
        EF_TRY(ef_build_validation_writes(g, t, p, m->option, m->target));
        c->kind = EF_C_BUILD;
        c->a = m->option;
        c->p = m->target;
        return ef_ok();
    }
    case 9:
        EF_NEED(has_target, EF_ERR_NO_TARGET);
        c->kind = EF_C_DESTROY;
        c->p = m->target;
        return ef_ok();
    case 29: {
        EF_NEED(has_opponent, EF_ERR_NO_OPPONENT);
        int32_t opponent = m->opponent;
        if (opponent != 0) {
            int i;
            EF_TRY(ef_player(g, p, &i));
            EfRelation *r;
            EF_TRY(ef_relation_entry(g, i, opponent, &r));
        }
        EfXy capital;
        EF_TRY(ef_current_capital(g, opponent, &capital));
        c->kind = EF_C_BREAK_PEACE;
        c->a = opponent;
        return ef_ok();
    }
    case 28:
        EF_NEED(has_opponent, EF_ERR_NO_OPPONENT);
        c->kind = EF_C_PEACE_RESPONSE;
        c->a = m->opponent;
        c->b = m->accept == 1;
        return ef_ok();
    case 6: {
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        EF_NEED(has_target, EF_ERR_NO_TARGET);
        const EfTile *from = ef_tile_c(g, m->source);
        if (!from || from->unit_n == EF_NONE) return ef_err(EF_ERR_MOVE_NO_UNIT, 0);
        const EfUnit *mover = &ef_cunits(g)[from->unit_n];
        /* Move validity: the unit can move and has data; the path
         * is checked where the Move searches it (moves.h ef_move_command) */
        if (mover->moved_n || ef_unit_effect(g, mover, EF_FROZEN)) return ef_err(EF_ERR_MOVE_CANNOT_MOVE, 0);
        if (!ef_unit_data(t, mover->kind_n)) return ef_err(EF_ERR_MOVE_NO_DATA, 0);
        uint32_t id = mover->id;
        EF_TRY(ef_refresh_tech_cache(g, t, p));
        c->kind = EF_C_MOVE;
        c->a = (int32_t)id;
        c->p = m->source;
        c->q = m->target;
        return ef_ok();
    }
    case 5:
        EF_TRY(ef_refresh_tech_cache(g, t, p));
        EF_NEED(has_option, EF_ERR_NO_OPTION);
        EF_NEED(has_target, EF_ERR_NO_TARGET);
        c->kind = EF_C_TRAIN;
        c->a = m->option;
        c->p = m->target;
        return ef_ok();
    case 36:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        c->kind = EF_C_DISEMBARK;
        c->p = m->source;
        return ef_ok();
    case 16:
        EF_TRY(ef_refresh_tech_cache(g, t, p));
        EF_NEED(has_option, EF_ERR_NO_OPTION);
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        c->kind = EF_C_UPGRADE;
        c->a = m->option;
        c->p = m->source;
        return ef_ok();
    case 30: {
        EF_NEED(has_opponent, EF_ERR_NO_OPPONENT);
        int32_t opponent = m->opponent;
        EfXy capital;
        EF_TRY(ef_current_capital(g, opponent, &capital));
        int32_t cost;
        EF_TRY(ef_embassy_cost(g, p, &cost));
        EF_TRY(ef_refresh_tech_cache(g, t, p));
        int o;
        EF_TRY(ef_player(g, opponent, &o));
        EfRelation *r;
        EF_TRY(ef_relation_entry(g, o, p, &r));
        c->kind = EF_C_ESTABLISH_EMBASSY;
        c->a = opponent;
        return ef_ok();
    }
    case 2: {
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        EF_NEED(has_target, EF_ERR_NO_TARGET);
        const EfTile *at = ef_tile_c(g, m->source);
        if (!at || at->unit_n == EF_NONE) return ef_err(EF_ERR_ATTACK_NO_UNIT, 0);
        EfUnit unit = ef_cunits(g)[at->unit_n];
        EF_TRY(ef_attack_admit_writes(g, t, &unit));
        c->kind = EF_C_ATTACK;
        c->a = (int32_t)unit.id;
        c->p = m->source;
        c->q = m->target;
        return ef_ok();
    }
    case 7: {
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        const EfTile *at = ef_tile_c(g, m->source);
        if (!at || at->unit_n == EF_NONE) return ef_err(EF_ERR_CAPTURE_NO_UNIT, 0);
        uint32_t unit = ef_cunits(g)[at->unit_n].id;
        EF_TRY(ef_capture_admit_writes(g, p, m->source));
        c->kind = EF_C_CAPTURE;
        c->a = (int32_t)unit;
        c->p = m->source;
        return ef_ok();
    }
    case 11:
        EF_NEED(has_option, EF_ERR_NO_OPTION);
        EF_NEED(has_target, EF_ERR_NO_TARGET);
        c->kind = EF_C_CITY_REWARD;
        c->a = m->option;
        c->p = m->target;
        return ef_ok();
    case 14:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        c->kind = EF_C_EXAMINE_RUINS;
        c->p = m->source;
        return ef_ok();
    case 4:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        c->kind = EF_C_HEAL_OTHERS;
        c->p = m->source;
        return ef_ok();
    case 13: {
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        const EfTile *at = ef_tile_c(g, m->source);
        if (!at || at->unit_n == EF_NONE) return ef_err(EF_ERR_PROMOTE_NO_UNIT, 0);
        int32_t n = at->unit_n;
        int32_t owner = ef_cunits(g)[ef_cunits(g)[n].passenger_n != EF_NONE ? ef_cunits(g)[n].passenger_n : n].owner_n;
        EF_TRY(ef_refresh_tech_cache(g, t, owner));
        c->kind = EF_C_PROMOTE;
        c->p = m->source;
        return ef_ok();
    }
    case 35:
        /* the menu's Resign is the mover's own; a recorded one names who resigned (opponent)
         * and, for a kick, who kicked (option) and that it was one (accept) */
        c->kind = EF_C_RESIGN;
        c->a = m->has_opponent ? m->opponent : p;
        c->b = m->has_option ? m->option : 0;
        c->c = m->accept == 1;
        return ef_ok();
    case 15: c->kind = EF_C_END_TURN; return ef_ok();
    case 20: c->kind = EF_C_START_MATCH; return ef_ok();
    case 22: c->kind = EF_C_END_MATCH; return ef_ok();
    case 3:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        c->kind = EF_C_RECOVER;
        c->p = m->source;
        return ef_ok();
    case 10:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        c->kind = EF_C_DISBAND;
        c->p = m->source;
        return ef_ok();
    case 17:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        EF_TRY(ef_admit_freeze_area(g, t, m->source));
        c->kind = EF_C_FREEZE_AREA;
        c->p = m->source;
        return ef_ok();
    case 38:
    case 25:
    case 24:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        c->kind = m->kind == 38 ? EF_C_SWARM : m->kind == 25 ? EF_C_BOOST : EF_C_EXPLODE;
        c->p = m->source;
        return ef_ok();
    case 26:
        EF_NEED(has_target, EF_ERR_NO_TARGET);
        c->kind = EF_C_DECOMPOSE;
        c->p = m->target;
        return ef_ok();
    case 37:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        c->kind = EF_C_FLOOD;
        c->p = m->source;
        return ef_ok();
    case 39:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        EF_NEED(has_option, EF_ERR_NO_OPTION);
        c->kind = EF_C_CLEAR_TILE_EFFECT;
        c->a = m->option;
        c->p = m->source;
        return ef_ok();
    case 18:
        EF_NEED(has_source, EF_ERR_NO_SOURCE);
        c->kind = EF_C_BREAK_ICE;
        c->p = m->source;
        return ef_ok();
    default:
        return ef_err(EF_ERR_COMMAND_KIND, m->kind);
    }
#undef EF_NEED
}

/* step.rs execute_command: the command's state writes and the actions it queues */
EF_BIG EfResult ef_execute_command(EfGame *g, const EfTables *t, int32_t p, const EfCommand *c, int64_t history) {
    EF_COVER(ef_coverage_commands, c->kind);
    switch (c->kind) {
    case EF_C_RESEARCH: {
        int32_t tech = (int32_t)c->w;
        EF_TRY(ef_base(g, p, 8));
        int pi = ef_player_index(g, p);
        if (pi != EF_NONE && ef_tech_known(t, tech)) {
            if (t->techs[tech].cost != 0) EF_TRY(ef_refresh_tech_cache(g, t, p));
            EfAction a = ef_action(EF_A_RESEARCH, p);
            a.w[0] = c->w;
            a.i[0] = ef_tech_cost_for(g, t, &g->players[pi], tech);
            EF_TRY(ef_push(g, a));
        }
        return ef_ok();
    }
    case EF_C_BUILD: {
        EF_TRY(ef_base(g, p, 1));
        EfAction a = ef_action_at(EF_A_BUILD, p, c->p);
        a.i[0] = c->a;
        a.i[1] = 1;
        return ef_push(g, a);
    }
    case EF_C_DESTROY:
        EF_TRY(ef_base(g, p, 9));
        return ef_push(g, ef_action_at(EF_A_DESTROY_IMPROVEMENT, p, c->p));
    case EF_C_BREAK_PEACE: {
        EF_TRY(ef_route_children(g, p, c->a));
        EfAction a = ef_action(EF_A_BREAK_PEACE, p);
        a.i[0] = c->a;
        return ef_push(g, a);
    }
    case EF_C_PEACE_RESPONSE: {
        int n = -1;
        for (int k = g->triggers_len - 1; k >= 0; k--)
            if (g->triggers[k].player == p && g->triggers[k].kind == EF_PEACE_REQUEST_TRIGGER) { n = k; break; }
        if (n < 0) return ef_err(EF_ERR_PEACE_TRIGGER, 0);
        for (int k = n; k + 1 < g->triggers_len; k++) g->triggers[k] = g->triggers[k + 1];
        g->triggers_len -= 1;
        if (c->b) EF_TRY(ef_route_children(g, p, c->a));
        EfAction a = ef_action(EF_A_PEACE_RESPONSE, p);
        a.i[0] = c->a;
        a.i[1] = c->b;
        return ef_push(g, a);
    }
    case EF_C_ATTACK: return ef_attack_command(g, t, p, (uint32_t)c->a, c->p, c->q);
    case EF_C_HEAL_OTHERS: {
        EF_TRY(ef_base(g, p, 4));
        EfTile *tile = ef_tile(g, c->p);
        if (!tile) return ef_err(EF_ERR_HEAL_OTHERS_TILE, 0);
        if (tile->unit_n != EF_NONE) {
            ef_units(g)[tile->unit_n].moved_n = 1; /* narrow: a flag or constant */
            ef_units(g)[tile->unit_n].attacked_n = 1; /* narrow: a flag or constant */
        }
        return ef_push(g, ef_action_at(EF_A_HEAL_OTHERS, p, c->p));
    }
    case EF_C_PROMOTE:
        EF_TRY(ef_base(g, p, 13));
        return ef_push(g, ef_action_at(EF_A_PROMOTE, p, c->p));
    case EF_C_RESIGN: {
        EF_TRY(ef_base(g, p, 35));
        int32_t resigned = c->a;
        int i = ef_player_index(g, resigned);
        if (i == EF_NONE) return ef_ok();
        EfPlayer *x = &g->players[i];
        x->auto_play = 1;
        x->resigned_at_command = history - 1;
        x->resigned_turn = g->current_turn;
        if (g->game_type == 1 || g->game_type == 4) x->handicap = 1;
        if (g->triggers_len > EF_RESIGN_TRIGGERS) return ef_err(EF_ERR_TRIGGERS_FULL, 0);
        EfAction a = ef_action(EF_A_RESIGN, p);
        a.i[0] = resigned;
        a.i[1] = c->b;
        a.i[2] = c->c;
        a.len = g->triggers_len;
        for (int k = 0; k < g->triggers_len; k++) a.list.triggers[k] = g->triggers[k];
        g->triggers_len = 0;
        EF_TRY(ef_push(g, a));
        int32_t selected;
        int completed;
        EF_TRY(ef_winner(g, &selected, &completed));
        if (completed) {
            EfAction over = ef_action(EF_A_GAME_OVER, p);
            over.i[0] = selected;
            EF_TRY(ef_push(g, over));
        }
        return ef_ok();
    }
    case EF_C_CITY_REWARD: return ef_reward_command(g, p, c->a, c->p);
    case EF_C_EXAMINE_RUINS: return ef_examine_command(g, t, p, c->p);
    case EF_C_CAPTURE: return ef_capture_command(g, p, (uint32_t)c->a, c->p);
    case EF_C_END_TURN: return ef_end_command(g, t, p);
    case EF_C_END_MATCH: return ef_end_match(g, p);
    case EF_C_START_MATCH:
        EF_TRY(ef_base(g, p, 20));
        return ef_push(g, ef_action(EF_A_START_MATCH, p));
    case EF_C_MOVE: return ef_move_command(g, t, p, (uint32_t)c->a, c->p, c->q);
    case EF_C_TRAIN: {
        EF_TRY(ef_base(g, p, 5));
        const EfUnitData *d = ef_unit_data(t, c->a);
        if (!d) return ef_ok();
        EfAction a = ef_action_at(EF_A_TRAIN, p, c->p);
        a.i[0] = c->a;
        a.i[1] = d->cost;
        a.p[1] = ef_xy(-1, -1);
        return ef_push(g, a);
    }
    case EF_C_ESTABLISH_EMBASSY: {
        EfAction a = ef_action(EF_A_ESTABLISH_EMBASSY, p);
        a.i[0] = c->a;
        EF_TRY(ef_push(g, a));
        int o = ef_player_index(g, c->a);
        if (o != EF_NONE) EF_TRY(ef_push(g, ef_meet(c->a, p, g->players[o].start_tile)));
        return ef_ok();
    }
    case EF_C_UPGRADE: {
        EF_TRY(ef_base(g, p, 16));
        const EfUnitData *d = c->a != 0 ? ef_unit_data(t, c->a) : 0;
        if (!d) return ef_ok();
        EfAction a = ef_action_at(EF_A_UPGRADE, p, c->p);
        a.i[0] = c->a;
        a.i[1] = d->cost;
        return ef_push(g, a);
    }
    case EF_C_DISEMBARK: return ef_push(g, ef_action_at(EF_A_DISEMBARK, p, c->p));
    case EF_C_PEACE_TREATY: {
        EfAction a = ef_action(EF_A_PEACE_TREATY, p);
        a.i[0] = c->a;
        return ef_push(g, a);
    }
    case EF_C_RECOVER:
        EF_TRY(ef_base(g, p, 3));
        EF_TRY(ef_push(g, ef_action_at(EF_A_RECOVER, p, c->p)));
        return ef_push(g, ef_action_at(EF_A_HEAL, p, c->p));
    case EF_C_DISBAND: {
        EF_TRY(ef_base(g, p, 10));
        EfAction a = ef_action_at(EF_A_DISBAND_UNIT, p, c->p);
        a.p[1] = ef_xy(0, 0);
        a.i[0] = 1;
        return ef_push(g, a);
    }
    case EF_C_FREEZE_AREA: return ef_freeze_area_command(g, p, c->p);
    case EF_C_BREAK_ICE: return ef_break_ice_command(g, p, c->p);
    case EF_C_FLOOD: return ef_flood_command(g, p, c->p);
    case EF_C_SWARM:
    case EF_C_BOOST: return ef_swarm_command(g, p, c->p);
    case EF_C_EXPLODE: return ef_explode_command(g, p, c->p);
    case EF_C_DECOMPOSE: return ef_decompose_command(g, t, p, c->p);
    case EF_C_CLEAR_TILE_EFFECT: {
        EfAction a = ef_action_at(EF_A_CLEAR_TILE_EFFECT, p, c->p);
        a.i[0] = c->a;
        a.i[1] = 1;
        return ef_push(g, a);
    }
    default: return ef_not_ported(EF_NOT_PORTED_COMMAND + c->kind);
    }
}

/* reset.rs learn: a starting technology, its score, its tasks and the genius check */
EF_FN EfResult ef_learn(EfGame *g, const EfTables *t, int i, int32_t tech) {
    EfPlayer *x = &g->players[i];
    if (ef_contains(x->available_tech, x->available_tech_len, tech)) return ef_ok();
    int32_t id = x->id, score = 0;
    const EfTechData *d = ef_tech_data(t, tech);
    if (d) {
        float value = (float)ef_wmul(d->cost, 100);
        score = (value >= 2147483648.0f || value < -2147483648.0f) ? INT32_MIN : (int32_t)value;
        if (d->task_unlocks_error) return ef_err(EF_ERR_TABLE, d->task_unlocks_error - 1);
        if (ef_has(d->abilities, EF_AB_CAPITALVISION)) return ef_err(EF_ERR_STARTING_VISION, 0);
    }
    if (x->available_tech_len >= EF_MAX_TECH) return ef_short_list_full(EF_MAX_TECH);
    x->available_tech[x->available_tech_len++] = tech;
    EF_TRY(ef_add_score(g, id, (int64_t)score));
    if (d) {
        for (int k = 0; k < d->task_unlocks_len; k++) {
            if (ef_task_slot(&g->players[i], d->task_unlocks[k]) >= 0) continue;
            EfAction e = ef_action(EF_A_ENABLE_TASK, id);
            e.i[0] = d->task_unlocks[k];
            EF_TRY(ef_push(g, e));
        }
    }
    return ef_check_task(g, t, i, EF_TASK_GENIUS);
}

/* reset.rs start_match: every player learns its tribe's starting
 * technologies, a player already seeing a map corner gets the lighthouse hook, and the match
 * is Started */
EF_BIG EfResult ef_start_match(EfGame *g, const EfTables *t) {
    for (int i = 0; i < g->player_count; i++) {
        int32_t id = g->players[i].id, tribe = g->players[i].tribe;
        int hook = 1;
        if (tribe != 0 && (ef_tribe_flags(t, tribe) & EF_TRIBE_KNOWN)) {
            const EfTribeData *d = &t->tribes[tribe];
            if (d->starting_error) return ef_err(EF_ERR_TABLE, d->starting_error - 1);
            hook = d->starting_len > 0;
            for (int k = 0; k < d->starting_len; k++) EF_TRY(ef_learn(g, t, i, d->starting[k]));
        }
        if (hook && id != EF_NATURE) {
            int32_t w = g->width - 1, h = g->tile_count / g->width - 1;
            const EfXy corners[4] = { ef_xy(w, h), ef_xy(0, 0), ef_xy(0, h), ef_xy(w, 0) };
            for (int k = 0; k < 4; k++) {
                int32_t corner = ef_tile_index(g, corners[k]);
                if (corner == EF_NONE) return ef_err(EF_ERR_LIGHTHOUSE_CORNER, 0);
                if (ef_explored(g, corner, id)) {
                    EF_TRY(ef_reveal_lighthouse(g, t, id, corners[k]));
                    break;
                }
            }
        }
    }
    g->current_state = EF_STARTED;
    return ef_ok();
}

/* step.rs heal */
EF_BIG EfResult ef_heal(EfGame *g, const EfTables *t, EfXy c, int64_t fixed) {
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile || tile->unit_n == EF_NONE) return ef_ok();
    int32_t node = tile->unit_n, owner = tile->owner_n;
    EfUnit unit = ef_cunits(g)[node];
    /* cymanti.rs cure: the poison (and Charmed) go instead of hit points, from the passenger's
     * list too */
    if (ef_unit_effect(g, &unit, EF_POISONED)) {
        int32_t nodes[2] = { node, unit.passenger_n };
        for (int k = 0; k < 2; k++) {
            if (nodes[k] == EF_NONE) continue;
            ef_remove_effect(g, nodes[k], EF_POISONED);
            ef_remove_effect(g, nodes[k], EF_CHARMED);
        }
        return ef_ok();
    }
    int32_t max_i;
    EF_TRY(ef_max_health(g, t, &unit, &max_i));
    uint16_t max = (uint16_t)(uint32_t)max_i, health = (uint16_t)(uint32_t)unit.health_n;
    uint16_t amount;
    if (fixed != 0) {
        amount = (uint16_t)(uint64_t)fixed;
    } else {
        amount = 20;
        int i = ef_player_index(g, unit.owner_n);
        if (i != EF_NONE) {
            int peace = 0;
            if (owner != unit.owner_n && owner != 0) {
                EfRelation *r;
                EF_TRY(ef_relation_entry(g, i, owner, &r));
                peace = r->state == EF_PEACE;
            }
            if (owner == unit.owner_n || peace) amount = 40;
        }
        uint16_t room = (uint16_t)(max - health);
        if (room < amount) amount = room;
    }
    uint16_t healed = (uint16_t)(health + amount);
    ef_units(g)[node].health_n = healed < max ? healed : max; /* narrow: u16 values, as the engine's */
    return ef_ok();
}

/* step.rs disband: the tile's unit goes (its slot is freed between
 * decisions), half the cost is refunded a star at a time when paid, and its score is taken back */
EF_BIG EfResult ef_disband(EfGame *g, const EfTables *t, int32_t player, EfXy c, int paid) {
    int i = -1;
    for (int k = 0; k < g->tile_count; k++)
        if (ef_xy_eq(ef_ctiles(g)[k].coordinates, c)) { i = k; break; }
    if (i < 0) return ef_err(EF_ERR_DISBAND_TILE, 0);
    int32_t node = ef_ctiles(g)[i].unit_n;
    if (node == EF_NONE) return ef_ok();
    EfUnit unit = ef_cunits(g)[node];
    /* the follower goes too (paid), and the leader lets go */
    int32_t f = unit.follower != 0 ? ef_find_unit(g, unit.follower) : EF_NONE;
    if (f != EF_NONE) {
        EfAction a = ef_action_at(EF_A_DISBAND_UNIT, player, ef_cunits(g)[f].coordinates);
        a.p[1] = ef_xy(0, 0);
        a.i[0] = 1;
        EF_TRY(ef_push(g, a));
    }
    int32_t l = unit.leader != 0 ? ef_find_unit(g, unit.leader) : EF_NONE;
    if (l != EF_NONE) ef_units(g)[l].follower = 0;
    const EfUnitData *d = ef_unit_data(t, unit.kind_n);
    if (d) {
        int32_t passenger_cost = 0;
        if (unit.passenger_n != EF_NONE) {
            const EfUnitData *pd = ef_unit_data(t, ef_cunits(g)[unit.passenger_n].kind_n);
            if (pd) passenger_cost = pd->cost;
        }
        int32_t cost = ef_wadd(d->cost, passenger_cost);
        if (paid && cost > 1) {
            for (int32_t k = 0; k < cost / 2; k++) {
                EfAction a = ef_action_at(EF_A_INCREASE_CURRENCY, player, c);
                a.i[0] = 1;
                a.i[1] = 40;
                EF_TRY(ef_push(g, a));
            }
        }
        EfAction a = ef_action(EF_A_DECREASE_SCORE, unit.owner_n);
        a.i[0] = ef_wmul(cost, 5);
        EF_TRY(ef_push(g, a));
    }
    ef_tiles(g)[i].unit_n = EF_NONE; /* narrow: EF_NONE */
    if (d && ef_has(d->abilities, EF_AB_TENTACLES)) EF_TRY(ef_update_tentacles(g, t));
    return ef_ok();
}

/* step.rs execute_action: run one queued action, pushing its children onto the stack */
EF_BIG EfResult ef_execute_action(EfGame *g, const EfTables *t, const EfAction *a, int64_t history) {
    int32_t p = a->player;
    EF_COVER(ef_coverage_actions, a->kind);
    switch (a->kind) {
    case EF_A_END_COMMAND:
        for (int k = 0; k < g->player_count; k++) g->players[k].opinions_dirty = 1;
        return ef_ok();
    case EF_A_RUN_DEFERRED: {
        if (!g->needs_routes) return ef_ok();
        EfAction u = ef_action(EF_A_UPDATE_CITY_CONNECTIONS, p);
        u.len = 1;
        u.list.players[0] = p;
        EF_TRY(ef_push(g, u));
        EF_TRY(ef_push(g, ef_action(EF_A_UPDATE_ROUTES, p)));
        g->needs_routes = 0;
        return ef_ok();
    }
    case EF_A_INCREASE_SCORE: return ef_add_score(g, p, a->i[0]);
    case EF_A_INCREASE_CURRENCY: {
        int i = ef_player_index(g, p);
        if (i == EF_NONE) return ef_ok();
        int32_t c = ef_wadd(g->players[i].currency, a->i[0]);
        g->players[i].currency = c > 0 ? c : 0;
        return ef_check_task(g, t, i, EF_TASK_WEALTH);
    }
    case EF_A_DECREASE_SCORE: {
        int i = ef_player_index(g, p);
        if (i != EF_NONE) g->players[i].score = (int64_t)((uint32_t)(uint64_t)g->players[i].score - (uint32_t)a->i[0]);
        return ef_ok();
    }
    case EF_A_ENABLE_TASK: {
        int32_t kind = a->i[0];
        int i = ef_player_index(g, p);
        if (i == EF_NONE || ef_task_slot(&g->players[i], kind) >= 0) return ef_ok();
        EfTask task;
        EF_TRY(ef_new_task(kind, &task));
        if (g->players[i].tasks_len >= EF_MAX_TASKS) return ef_short_list_full(EF_MAX_TASKS);
        g->players[i].tasks[g->players[i].tasks_len++] = task;
        return ef_check_task(g, t, i, kind);
    }
    case EF_A_TASK_COMPLETED: return ef_ok();
    case EF_A_DESTROY_EMBASSY: {
        int o = ef_player_index(g, a->i[0]);
        if (o == EF_NONE || p == 0) return ef_ok();
        EfRelation *r;
        EF_TRY(ef_relation_entry(g, o, p, &r));
        if (r->embassy_level > 0) r->embassy_level = 0;
        return ef_ok();
    }
    case EF_A_END_TURN: return ef_end_turn(g, p);
    case EF_A_START_TURN: return ef_start_turn(g, t, p);
    case EF_A_PASS_PLAYER:
    case EF_A_END_MATCH:
        return ef_ok();
    case EF_A_RECEIVE_MESSAGE: return ef_receive_message(g, p, a->i[0], a->w[0]);
    case EF_A_GAME_OVER: ef_game_over(g); return ef_ok();
    case EF_A_HOUDINI: return ef_ok();
    case EF_A_WIPE_PLAYER: {
        int32_t target = a->i[0];
        int k;
        EF_TRY(ef_player(g, p, &k));
        g->players[k].wipe_outs += 1;
        int v;
        EF_TRY(ef_player(g, target, &v));
        g->players[v].killed_turn = g->current_turn;
        g->players[v].killer_id = p;
        g->players[v].wiped_at_command = history - 1;
        EfAction end = ef_action(EF_A_WIPE_PLAYER_END, p);
        end.i[0] = target;
        EF_TRY(ef_push(g, end));
        for (int n = 0; n < g->tile_count; n++) {
            int32_t s = ef_ctiles(g)[n].unit_n;
            if (s != EF_NONE && ef_cunits(g)[s].owner_n == target) {
                EfAction d = ef_action_at(EF_A_DISBAND_UNIT, target, ef_ctiles(g)[n].coordinates);
                d.i[0] = 1;
                EF_TRY(ef_push(g, d));
            }
        }
        for (int q = 0; q < g->player_count; q++) {
            int32_t other = g->players[q].id;
            EfAction x = ef_action(EF_A_DESTROY_EMBASSY, target);
            x.i[0] = other;
            EF_TRY(ef_push(g, x));
            EfAction y = ef_action(EF_A_DESTROY_EMBASSY, other);
            y.i[0] = target;
            EF_TRY(ef_push(g, y));
        }
        return ef_ok();
    }
    case EF_A_WIPE_PLAYER_END: return ef_ok();
    case EF_A_MOVE_HEAL_OTHERS:
    case EF_A_HEAL_OTHERS: {
        /* cymanti.rs heal_options (the MoveEffect) and the TurnEffect, which also refuses a
         * tile off the board */
        EfXy c = a->p[0];
        if (!ef_tile_c(g, c)) return a->kind == EF_A_HEAL_OTHERS ? ef_err(EF_ERR_HEAL_OTHERS_TILE, 0) : ef_ok();
        for (int32_t xx = c.x - 1; xx <= c.x + 1; xx++) {
            for (int32_t yy = c.y - 1; yy <= c.y + 1; yy++) {
                EfXy target = ef_xy(xx, yy);
                if (ef_xy_eq(target, c)) continue;
                const EfTile *x = ef_tile_c(g, target);
                if (!x || x->unit_n == EF_NONE) continue;
                EfUnit u = ef_cunits(g)[x->unit_n];
                if (u.owner_n != p) continue;
                int32_t max;
                EF_TRY(ef_max_health(g, t, &u, &max));
                if (max > u.health_n || ef_unit_effect(g, &u, EF_POISONED)) {
                    EfAction h = ef_action_at(EF_A_HEAL, p, target);
                    h.i[0] = 40;
                    EF_TRY(ef_push(g, h));
                }
            }
        }
        return ef_ok();
    }
    case EF_A_RESIGN: {
        for (int k = 0; k < a->len; k++) {
            if (g->triggers_len >= EF_MAX_TRIGGERS) return ef_err(EF_ERR_TRIGGERS_FULL, 0);
            g->triggers[g->triggers_len++] = a->list.triggers[k];
        }
        return ef_ok();
    }
    case EF_A_RESELECT: return ef_ok();
    case EF_A_REVEAL: {
        const EfTile *x = ef_tile_c(g, a->p[0]);
        if (!x || x->unit_n == EF_NONE) return ef_ok();
        int32_t n = x->unit_n;
        EfEffectList *e = &ef_lists(g)[ef_units(g)[n].effects_n];
        int k = ef_effect_index(e, EF_INVISIBLE_EFFECT);
        if (k >= 0) ef_effect_remove_at(e, k);
        const EfTile *from_tile = ef_tile_c(g, a->p[1]);
        if (from_tile && from_tile->unit_n != EF_NONE) {
            int32_t from = from_tile->unit_n;
            int32_t d = ef_facing_direction(a->p[1], a->p[0]);
            ef_units(g)[from].direction_n = (int8_t)d; /* narrow: ef_facing_direction gives 0 to 8 */
            ef_units(g)[from].flipped_n = d < 4; /* narrow: a flag or constant */
            int32_t target_owner = ef_cunits(g)[n].owner_n;
            int32_t other = target_owner == p ? ef_cunits(g)[from].owner_n : target_owner;
            EF_TRY(ef_push(g, ef_meet(p, other, a->p[0])));
        }
        return ef_ok();
    }
    case EF_A_FREEZE_AREA:
    case EF_A_FREEZE_BORDER: return ef_freeze_area(g, p, a->p[0], a->i[0], a->i[1], a->i[2]);
    case EF_A_FREEZE_TILE: return ef_freeze_tile(g, t, p, a->p[0]);
    case EF_A_FREEZE_UNIT: return ef_freeze_unit(g, t, p, a->p[0], a->p[1], a->i[0]);
    case EF_A_BREAK_ICE_AREA: return ef_break_ice_area(g, t, p, a->p[0], a->i[0]);
    case EF_A_BREAK_ICE: return ef_break_ice(g, t, p, a->p[0]);
    case EF_A_CHANGE_CLIMATE: return ef_change_climate(g, p, a->p[0], a->i[0]);
    case EF_A_FLOOD: return ef_flood(g, t, p, a->p[0]);
    case EF_A_POISON: return ef_poison(g, p, a->p[1]);
    case EF_A_EXPLODE: return ef_explode(g, t, p, a->p[0]);
    case EF_A_BOOST_OTHERS: return ef_boost_others(g, p, a->p[0]);
    case EF_A_BOOST: return ef_boost(g, a->p[0]);
    case EF_A_DECOMPOSE: return ef_decompose(g, a->p[0]);
    case EF_A_BUBBLE: return ef_bubble(g, (uint32_t)a->i[0], a->i[1]);
    case EF_A_CLEAR_TILE_EFFECT: return ef_clear_tile_effect(g, p, a->p[0], a->i[0], a->i[1]);
    case EF_A_HIDE: {
        const EfTile *x = ef_tile_c(g, a->p[0]);
        if (!x || x->unit_n == EF_NONE) return ef_ok();
        int32_t n = x->unit_n;
        ef_units(g)[n].attacked_n = 1; /* narrow: a flag or constant */
        EfEffectList *e = &ef_lists(g)[ef_units(g)[n].effects_n];
        if (!ef_has_effect(e, EF_INVISIBLE_EFFECT)) {
            if (e->len_n >= 9) return ef_short_list_full(9);
            e->items_n[e->len_n++] = EF_INVISIBLE_EFFECT; /* narrow: the constant 3, below 9 entries */
        }
        return ef_ok();
    }
    case EF_A_BREAK_PEACE: {
        int32_t q = a->i[0];
        int other, actor;
        EF_TRY(ef_player(g, q, &other));
        EF_TRY(ef_player(g, p, &actor));
        int32_t turn = g->current_turn;
        int who[2] = { actor, other };
        int32_t toward[2] = { q, p };
        for (int k = 0; k < 2; k++) {
            EfRelation *r;
            EF_TRY(ef_relation_entry(g, who[k], toward[k], &r));
            r->state = EF_COOL_DOWN;
            r->last_peace_broken_turn = turn;
        }
        for (int i = 0; i < g->tile_count; i++) {
            int32_t n = ef_ctiles(g)[i].unit_n;
            if (n == EF_NONE || ef_cunits(g)[n].owner_n != p) continue;
            EfXy c = ef_cunits(g)[n].coordinates;
            const EfTile *at = ef_tile_c(g, c);
            if (!at) return ef_err(EF_ERR_UNIT_OFF_BOARD, 0);
            if (at->owner_n == q) {
                EfAction d = ef_action_at(EF_A_DISBAND_UNIT, p, c);
                d.i[0] = 0;
                EF_TRY(ef_push(g, d));
            } else {
                ef_units(g)[n].moved_n = 1; /* narrow: a flag or constant */
                ef_units(g)[n].attacked_n = 1; /* narrow: a flag or constant */
            }
        }
        return ef_ok();
    }
    case EF_A_ESTABLISH_EMBASSY: {
        int32_t q = a->i[0];
        int other, actor;
        EF_TRY(ef_player(g, q, &other));
        EF_TRY(ef_player(g, p, &actor));
        int32_t cost;
        EF_TRY(ef_embassy_cost(g, p, &cost));
        int32_t c = ef_wsub(g->players[actor].currency, cost);
        g->players[actor].currency = c > 0 ? c : 0;
        EfRelation *r;
        EF_TRY(ef_relation_entry(g, other, p, &r));
        r->embassy_level = 1;
        r->embassy_build_turn = g->current_turn;
        EfXy center = g->players[other].start_tile;
        for (int32_t x = center.x - 1; x <= center.x + 1; x++) {
            for (int32_t y = center.y - 1; y <= center.y + 1; y++) {
                EfXy at = ef_xy(x, y);
                const EfTile *tile = ef_tile_c(g, at);
                if (!ef_xy_eq(at, center) && tile && !ef_tile_explored(tile, p))
                    EF_TRY(ef_push(g, ef_action_at(EF_A_EXPLORE, p, at)));
            }
        }
        return ef_ok();
    }
    case EF_A_PEACE_RESPONSE: {
        if (!a->i[1]) return ef_ok();
        int32_t q = a->i[0];
        int actor, other;
        EF_TRY(ef_player(g, p, &actor));
        EF_TRY(ef_player(g, q, &other));
        EfRelation *r;
        EF_TRY(ef_relation_entry(g, actor, q, &r));
        r->state = EF_PEACE;
        EF_TRY(ef_relation_entry(g, other, p, &r));
        r->state = EF_PEACE;
        return ef_ok();
    }
    case EF_A_RESEARCH: {
        int32_t tech = (int32_t)a->w[0];
        int i = ef_player_index(g, p);
        if (i == EF_NONE) return ef_ok();
        if (ef_contains(g->players[i].available_tech, g->players[i].available_tech_len, tech)) return ef_ok();
        const EfTechData *d = ef_tech_data(t, tech);
        if (!d) return ef_err(EF_ERR_UNKNOWN_TECH, 0);
        int vision = ef_has(d->abilities, EF_AB_CAPITALVISION);
        EfXy start = g->players[i].start_tile;
        if (g->players[i].available_tech_len >= EF_MAX_TECH) return ef_short_list_full(EF_MAX_TECH);
        g->players[i].available_tech[g->players[i].available_tech_len++] = tech;
        int32_t c = ef_wsub(g->players[i].currency, a->i[0]);
        g->players[i].currency = c > 0 ? c : 0;
        EfAction score = ef_action_at(EF_A_INCREASE_SCORE, p, start);
        score.i[0] = d->cost * 100;
        EF_TRY(ef_push(g, score));
        if (d->task_unlocks_error) return ef_err(EF_ERR_TABLE, d->task_unlocks_error - 1);
        for (int k = 0; k < d->task_unlocks_len; k++) {
            int32_t kind = d->task_unlocks[k];
            if (ef_task_slot(&g->players[i], kind) < 0) {
                EfAction e = ef_action(EF_A_ENABLE_TASK, p);
                e.i[0] = kind;
                EF_TRY(ef_push(g, e));
            }
        }
        if (vision) {
            EF_TRY(ef_refresh_tech_cache(g, t, p));
            if (ef_player_ability(g, t, p, EF_AB_CAPITALVISION)) {
                for (int o = 0; o < g->player_count; o++) {
                    int32_t other = g->players[o].id;
                    int known = other == EF_NATURE || other == p
                                || ef_contains(g->players[i].known_players, g->players[i].known_players_len, other);
                    if (other == EF_NATURE || !known) continue;
                    EfXy s = g->players[o].start_tile;
                    const EfTile *st = ef_tile_c(g, s);
                    if (!st) return ef_err(EF_ERR_CAPITAL_OFF_BOARD, 0);
                    if (!ef_tile_explored(st, p)) EF_TRY(ef_push(g, ef_action_at(EF_A_EXPLORE, p, s)));
                }
            }
        }
        return ef_check_task(g, t, i, EF_TASK_GENIUS);
    }
    case EF_A_PEACE_TREATY: {
        int i;
        EF_TRY(ef_player(g, a->i[0], &i));
        EfPlayer *x = &g->players[i];
        if (x->messages_len >= EF_MAX_PLAYERS) return ef_err(EF_ERR_SHORT_LIST_REFUSED, EF_MAX_PLAYERS);
        x->message_kinds[x->messages_len] = EF_PEACE_REQUEST_MESSAGE;
        x->message_senders[x->messages_len] = p;
        x->messages_len += 1;
        return ef_ok();
    }
    case EF_A_RUINS: return ef_ruins(g, t, p, a->w[0], a->p[0]);
    case EF_A_ATTACK: return ef_perform_attack(g, t, p, a->p[0], a->p[1], a->i[0]);
    case EF_A_KILL: return ef_kill(g, t, a->p[0]);
    case EF_A_CAPTURE_CITY: return ef_capture_city(g, t, p, a->p[0]);
    case EF_A_START_MATCH: return ef_start_match(g, t);
    case EF_A_INFILTRATE: return ef_infiltrate(g, t, p, a->p[0], a->p[1]);
    case EF_A_INFILTRATION_REWARD: return ef_infiltration_reward(g, t, p, a->w[0], a->p[0], a->i[0]);
    case EF_A_CONVERT: return ef_convert(g, t, p, a->p[0], a->p[1]);
    case EF_A_CITY_REWARD: return ef_city_reward(g, t, p, a->i[0], a->p[0]);
    case EF_A_SCOUT: return ef_scout(g, t, a);
    case EF_A_EXPAND_CITY: return ef_expand_city(g, t, p, a->p[0]);
    case EF_A_MODIFY_PRODUCTION: return ef_modify(g, 1, a->p[0], a->i[0]);
    case EF_A_MODIFY_SCORE: return ef_modify(g, 0, a->p[0], a->i[0]);
    case EF_A_BUILD: return ef_build(g, t, p, a->i[0], a->p[0], a->i[1]);
    case EF_A_BUILD_ROAD:
    case EF_A_DESTROY_RESOURCE:
    case EF_A_CREATE_RESOURCE:
    case EF_A_LEVEL_UP_IMPROVEMENT:
    case EF_A_LEVEL_DOWN_IMPROVEMENT:
        return ef_improvement_action(g, t, a);
    case EF_A_DESTROY_IMPROVEMENT: return ef_destroy(g, t, p, a->p[0]);
    case EF_A_INCREASE_POPULATION:
    case EF_A_DECREASE_POPULATION:
    case EF_A_LEVEL_UP:
        return ef_city_action(g, t, a->kind, p, a->kind == EF_A_INCREASE_POPULATION ? a->p[1] : a->p[0]);
    case EF_A_TRAIN: return ef_train(g, t, p, a->i[0], a->p[0], a->i[1], a->p[1]);
    case EF_A_EMBARK: return ef_embark(g, t, p, a->p[0]);
    case EF_A_DISEMBARK: return ef_disembark(g, t, p, a->p[0]);
    case EF_A_GROW_UP:
    case EF_A_UPGRADE: return ef_upgrade(g, t, p, a->p[0], a->i[0], a->i[1]);
    case EF_A_EAT: return ef_eat(g, t, p, (uint32_t)a->i[0], a->p[0]);
    case EF_A_PROMOTE: return ef_promote(g, t, a->p[0]);
    case EF_A_UPDATE_ROUTES: {
        EfRouteWork *w = EF_SCRATCH_TAKE(EfRouteWork, t, route_scratch);
        EfResult r = ef_update_routes(g, t, w);
        EF_SCRATCH_GIVE();
        return r;
    }
    case EF_A_UPDATE_CITY_CONNECTIONS: {
        EfConnectWork *w = EF_SCRATCH_TAKE(EfConnectWork, t, connect_scratch);
        EfResult r = ef_update_city_connections(g, t, a->list.players, a->len, w);
        EF_SCRATCH_GIVE();
        return r;
    }
    case EF_A_CONNECT:
    case EF_A_CHANGE:
    case EF_A_DISCONNECT:
        return ef_city_connection(g, t, a->kind, p, a->p[0]);
    case EF_A_MOVE: return ef_move_action(g, t, p, (uint32_t)a->i[0], a->list.path, a->len, a->i[2]);
    case EF_A_EXPLORE: return ef_explore(g, t, p, a->p[0]);
    case EF_A_MEET: return ef_meet_action(g, t, p, a->i[0], a->p[0]);
    case EF_A_RECOVER: {
        const EfTile *tile = ef_tile_c(g, a->p[0]);
        if (!tile) return ef_ok();
        if (tile->unit_n == EF_NONE) return ef_err(EF_ERR_RECOVER_NO_UNIT, 0);
        ef_units(g)[tile->unit_n].moved_n = 1; /* narrow: a flag or constant */
        ef_units(g)[tile->unit_n].attacked_n = 1; /* narrow: a flag or constant */
        return ef_ok();
    }
    case EF_A_HEAL: return ef_heal(g, t, a->p[0], a->i[0]);
    case EF_A_DISBAND_UNIT: return ef_disband(g, t, p, a->p[0], a->i[0]);
    default: return ef_not_ported(EF_NOT_PORTED_ACTION + a->kind);
    }
}

/* step.rs submit: count the command and run the command manager until it is idle or a
 * pending choice stops it; g->actions is the stack (newest last) */
EF_FN EfResult ef_submit(EfGame *g, const EfTables *t, int32_t player, const EfCommand *command) {
    g->commands += 1;
    int64_t history = (int64_t)g->commands;
    int command_left = 1;
    for (;;) {
        if (g->actions_len > 0 && !ef_pending_trigger(g)) {
            /* the newest action runs; its children go on top in its place */
            EfAction a = g->actions[--g->actions_len];
            EF_TRY(ef_execute_action(g, t, &a, history));
            continue;
        }
        if (g->current_command >= g->commands) break;
        if (!command_left) return ef_err(EF_ERR_COMMANDS_NEVER_RAN, 0);
        command_left = 0;
        g->current_command += 1;
        EF_TRY(ef_execute_command(g, t, player, command, history));
    }
    return ef_ok();
}

/* Free the unit slots no tile reaches and the effect lists no reached unit uses, between
 * decisions (the Rust engine keeps such nodes; no rule reads them). */
EF_FN void ef_collect(EfGame *g) {
    uint32_t unit_reached[EF_BIT_WORDS(EF_MAX_UNITS)], list_reached[EF_BIT_WORDS(EF_MAX_UNITS + 1)];
    memset(unit_reached, 0, sizeof unit_reached);
    memset(list_reached, 0, sizeof list_reached);
    ef_bit_set(list_reached, 0);
    for (int i = 0; i < g->tile_count; i++) {
        int guard = 0;
        for (int32_t u = ef_ctiles(g)[i].unit_n; u != EF_NONE && guard < 4; u = ef_cunits(g)[u].passenger_n, guard++) {
            ef_bit_set(unit_reached, u);
            ef_bit_set(list_reached, ef_cunits(g)[u].effects_n);
        }
    }
    /* only slots in use are touched: a free slot is already zero */
    for (int u = 0; u < g->unit_cap; u++)
        if (ef_units(g)[u].used_n && !ef_bit(unit_reached, u)) { memset(&ef_units(g)[u], 0, sizeof ef_units(g)[u]); ef_units(g)[u].passenger_n = EF_NONE; } /* narrow: a flag or constant */
    for (int l = 1; l <= g->unit_cap; l++)
        if (ef_lists(g)[l].used_n && !ef_bit(list_reached, l)) memset(&ef_lists(g)[l], 0, sizeof ef_lists(g)[l]);
}

/* step.rs advance: admit and run the chosen move, then Nature's continuation */
EF_FN EfResult ef_advance(EfGame *g, const EfTables *t, const EfMove *move, int32_t *elapsed) {
    int32_t player = ef_mover(g);
    EfCommand command;
    EF_TRY(ef_admit(g, t, move, &command));
    int32_t before_mover = ef_mover(g), before_turn = g->current_turn;
    EF_TRY(ef_submit(g, t, player, &command));
    *elapsed = ef_mover(g) != before_mover || g->current_turn != before_turn;
    /* Nature's turn ends at once (EndMatch in the final turn), at most 4 times */
    for (int k = 0; k < 4; k++) {
        int64_t state = g->current_state;
        if (state == EF_ENDED || ef_mover(g) != EF_NATURE) break;
        EfCommand nature;
        memset(&nature, 0, sizeof nature);
        if (state == EF_STARTED) nature.kind = EF_C_END_TURN;
        else if (state == EF_FINAL_TURN) nature.kind = EF_C_END_MATCH;
        else return ef_err(EF_ERR_NATURE_PHASE, 0);
        EF_TRY(ef_submit(g, t, EF_NATURE, &nature));
    }
    if (ef_mover(g) == EF_NATURE && g->current_state != EF_ENDED) return ef_err(EF_ERR_NATURE_SETTLE, 0);
    g->decisions += 1;
    ef_collect(g);
    return ef_ok();
}

/* step.rs command: admit and run one command alone, with no Nature turn and no decision
  * count (the step a recorded game's commands take) */
EF_FN EfResult ef_command(EfGame *g, const EfTables *t, const EfMove *move) {
    int32_t player = ef_mover(g);
    EfCommand command;
    EF_TRY(ef_admit(g, t, move, &command));
    EF_TRY(ef_submit(g, t, player, &command));
    ef_collect(g);
    return ef_ok();
}

#endif /* EF_STEP_H */
