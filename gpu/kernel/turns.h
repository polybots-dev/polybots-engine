/* turns.h - port of engine-fast/src/turns.rs: the EndTurn command, the EndTurn and StartTurn
 * actions, the winner check, messages and EndMatch. */
#ifndef EF_TURNS_H
#define EF_TURNS_H

#include "economy.h"

#define EF_FROZEN 0
#define EF_PETRIFIED 5
#define EF_PASS_AND_PLAY 2
#define EF_ROBBED 1

/* cymanti.rs heal_options: own units around `c` (and on it when `centre`) that are hurt or
 * poisoned; their count, and whether `c` is among them */
EF_FN EfResult ef_heal_options(EfGame *g, const EfTables *t, EfXy c, int32_t p, int centre, int *count, int *on_centre) {
    *count = 0;
    *on_centre = 0;
    for (int32_t x = c.x - 1; x <= c.x + 1; x++)
        for (int32_t y = c.y - 1; y <= c.y + 1; y++) {
            EfXy d = ef_xy(x, y);
            if (!centre && ef_xy_eq(d, c)) continue;
            const EfTile *tile = ef_tile_c(g, d);
            if (!tile || tile->unit_n == EF_NONE) continue;
            EfUnit u = ef_cunits(g)[tile->unit_n];
            if (u.owner_n != p) continue;
            int32_t max;
            EF_TRY(ef_max_health(g, t, &u, &max));
            if (max > u.health_n || ef_unit_effect(g, &u, EF_POISONED)) {
                ++*count;
                *on_centre |= ef_xy_eq(d, c);
            }
        }
    return ef_ok();
}

/* cymanti.rs improvement_upkeep: a Heal improvement (the
 * mycelium) heals the unit on it and its neighbours; a decomposing one pays its cost back one
 * star at a time and is destroyed, unless a unit stands on a bridge */
EF_BIG EfResult ef_improvement_upkeep(EfGame *g, const EfTables *t, int32_t p, EfXy c) {
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_UPKEEP_TILE, 0);
    if (!tile->has_improvement_n) return ef_ok();
    const EfImprovement *m = &tile->improvement;
    const EfImprovementData *d = ef_improvement_data(t, m->kind_n);
    if (!d) return ef_err(EF_ERR_END_COMMAND_IMPROVEMENT, 0);
    if (ef_has(d->abilities, EF_AB_HEAL)) {
        int count, on_centre;
        EF_TRY(ef_heal_options(g, t, c, p, 1, &count, &on_centre));
        if (on_centre) {
            EfAction h = ef_action_at(EF_A_HEAL, p, c);
            h.i[0] = 40;
            EF_TRY(ef_push(g, h));
        }
        if (count) EF_TRY(ef_push(g, ef_action_at(EF_A_MOVE_HEAL_OTHERS, p, c)));
    }
    if (ef_contains_i8(m->effects_n, m->effects_len_n, EF_DECOMPOSING) && (tile->unit_n == EF_NONE || !ef_has(d->abilities, EF_AB_BRIDGE))) {
        int32_t cost = d->has_cost && d->cost > 0 ? d->cost : 0;
        for (int32_t k = 0; k < cost; k++) {
            EfAction a = ef_action_at(EF_A_INCREASE_CURRENCY, p, c);
            a.i[0] = 1;
            a.i[1] = 40;
            EF_TRY(ef_push(g, a));
        }
        EF_TRY(ef_push(g, ef_action_at(EF_A_DESTROY_IMPROVEMENT, p, c)));
    }
    return ef_ok();
}

/* turns.rs grow: once a Grow unit's age reaches its kind's growthRate it upgrades, free, to
 * the unit kind whose upgradesFrom names it */
EF_FN EfResult ef_grow(EfGame *g, const EfTables *t, int32_t p, EfXy c, const EfUnit *u) {
    const EfUnitData *data = ef_unit_data(t, u->kind_n);
    if (!data) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    int32_t next = EF_NONE;
    for (int32_t k = 0; k < t->unit_count; k++) {
        if (!ef_unit_data(t, k) || t->units[k].upgrades_from != u->kind_n) continue;
        /* the game takes the first in its catalog's serialized order; a Grow unit has one
         * successor, so two are refused rather than ordered */
        if (next != EF_NONE) return ef_err(EF_ERR_GROW_SUCCESSORS, 0);
        next = k;
    }
    if (next == EF_NONE) return ef_ok();
    int64_t rate = data->has_growth_rate ? data->growth_rate : 0;
    int64_t age = (int64_t)(uint32_t)((uint32_t)g->current_turn - (uint32_t)u->created_turn_n);
    if (rate > age) return ef_ok();
    EfAction a = ef_action_at(EF_A_UPGRADE, p, c);
    a.i[0] = next;
    a.i[1] = 0;
    return ef_push(g, a);
}

/* turns.rs end_command */
EF_BIG EfResult ef_end_command(EfGame *g, const EfTables *t, int32_t p) {
    EF_TRY(ef_base(g, p, 15));
    EF_TRY(ef_push(g, ef_action(EF_A_END_TURN, p)));
    if (ef_records_achievements(g)) return ef_err(EF_ERR_ACHIEVEMENTS, 0);
    for (int i = 0; i < g->tile_count; i++) {
        EfXy c = ef_ctiles(g)[i].coordinates;
        int32_t n = ef_ctiles(g)[i].unit_n;
        if (n != EF_NONE) {
            EfUnit u = ef_cunits(g)[n];
            if (u.owner_n == p) {
                int32_t max;
                EF_TRY(ef_max_health(g, t, &u, &max));
                const EfEffectList *fx = ef_effect_list(g, u.effects_n);
                int recover = u.leader == 0 && (u.health_n < max || ef_has_effect(fx, EF_POISONED)) && !u.moved_n
                              && !ef_has_effect(fx, EF_FROZEN) && !u.attacked_n;
                if (recover) EF_TRY(ef_push(g, ef_action_at(EF_A_HEAL, p, c)));
                if (ef_unit_has(t, u.kind_n, EF_AB_GROW)) EF_TRY(ef_grow(g, t, p, c, &u));
                EfUnit *m = &ef_units(g)[n];
                m->moved_n = 0; /* narrow: a flag or constant */
                m->attacked_n = 0; /* narrow: a flag or constant */
                m->previous_turn_end_coordinates = m->coordinates;
                EfEffectList *e = &ef_lists(g)[m->effects_n];
                const int64_t gone[2] = { EF_FROZEN, EF_PETRIFIED };
                for (int k = 0; k < 2; k++) {
                    int at = ef_effect_index(e, gone[k]);
                    if (at >= 0) ef_effect_remove_at(e, at);
                }
                if (ef_unit_has(t, u.kind_n, EF_AB_DOUBLEATTACK)) EF_TRY(ef_unit_effect_add(g, n, EF_DOUBLE_READY));
            }
        }
        if (ef_ctiles(g)[i].owner_n == p) EF_TRY(ef_improvement_upkeep(g, t, p, c));
    }
    return ef_update_tentacles(g, t);
}

typedef struct { int64_t primary, score; } EfRankKey;
EF_FN int ef_key_less(EfRankKey a, EfRankKey b) { return a.primary < b.primary || (a.primary == b.primary && a.score < b.score); }

EF_FN EfRankKey ef_rank_key(const EfGame *g, int k) {
    EfRankKey r;
    const EfPlayer *p = &g->players[k];
    r.primary = g->rules_mode == 2 ? p->cities : g->rules_mode == 4 ? p->capital_count_cache : 0;
    r.score = p->score;
    return r;
}

/* turns.rs winner: (rank-first player, whether the game is complete); refreshes every
 * player's capital_count_cache */
EF_BIG EfResult ef_winner(EfGame *g, int32_t *selected_id, int *completed) {
    int playable = 0;
    for (int k = 0; k < g->player_count; k++) playable += g->players[k].id != EF_NATURE;
    if (playable < 2 || playable > 4 || g->player_count > 5 || g->rules_mode == 7) return ef_err(EF_ERR_WINNER_SCOPE, 0);
    int32_t starts[EF_MAX_PLAYERS], ids[EF_MAX_PLAYERS];
    for (int k = 0; k < g->player_count; k++) {
        const EfPlayer *p = &g->players[k];
        ids[k] = p->id;
        if (p->id == EF_NATURE) { starts[k] = 0; continue; }
        const EfTile *s = ef_tile_c(g, p->start_tile);
        if (!s) return ef_err(EF_ERR_CAPITAL_OFF_BOARD, 0);
        starts[k] = s->owner_n;
    }
    for (int k = 0; k < g->player_count; k++) {
        int64_t count = 0;
        for (int q = 0; q < g->player_count; q++) count += ids[q] != EF_NATURE && starts[q] == ids[k];
        g->players[k].capital_count_cache = count;
    }
    int ranked[EF_MAX_PLAYERS], n = g->player_count;
    for (int k = 0; k < n; k++) ranked[k] = k;
    if (n == 2 || n == 3) {
        const int pairs[3][2] = { { 0, 1 }, { 0, 2 }, { 1, 2 } };
        int count = n == 2 ? 1 : 3;
        for (int k = 0; k < count; k++) {
            int i = pairs[k][0], j = pairs[k][1];
            if (ef_key_less(ef_rank_key(g, ranked[i]), ef_rank_key(g, ranked[j]))) { int x = ranked[i]; ranked[i] = ranked[j]; ranked[j] = x; }
        }
    } else {
        for (int i = 1; i < n; i++) {
            int j = i;
            while (j > 0 && ef_key_less(ef_rank_key(g, ranked[j - 1]), ef_rank_key(g, ranked[j]))) {
                int x = ranked[j]; ranked[j] = ranked[j - 1]; ranked[j - 1] = x;
                j--;
            }
        }
    }
    int active[EF_MAX_PLAYERS], eliminated[EF_MAX_PLAYERS], na = 0, ne = 0, nature = -1;
    for (int r = 0; r < n; r++) {
        int k = ranked[r];
        if (ids[k] == EF_NATURE) nature = k;
        else if (g->players[k].wiped_at_command != -1) eliminated[ne++] = k;
        else active[na++] = k;
    }
    if (ne > 3) return ef_err(EF_ERR_ELIMINATED_RANKING, 0);
    {
        const int pairs[3][2] = { { 0, 1 }, { 0, 2 }, { 1, 2 } };
        int count = ne == 2 ? 1 : ne == 3 ? 3 : 0;
        for (int k = 0; k < count; k++) {
            int a = pairs[k][0], b = pairs[k][1];
            const EfPlayer *pa = &g->players[eliminated[a]], *pb = &g->players[eliminated[b]];
            int64_t wa = pa->resigned_at_command >= 0 ? pa->resigned_at_command : pa->wiped_at_command;
            int64_t wb = pb->resigned_at_command >= 0 ? pb->resigned_at_command : pb->wiped_at_command;
            if (wa < wb) { int x = eliminated[a]; eliminated[a] = eliminated[b]; eliminated[b] = x; }
        }
    }
    for (int k = 0; k < ne; k++) active[na++] = eliminated[k];
    if (nature >= 0) active[na++] = nature;
    int selected = active[0];
    int real = 0, alive_count = 0;
    for (int k = 0; k < n; k++) {
        if (ids[k] == EF_NATURE) continue;
        int a = ef_alive(g, ids[k]);
        alive_count += a;
        real += a && !g->players[ef_player_index(g, ids[k])].auto_play;
    }
    int64_t capitals_selected = 0;
    for (int q = 0; q < n; q++) capitals_selected += ids[q] != EF_NATURE && starts[q] == ids[selected];
    int done;
    if ((real < 2 && (g->game_type == 1 || g->game_type == 4)) || real == 0) done = 1;
    else if (g->win_by_extermination && alive_count == 1) done = 1;
    else if (g->turn_limit > 0) done = (int64_t)g->current_turn >= g->turn_limit;
    else if (g->win_by_capital && capitals_selected == (int64_t)g->player_count - 1) done = 1;
    else done = g->score_limit > 0 && g->players[selected].score >= g->score_limit;
    *selected_id = ids[selected];
    *completed = done;
    return ef_ok();
}

/* turns.rs end_turn (the EndTurn turn effect) */
EF_BIG EfResult ef_end_turn(EfGame *g, int32_t p) {
    int i;
    EF_TRY(ef_player(g, p, &i));
    int n = ef_task_slot(&g->players[i], EF_TASK_PACIFIST);
    if (n >= 0 && !g->players[i].tasks[n].completed) {
        EfTask *task = &g->players[i].tasks[n];
        task->peaceful_turns = (task->has_peaceful_turns ? task->peaceful_turns : 0) + 1;
        task->has_peaceful_turns = 1;
    }
    int32_t current = ef_mover(g);
    if (p == current) {
        int alive[EF_MAX_PLAYERS], any = 0;
        for (int k = 0; k < g->player_count; k++) { alive[k] = ef_alive(g, g->players[k].id); any |= alive[k]; }
        if (!any) return ef_err(EF_ERR_NO_PLAYER_ALIVE, 0);
        int index = g->current_player_index, wrapped = 0;
        for (;;) {
            index += 1;
            if (index >= g->player_count) { index = 0; wrapped = 1; }
            if (alive[index]) break;
        }
        g->current_player_index = index;
        g->current_turn += wrapped;
    }
    int32_t selected;
    int completed;
    EF_TRY(ef_winner(g, &selected, &completed));
    if (completed) {
        EfAction a = ef_action(EF_A_GAME_OVER, p);
        a.i[0] = selected;
        EF_TRY(ef_push(g, a));
    }
    int32_t next = ef_mover(g);
    EF_TRY(ef_push(g, ef_action(EF_A_START_TURN, next)));
    if (g->game_type == EF_PASS_AND_PLAY) EF_TRY(ef_push(g, ef_action(EF_A_PASS_PLAYER, next)));
    /* the robbed effect leaves the ending player's improvements */
    for (int k = 0; k < g->tile_count; k++) {
        EfTile *x = &ef_tiles(g)[k];
        if (x->owner_n != p || !x->has_improvement_n) continue;
        int at = ef_index_of_i8(x->improvement.effects_n, x->improvement.effects_len_n, EF_ROBBED);
        if (at >= 0) ef_remove_at_i8(x->improvement.effects_n, &x->improvement.effects_len_n, at);
    }
    /* the ending player's cool-downs toward every player end */
    for (int k = 0; k < g->player_count; k++) {
        int32_t other = g->players[k].id;
        if (other == 0) continue;
        EfRelation *r;
        EF_TRY(ef_relation_entry(g, i, other, &r));
        if (r->state == EF_COOL_DOWN) r->state = EF_NEUTRAL;
    }
    return ef_ok();
}

/* turns.rs relation: a relation as stored, or the default record where there is none (the
 * income queries insert nothing) */
EF_FN EfRelation ef_relation_read(const EfGame *g, int i, int32_t other) {
    const EfPlayer *p = &g->players[i];
    int n = ef_index_of(p->relation_ids, p->relations_len, other);
    return n < 0 ? ef_relation_default() : p->relations[n];
}

/* turns.rs has_war */
EF_FN int ef_has_war(const EfGame *g, int i, int o) {
    int32_t other = g->players[o].id;
    if (other != 0 && ef_relation_read(g, i, other).state == EF_PEACE) return 0;
    int64_t turn = g->current_turn;
    int64_t delta = turn - (int64_t)ef_relation_read(g, i, other).last_attack_turn;
    if (delta < 1 || (delta == 1 && !(i < g->current_player_index))) return 1;
    int32_t own = g->players[i].id;
    delta = turn - (int64_t)ef_relation_read(g, o, own).last_attack_turn;
    return delta < 1 || (delta == 1 && !(o < g->current_player_index));
}

/* turns.rs embassy_income */
EF_FN EfResult ef_embassy_income(const EfGame *g, const EfTables *t, int i, int o, int32_t *out) {
    EfRelation r = ef_relation_read(g, i, g->players[o].id);
    int32_t level = r.embassy_level;
    *out = 0;
    if (level < 1 || ef_has_war(g, o, i)) return ef_ok();
    if (!t->has_embassy_income) return ef_err(EF_ERR_EMBASSY_INCOME, 0);
    int64_t peace = r.state == EF_PEACE ? 2 : 1;
    *out = (int32_t)(uint32_t)((uint64_t)t->embassy_income * (uint64_t)peace * (uint64_t)(int64_t)level);
    return ef_ok();
}

/* turns.rs work, with `unit` (a node, or EF_NONE) taken as the unit
 * on the tile: its own for the rules, the one the viewer sees for an output (queries.rs
 * as_shown) */
EF_BIG EfResult ef_work_with(const EfGame *g, const EfTables *t, EfXy c, int32_t level, int32_t unit, int32_t *out) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_WORK_TILE, 0);
    *out = 0;
    const EfTile *tile = &ef_ctiles(g)[i];
    if (!tile->has_improvement_n) return ef_ok();
    int32_t owner = tile->owner_n;
    int32_t kind = tile->improvement.kind_n, production = tile->improvement.production_n, xp = tile->improvement.xp_n;
    int robbed = ef_contains_i8(tile->improvement.effects_n, tile->improvement.effects_len_n, EF_ROBBED);
    if (kind == 0) return ef_ok();
    const EfImprovementData *d = ef_improvement_data(t, kind);
    if (!d) return ef_ok();
    if (kind != EF_CITY || owner == 0) {
        int32_t w = d->has_work ? d->work : 0;
        if (w > 0) { w = ef_wmul(w, level); if (w < 0) w = 0; }
        int doubled = 0;
        if (ef_has(d->abilities, EF_AB_DOUBLED)) {
            for (int32_t x = c.x - 1; x <= c.x + 1; x++) {
                int stop = 0;
                for (int32_t y = c.y - 1; y <= c.y + 1; y++) {
                    if (x == c.x && y == c.y) continue;
                    const EfTile *o = ef_tile_c(g, ef_xy(x, y));
                    if (o && o->has_improvement_n && o->improvement.kind_n == EF_PORT) { doubled = o->owner_n == owner; stop = 1; break; }
                }
                if (stop) break;
            }
        }
        *out = (int32_t)((uint32_t)w << (doubled ? 1 : 0));
        return ef_ok();
    }
    if (unit != EF_NONE && ef_cunits(g)[unit].owner_n != owner) return ef_ok();
    if (robbed) return ef_ok();
    int pi = ef_player_index(g, owner);
    if (pi == EF_NONE) return ef_ok();
    int32_t w = ef_wsub(ef_wadd(level, production), 1);
    if (ef_xy_eq(g->players[pi].start_tile, c)) {
        w = ef_wadd(w, g->players[pi].handicap);
        for (int o = 0; o < g->player_count; o++) {
            int32_t income;
            EF_TRY(ef_embassy_income(g, t, pi, o, &income));
            w = ef_wadd(w, income);
        }
    }
    w = ef_wadd(w, xp < 0 ? xp : 0);
    *out = w > 0 ? w : 0;
    return ef_ok();
}

/* the unit on tile c, EF_NONE off the board or where there is none */
EF_FN int32_t ef_unit_at(const EfGame *g, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    return i == EF_NONE ? EF_NONE : ef_ctiles(g)[i].unit_n;
}

EF_FN EfResult ef_work(const EfGame *g, const EfTables *t, EfXy c, int32_t level, int32_t *out) {
    return ef_work_with(g, t, c, level, ef_unit_at(g, c), out);
}

/* turns.rs player_work: income from a tile for p, own or an embassy's capital, with `unit` as
 * the unit on the tile (ef_work_with) */
EF_FN EfResult ef_player_work_with(const EfGame *g, const EfTables *t, EfXy c, int32_t p, int32_t level, int32_t unit, int32_t *out) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_PLAYER_WORK_TILE, 0);
    int pi;
    EF_TRY(ef_player(g, p, &pi));
    int32_t owner = ef_ctiles(g)[i].owner_n, capital_of = ef_ctiles(g)[i].capital_of_n;
    if (owner == p) return ef_work_with(g, t, c, level, unit, out);
    *out = 0;
    if (owner != 0 && capital_of == owner) {
        int opponent = 0;
        if (unit != EF_NONE) {
            int32_t o = ef_cunits(g)[unit].owner_n;
            opponent = o != EF_NATURE && o != owner;
        }
        if (!opponent) {
            int oi;
            EF_TRY(ef_player(g, owner, &oi));
            return ef_embassy_income(g, t, oi, pi, out);
        }
    }
    return ef_ok();
}

EF_FN EfResult ef_player_work(const EfGame *g, const EfTables *t, EfXy c, int32_t p, int32_t level, int32_t *out) {
    return ef_player_work_with(g, t, c, p, level, ef_unit_at(g, c), out);
}

EF_FN int ef_knows(const EfGame *g, int i, int32_t other) {
    return other == EF_NATURE || g->players[i].id == other
           || ef_contains(g->players[i].known_players, g->players[i].known_players_len, other);
}

EF_FN EfAction ef_meet(int32_t p, int32_t other, EfXy c) {
    EfAction a = ef_action_at(EF_A_MEET, p, c);
    a.i[0] = other;
    a.i[1] = 0;
    return a;
}

/* turns.rs tribe_connection: Meet any unknown player whose unit or city the player can see */
EF_BIG EfResult ef_tribe_connection(EfGame *g, int i) {
    int32_t p = g->players[i].id;
    int others = 0;
    for (int k = 0; k < g->player_count; k++) others += g->players[k].id != EF_NATURE;
    if (p == EF_NATURE || g->players[i].known_players_len >= others) return ef_ok();
    for (int n = 0; n < g->tile_count; n++) {
        const EfTile *tile = &ef_ctiles(g)[n];
        if (!ef_tile_explored(tile, p)) continue;
        EfXy c = tile->coordinates;
        int32_t tile_owner = tile->owner_n, tile_unit = tile->unit_n;
        int city = tile->has_improvement_n && tile->improvement.kind_n == EF_CITY;
        int has_unit = 0;
        int32_t unit = 0;
        if (tile_unit != EF_NONE) {
            int32_t owner = ef_cunits(g)[tile_unit].owner_n;
            int hidden = ef_has_effect(ef_effect_list(g, ef_cunits(g)[tile_unit].effects_n), EF_INVISIBLE_EFFECT) && owner != p;
            if (hidden) {
                EfRelation r;
                EF_TRY(ef_relation_of(g, i, owner, &r));
                hidden = r.state != EF_PEACE;
            }
            if (!hidden) { has_unit = 1; unit = owner; }
        }
        int unit_unknown = has_unit && unit != p && !ef_knows(g, i, unit);
        int city_unknown = city && tile_owner != 0 && tile_owner != p && !ef_knows(g, i, tile_owner);
        if (unit_unknown) EF_TRY(ef_push(g, ef_meet(p, unit, c)));
        if (city_unknown && (!unit_unknown || !(has_unit && unit == tile_owner))) EF_TRY(ef_push(g, ef_meet(p, tile_owner, c)));
    }
    return ef_ok();
}

/* turns.rs diplomacy: deliver pending messages, newest first, and end
 * expired cool-downs */
EF_BIG EfResult ef_turn_diplomacy(EfGame *g, int i) {
    int32_t p = g->players[i].id;
    int count = g->players[i].messages_len;
    for (int n = count - 1; n >= 0; n--) {
        int32_t sender = g->players[i].message_senders[n];
        int64_t kind = g->players[i].message_kinds[n];
        if (ef_player_index(g, sender) == EF_NONE) return ef_err(EF_ERR_MESSAGE_SENDER, 0);
        if (ef_alive(g, sender)) {
            EfAction a = ef_action(EF_A_RECEIVE_MESSAGE, p);
            a.i[0] = sender;
            a.w[0] = kind;
            EF_TRY(ef_push(g, a));
            if (!ef_knows(g, i, sender)) EF_TRY(ef_push(g, ef_meet(p, sender, g->players[i].start_tile)));
        } else {
            /* the first message equal to this one goes */
            EfPlayer *x = &g->players[i];
            for (int k = 0; k < x->messages_len; k++) {
                if (x->message_senders[k] == sender && x->message_kinds[k] == kind) {
                    for (int j = k; j + 1 < x->messages_len; j++) {
                        x->message_senders[j] = x->message_senders[j + 1];
                        x->message_kinds[j] = x->message_kinds[j + 1];
                    }
                    x->messages_len -= 1;
                    break;
                }
            }
        }
    }
    int64_t turn = g->current_turn;
    for (int k = 0; k < g->player_count; k++) {
        int32_t opponent = g->players[k].id;
        if (opponent == 0) continue;
        EfRelation r;
        EF_TRY(ef_relation_of(g, i, opponent, &r));
        int64_t broken = r.last_peace_broken_turn;
        if (r.state == EF_COOL_DOWN && ((opponent < p && broken < turn) || (p < opponent && broken + 1 < turn))) {
            EfRelation *e;
            EF_TRY(ef_relation_entry(g, i, opponent, &e));
            e->state = EF_NEUTRAL;
        }
    }
    return ef_ok();
}

/* turns.rs start_turn (the StartTurn turn effect) */
EF_BIG EfResult ef_start_turn(EfGame *g, const EfTables *t, int32_t p) {
    int index;
    EF_TRY(ef_player(g, p, &index));
    for (int other = 0; other < g->player_count; other++) {
        EfRelation *r;
        EF_TRY(ef_relation_entry(g, other, p, &r));
        if (r->embassy_level >= 1) {
            if (!ef_has_war(g, index, other)) {
                EF_TRY(ef_relation_entry(g, other, p, &r));
                r->embassy_level = 1;
            }
        }
    }
    int32_t current = ef_mover(g);
    for (int n = 0; n < g->tile_count; n++) {
        const EfTile *tile = &ef_ctiles(g)[n];
        EfXy c = tile->coordinates;
        int32_t owner = tile->owner_n;
        int built = tile->has_improvement_n;
        int32_t kind = tile->improvement.kind_n, level = tile->improvement.level_n, xp = tile->improvement.xp_n;
        if (owner == p) {
            if (built && g->current_turn != 0 && kind != 0 && ef_improvement_known(t, kind)) {
                if (ef_improvement_has(t, kind, EF_AB_ATTRACT)) EF_TRY(ef_attract(g, t, p, c, kind, tile->improvement.founded_n));
                if (level < xp && p == current) EF_TRY(ef_push(g, ef_action_at(EF_A_LEVEL_UP, p, c)));
                int32_t now, amount;
                EF_TRY(ef_update_level(g, t, p, c, &now));
                EF_TRY(ef_work(g, t, c, now, &amount));
                if (amount > 0) {
                    EfAction a = ef_action_at(EF_A_INCREASE_CURRENCY, p, c);
                    a.i[0] = amount;
                    a.i[1] = 10;
                    EF_TRY(ef_push(g, a));
                }
            }
            int has;
            EfAction a;
            EF_TRY(ef_restore_climate(g, t, index, c, &has, &a));
            if (has) EF_TRY(ef_push(g, a));
        } else {
            int32_t amount;
            EF_TRY(ef_player_work(g, t, c, p, built ? level : 0, &amount));
            if (amount > 0) {
                EfAction a = ef_action_at(EF_A_INCREASE_CURRENCY, p, c);
                a.i[0] = amount;
                a.i[1] = 10;
                EF_TRY(ef_push(g, a));
            }
        }
    }
    EF_TRY(ef_check_task(g, t, index, EF_TASK_PACIFIST));
    EF_TRY(ef_check_task(g, t, index, EF_TASK_EXPLORER));
    EF_TRY(ef_tribe_connection(g, index));
    EF_TRY(ef_turn_diplomacy(g, index));
    EF_TRY(ef_refresh_tech_cache(g, t, p));
    if (ef_player_ability(g, t, p, EF_AB_CAPITALVISION)) {
        for (int o = 0; o < g->player_count; o++) {
            int32_t other = g->players[o].id;
            int known = other == p || ef_contains(g->players[index].known_players, g->players[index].known_players_len, other);
            if (other == EF_NATURE || !known) continue;
            EfXy start = g->players[o].start_tile;
            const EfTile *s = ef_tile_c(g, start);
            if (!s) return ef_err(EF_ERR_CAPITAL_TILE_PLAIN, 0);
            if (!ef_tile_explored(s, p)) EF_TRY(ef_push(g, ef_action_at(EF_A_EXPLORE, p, start)));
        }
    }
    return ef_ok();
}

/* turns.rs receive_message */
EF_FN EfResult ef_receive_message(EfGame *g, int32_t player, int32_t sender, int64_t kind) {
    int i;
    EF_TRY(ef_player(g, player, &i));
    EfPlayer *x = &g->players[i];
    int k = -1;
    for (int n = 0; n < x->messages_len; n++)
        if (x->message_senders[n] == sender && x->message_kinds[n] == kind) { k = n; break; }
    if (k < 0) return ef_ok();
    for (int j = k; j + 1 < x->messages_len; j++) {
        x->message_senders[j] = x->message_senders[j + 1];
        x->message_kinds[j] = x->message_kinds[j + 1];
    }
    x->messages_len -= 1;
    if (kind == EF_PEACE_REQUEST_MESSAGE) {
        if (g->triggers_len >= EF_MAX_TRIGGERS) return ef_err(EF_ERR_TRIGGERS_FULL, 0);
        EfTrigger *tr = &g->triggers[g->triggers_len++];
        tr->player = player;
        tr->opponent = sender;
        tr->kind = EF_PEACE_REQUEST_TRIGGER;
        tr->coordinates = ef_xy(0, 0);
    } else if (kind != EF_EMBASSY_MESSAGE) {
        return ef_err(EF_ERR_DIPLOMACY_MESSAGE, 0);
    }
    return ef_ok();
}

/* turns.rs game_over */
EF_FN void ef_game_over(EfGame *g) {
    if (g->current_state == EF_FINAL_TURN || g->current_state == EF_ENDED) return;
    g->current_state = EF_FINAL_TURN;
}

/* turns.rs end_match */
EF_FN EfResult ef_end_match(EfGame *g, int32_t p) {
    EF_TRY(ef_base(g, p, 22));
    g->current_state = EF_ENDED;
    return ef_push(g, ef_action(EF_A_END_MATCH, 0));
}

#endif /* EF_TURNS_H */
