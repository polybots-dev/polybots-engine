/* outputs.h - the batch's optional outputs beside the view and the legal list
 * (engine outputs every model may read, the public state the view leaves out;
 * engine-fast/batch/src/outputs.rs). Each is computed only when a caller asks. Some show what
 * the view hides; engine-fast/src/queries.rs lists which. */
#ifndef EF_OUTPUTS_H
#define EF_OUTPUTS_H

#include "menu.h"

/* the outputs that are one int16 plane over the tiles, in outputs.rs Plane order */
enum {
    EF_PLANE_DEFENCE, EF_PLANE_MAX_HEALTH, EF_PLANE_MOVEMENT, EF_PLANE_ATTACK_MODIFIER, EF_PLANE_CITY_WORK,
    EF_PLANE_CITY_UNITS, EF_PLANE_CITY_SIEGE, EF_PLANE_BORDER_EDGES, EF_PLANES
};
/* outputs.rs PROGRESSION: units lost, start tile, revealed resources; monuments; tasks of three;
 * lighthouses of six; embassies of two per player row */
#define EF_PROGRESSION_MONUMENTS 8
#define EF_PROGRESSION_LIGHTHOUSES 4
#define EF_PROGRESSION_SEATS 4
#define EF_PROGRESSION_LEN (3 + EF_PROGRESSION_MONUMENTS + EF_MAX_TASKS * 3 + EF_PROGRESSION_LIGHTHOUSES * 6 + EF_PROGRESSION_SEATS * 2)

/* where one slot's optional outputs from the game go, NULL for one not asked for: per plane
 * one int16 per tile, the income (one int32), the city score (one int32 per tile) */
typedef struct {
    int16_t *planes[EF_PLANES];
    int32_t *income, *city_score;
} EfQueryOut;

EF_FN int ef_query_asked(const EfQueryOut *o) {
    for (int k = 0; k < EF_PLANES; k++)
        if (o->planes[k]) return 1;
    return o->income || o->city_score;
}

/* queries.rs as_shown: the unit on tile x as `viewer` is shown the game, EF_NONE where there is
 * none or it cannot see it (menu.rs visible_unit) */
EF_FN int32_t ef_shown_unit(const EfGame *g, const EfTile *x, int32_t viewer) {
    return ef_visible_unit(g, x, viewer) ? x->unit_n : EF_NONE;
}

/* queries.rs shown_city: a city on an explored tile */
EF_FN int ef_shown_city(const EfTile *x, int32_t viewer) {
    return x->has_improvement_n && x->improvement.kind_n == EF_CITY && ef_tile_explored(x, viewer);
}

/* queries.rs unit_stats (panel): the planes asked for of every unit with an info panel for
 * `viewer` (combat.rs visible_root), -1 (queries UNKNOWN) elsewhere. Can show a foreign unit's
 * effects and its owner's techs. */
EF_BIG EfResult ef_unit_stats(const EfGame *g, const EfTables *t, int32_t viewer, int16_t *const *planes) {
    int16_t *defence = planes[EF_PLANE_DEFENCE], *max_health = planes[EF_PLANE_MAX_HEALTH];
    int16_t *movement = planes[EF_PLANE_MOVEMENT], *attack = planes[EF_PLANE_ATTACK_MODIFIER];
    if (!defence && !max_health && !movement && !attack) return ef_ok();
    for (int i = 0; i < g->tile_count; i++) {
        if (defence) defence[i] = -1;
        if (max_health) max_health[i] = -1;
        if (movement) movement[i] = -1;
        if (attack) attack[i] = -1;
        const EfUnit *u = ef_visible_root(g, &ef_ctiles(g)[i], viewer);
        if (!u) continue;
        const EfUnitData *d = ef_unit_data(t, u->kind_n);
        if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
        const EfEffectList *fx = ef_effect_list(g, u->effects_n);
        /* the panel's figures narrowed as queries.rs narrows them */
        if (max_health) {
            int32_t max;
            EF_TRY(ef_max_health(g, t, u, &max));
            max_health[i] = ef_i16(ef_tenths_up(max));
        }
        if (attack) attack[i] = ef_has_effect(fx, EF_BOOSTED) ? 50 : 0;
        if (movement) movement[i] = ef_i16(ef_unit_movement(fx, d));
        if (defence) {
            int32_t bonus;
            EF_TRY(ef_defence_bonus(g, t, u, &bonus));
            defence[i] = ef_i16(bonus);
        }
    }
    return ef_ok();
}

/* queries.rs city_work, masked: each city the viewer sees, the stars it gives the viewer as it
 * displays them (0 when robbed or none) */
EF_BIG EfResult ef_city_work(const EfGame *g, const EfTables *t, int32_t viewer, int16_t *out) {
    for (int i = 0; i < g->tile_count; i++) {
        const EfTile *x = &ef_ctiles(g)[i];
        out[i] = -1;
        if (!ef_shown_city(x, viewer)) continue;
        const EfImprovement *m = &x->improvement;
        int32_t w = 0;
        if (!ef_contains_i8(m->effects_n, m->effects_len_n, EF_ROBBED))
            EF_TRY(ef_player_work_with(g, t, x->coordinates, viewer, m->level_n, ef_shown_unit(g, x, viewer), &w));
        out[i] = ef_i16(w > 0 ? w : 0);
    }
    return ef_ok();
}

/* queries.rs income, masked: the viewer's stars per turn, every improvement tile in map order at
 * its stored level */
EF_BIG EfResult ef_shown_income(const EfGame *g, const EfTables *t, int32_t viewer, int32_t *out) {
    int32_t income = 0;
    for (int i = 0; i < g->tile_count; i++) {
        const EfTile *x = &ef_ctiles(g)[i];
        if (!x->has_improvement_n) continue;
        int32_t w;
        EF_TRY(ef_player_work_with(g, t, x->coordinates, viewer, x->improvement.level_n, ef_shown_unit(g, x, viewer), &w));
        income = ef_wadd(income, w);
    }
    *out = income;
    return ef_ok();
}

/* queries.rs city_units, masked: per own city the viewer sees, the population slots (level + 1)
 * taken by the units homed there that it sees, counted in map order up to the slots */
EF_FN void ef_city_units(const EfGame *g, int32_t viewer, int16_t *out) {
    for (int i = 0; i < g->tile_count; i++) {
        const EfTile *x = &ef_ctiles(g)[i];
        out[i] = -1;
        if (!ef_shown_city(x, viewer) || x->owner_n != viewer) continue;
        int32_t slots = ef_wadd(x->improvement.level_n, 1), used = 0;
        for (int j = 0; j < g->tile_count; j++) {
            int32_t n = ef_shown_unit(g, &ef_ctiles(g)[j], viewer);
            if (n == EF_NONE || !ef_xy_eq(ef_cunits(g)[n].home, x->coordinates)) continue;
            used++;
            if (used == slots) break;
        }
        out[i] = ef_i16(used);
    }
}

/* queries.rs city_siege, masked: 1 on an explored, owned city with another player's unit the
 * viewer sees in it, else 0; not who */
EF_FN void ef_city_siege(const EfGame *g, int32_t viewer, int16_t *out) {
    for (int i = 0; i < g->tile_count; i++) {
        const EfTile *x = &ef_ctiles(g)[i];
        out[i] = -1;
        if (!ef_tile_explored(x, viewer)) continue;
        int owned = x->owner_n != 0 && x->owner_n != EF_NATURE;
        int32_t n = ef_shown_unit(g, x, viewer);
        int city = x->has_improvement_n && x->improvement.kind_n == EF_CITY;
        out[i] = (int16_t)(owned && city && n != EF_NONE && ef_cunits(g)[n].owner_n != x->owner_n);
    }
}

/* queries.rs border_edges: the sides of each explored tile where its owner's territory ends, as
 * bits (north y + 1, south, east, west: 1, 2, 4, 8) */
EF_FN void ef_border_edges(const EfGame *g, int32_t viewer, int16_t *out) {
    const int32_t dx[4] = { 0, 0, 1, -1 }, dy[4] = { 1, -1, 0, 0 };
    for (int i = 0; i < g->tile_count; i++) {
        const EfTile *x = &ef_ctiles(g)[i];
        out[i] = -1;
        if (!ef_tile_explored(x, viewer)) continue;
        int owned = x->owner_n != 0 && x->owner_n != EF_NATURE;
        int16_t bits = 0;
        for (int k = 0; owned && k < 4; k++) {
            const EfTile *n = ef_tile_c(g, ef_xy(x->coordinates.x + dx[k], x->coordinates.y + dy[k]));
            if (!n || n->owner_n != x->owner_n) bits = (int16_t)(bits | (1 << k));
        }
        out[i] = bits;
    }
}

/* queries.rs city_score: the score of each city the viewer sees */
EF_FN void ef_city_score(const EfGame *g, const EfTables *t, int32_t viewer, int32_t *out) {
    for (int i = 0; i < g->tile_count; i++)
        out[i] = ef_shown_city(&ef_ctiles(g)[i], viewer) ? ef_improvement_score(t, g, i) : -1;
}

/* Every output of `o` from the game, for `viewer`. */
EF_BIG EfResult ef_query_outputs(const EfGame *g, const EfTables *t, int32_t viewer, const EfQueryOut *o) {
    EF_TRY(ef_unit_stats(g, t, viewer, o->planes));
    if (o->planes[EF_PLANE_CITY_WORK]) EF_TRY(ef_city_work(g, t, viewer, o->planes[EF_PLANE_CITY_WORK]));
    if (o->planes[EF_PLANE_CITY_UNITS]) ef_city_units(g, viewer, o->planes[EF_PLANE_CITY_UNITS]);
    if (o->planes[EF_PLANE_CITY_SIEGE]) ef_city_siege(g, viewer, o->planes[EF_PLANE_CITY_SIEGE]);
    if (o->planes[EF_PLANE_BORDER_EDGES]) ef_border_edges(g, viewer, o->planes[EF_PLANE_BORDER_EDGES]);
    if (o->income) EF_TRY(ef_shown_income(g, t, viewer, o->income));
    if (o->city_score) ef_city_score(g, t, viewer, o->city_score);
    return ef_ok();
}

/* the embassy level player `of` keeps in its relation record of `other` (0 without one; writes
 * no default record, unlike capture.h ef_embassy_level) */
EF_FN int32_t ef_embassy_read(const EfGame *g, int32_t of, int32_t other) {
    int i = ef_player_index(g, of);
    return i == EF_NONE ? 0 : ef_relation_read(g, i, other).embassy_level;
}

/* outputs.rs progression_row: view `v` of `g` laid out in EF_PROGRESSION_LEN int32, -1 past each
 * list's end (lists longer than their rows are cut, as the view's exports are) */
EF_FN void ef_progression_row(const EfGame *g, const EfView *v, int32_t *out) {
    for (int k = 0; k < EF_PROGRESSION_LEN; k++) out[k] = -1;
    out[0] = v->units_lost;
    out[1] = v->start_tile;
    out[2] = (int32_t)v->revealed_resources;
    int32_t *at = out + 3;
    for (int k = 0; k < v->monuments_len && k < EF_PROGRESSION_MONUMENTS; k++) at[k] = v->monuments[k];
    at += EF_PROGRESSION_MONUMENTS;
    for (int k = 0; k < v->tasks_len && k < EF_MAX_TASKS; k++) {
        at[3 * k] = v->tasks[k].kind;
        at[3 * k + 1] = v->tasks[k].completed;
        at[3 * k + 2] = v->tasks[k].counter;
    }
    at += 3 * EF_MAX_TASKS;
    for (int k = 0; k < v->lighthouses_len && k < EF_PROGRESSION_LIGHTHOUSES; k++) {
        at[6 * k] = v->lighthouses[k].tile;
        for (int e = 0; e < 5; e++) at[6 * k + 1 + e] = v->lighthouses[k].explorers[e];
    }
    at += 6 * EF_PROGRESSION_LIGHTHOUSES;
    for (int k = 0; k < v->players_len && k < EF_PROGRESSION_SEATS; k++) {
        int32_t seat = v->players[k].seat;
        at[2 * k] = ef_embassy_read(g, v->viewer, seat);
        at[2 * k + 1] = ef_embassy_read(g, seat, v->viewer);
    }
}

/* turns.rs other_seat: the next player after the one to move in turn order that is alive and
 * not Nature, else (every other player dead) the next that is not Nature; EF_NONE in a game of
 * one player */
EF_FN int32_t ef_other_seat(const EfGame *g) {
    int32_t n = g->player_count, from = g->current_player_index, mover = g->players[from].id;
    int32_t dead = EF_NONE;
    for (int32_t d = 1; d < n; d++) {
        int32_t p = g->players[(from + d) % n].id;
        if (p == mover || p == EF_NATURE) continue;
        if (ef_alive(g, p)) return p;
        if (dead == EF_NONE) dead = p;
    }
    return dead;
}

/* outputs.rs other_view, the privileged critic's input: the view of
 * ef_other_seat into `out`; with no other seat only the viewer is written, EF_NONE, and the
 * view's exports are -1 */
EF_BIG EfResult ef_other_view(const EfGame *g, const EfTables *t, EfView *out) {
    int32_t seat = ef_other_seat(g);
    if (seat == EF_NONE) {
        out->viewer = EF_NONE;
        return ef_ok();
    }
    return ef_view_write(g, t, seat, out);
}

#endif /* EF_OUTPUTS_H */
