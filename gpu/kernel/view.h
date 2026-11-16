/* view.h - port of engine-fast/src/view.rs: what one player may know, as integer arrays. */
#ifndef EF_VIEW_H
#define EF_VIEW_H

#include "query.h"

#define EF_VIEW_PLANES 30
#define EF_CITY_WALL_REWARD 1
#define EF_PARK_REWARD 2
#define EF_SUPER_UNIT_REWARD 6
/* state.rs TaskKind ids of the tasks that keep a counter */
#define EF_VIEW_TASK_PACIFIST 1
#define EF_VIEW_TASK_KILLER 5
#define EF_VIEW_TASK_CONVERT 9
#define EF_VIEW_UNKNOWN (-1)
#define EF_CITY 1
#define EF_RUIN 2
#define EF_INVISIBLE 3

/* the planes, in view.rs TILE_PLANES order */
enum {
    EF_P_EXPLORED, EF_P_TERRAIN, EF_P_CLIMATE, EF_P_RESOURCE, EF_P_IMPROVEMENT, EF_P_IMPROVEMENT_LEVEL, EF_P_OWNER,
    EF_P_CAPITAL_OF, EF_P_RULING_CITY, EF_P_ROAD, EF_P_ROUTE, EF_P_EFFECTS, EF_P_RUIN_HINT, EF_P_UNIT_KIND,
    EF_P_UNIT_OWNER, EF_P_UNIT_HEALTH, EF_P_UNIT_VETERAN, EF_P_UNIT_EFFECTS, EF_P_UNIT_PASSENGER, EF_P_UNIT_MOVED,
    EF_P_UNIT_ATTACKED, EF_P_UNIT_KILLS, EF_P_UNIT_HOME, EF_P_CITY_POPULATION, EF_P_CITY_CONNECTED,
    EF_P_CITY_PROGRESS, EF_P_CITY_WALL, EF_P_CITY_PARKS, EF_P_CITY_SUPER_UNITS, EF_P_FOUNDER_TRIBE
};

typedef struct {
    int16_t seat, tribe, relation, pad;
    int64_t score;
} EfPlayerView;

/* view.rs TaskView, LighthouseView and a colour entry */
typedef struct {
    int16_t kind, completed;
    int32_t counter;
} EfTaskView;
typedef struct {
    int16_t tile, explorers[5];
} EfLighthouseView;
typedef struct {
    int16_t seat, pad[3];
    int64_t color;
} EfColorView;

typedef struct {
    int32_t viewer, turn, width, height, stars;
    int32_t tech_len, players_len, tile_count;
    int32_t rules, mode;
    uint32_t revealed_resources;
    int32_t start_tile, units_lost;
    int32_t monuments_len, tasks_len, lighthouses_len, colors_len, pad;
    EfPlayerView players[EF_MAX_PLAYERS];
    EfColorView colors[EF_MAX_PLAYERS];
    EfTaskView tasks[EF_MAX_TASKS];
    EfLighthouseView lighthouses[4];
    int16_t monuments[8];
    int16_t tech[EF_MAX_TECH];
} EfView;

/* A view is this header followed by its tile planes, plane-major (plane p is tiles[p *
 * tile_count ..][.. tile_count]), with room for a block's tile_cap tiles: views are sized per
 * batch like the blocks. */
EF_FN uint64_t ef_view_size(int32_t tile_cap) {
    uint64_t end = (uint64_t)sizeof(EfView) + (uint64_t)EF_VIEW_PLANES * (uint64_t)tile_cap * sizeof(int16_t);
    return (end + 7u) & ~(uint64_t)7u;
}
EF_FN int16_t *ef_view_tiles(EfView *v) { return (int16_t *)(void *)((char *)v + sizeof(EfView)); }

/* Rust `x as i16` on an i32: the low 16 bits */
EF_FN int16_t ef_i16(int32_t x) { return (int16_t)(uint16_t)(uint32_t)x; }

EF_FN EfPlayerView ef_view_player(const EfPlayer *me, const EfPlayer *p) {
    EfPlayerView v;
    v.seat = ef_i16(p->id);
    v.tribe = ef_i16(p->tribe);
    v.relation = 0;
    for (int i = 0; i < me->relations_len; i++)
        if (me->relation_ids[i] == p->id) { v.relation = ef_i16(me->relations[i].state); break; }
    v.pad = 0;
    v.score = p->score;
    return v;
}

/* view.rs write into a view of ef_view_size(g->tile_cap) bytes, on a block that has passed
 * ef_game_check since it was last written (every
 * entry point checks blocks that come from outside; checking on every call would cost more
 * than the view itself) */
EF_BIG EfResult ef_view_write(const EfGame *g, const EfTables *t, int32_t viewer, EfView *out) {
    int mi = ef_player_index(g, viewer);
    if (mi == EF_NONE) return ef_err(EF_ERR_VIEW_PLAYER, 0);
    const EfPlayer *me = &g->players[mi];
    const int n = g->tile_count;
    out->viewer = ef_i16(viewer);
    out->turn = g->current_turn;
    out->width = ef_i16(g->width);
    out->height = ef_i16(n / g->width);
    out->stars = me->currency;
    out->tile_count = n;
    out->rules = ef_i16(g->version);
    if (g->rules_mode < INT16_MIN || g->rules_mode > INT16_MAX) return ef_err(EF_ERR_VIEW_MODE, 0);
    out->mode = (int32_t)g->rules_mode;
    int16_t *tiles = ef_view_tiles(out);
    for (int i = 0; i < EF_VIEW_PLANES * n; i++) tiles[i] = EF_VIEW_UNKNOWN;
    uint32_t revealed;
    EF_TRY(ef_revealed_kinds(g, t, viewer, &revealed));
    const int rainbow = (ef_tribe_flags(t, me->tribe) & EF_TRIBE_RAINBOWVISION) != 0;
#define EF_PLANE(p) (tiles[(p) * n + i])
    for (int i = 0; i < n; i++) {
        const EfTile *x = &ef_ctiles(g)[i];
        if (!ef_contains_u8(x->explorers_n, x->explorers_len_n, viewer)) {
            EF_PLANE(EF_P_EXPLORED) = 0;
            EF_PLANE(EF_P_RUIN_HINT) = (int16_t)(rainbow && x->has_improvement_n && x->improvement.kind_n == EF_RUIN);
            continue;
        }
        EF_PLANE(EF_P_EXPLORED) = 1;
        EF_PLANE(EF_P_RUIN_HINT) = 0;
        EF_PLANE(EF_P_TERRAIN) = ef_i16(x->terrain_n);
        EF_PLANE(EF_P_CLIMATE) = ef_i16(x->climate_n);
        /* a resource shows on a tile without an improvement, if the viewer's tech reveals its kind */
        int16_t resource = 0;
        if (x->has_resource_n && !x->has_improvement_n && x->resource_n >= 0 && x->resource_n < 32
            && ((revealed >> x->resource_n) & 1u))
            resource = ef_i16(x->resource_n);
        EF_PLANE(EF_P_RESOURCE) = resource;
        EF_PLANE(EF_P_OWNER) = ef_i16(x->owner_n);
        EF_PLANE(EF_P_CAPITAL_OF) = ef_i16(x->capital_of_n);
        int32_t ruling = ef_tile_index(g, x->ruling_city);
        EF_PLANE(EF_P_RULING_CITY) = ruling == EF_NONE ? EF_VIEW_UNKNOWN : ef_i16(ruling);
        EF_PLANE(EF_P_ROAD) = (int16_t)(x->has_road_n != 0);
        EF_PLANE(EF_P_ROUTE) = (int16_t)(x->has_route_n != 0);
        int16_t effects = 0;
        for (int k = 0; k < x->effects_len_n; k++) {
            if (x->effects_n[k] < 0 || x->effects_n[k] >= 15) return ef_err(EF_ERR_VIEW_EFFECT, x->effects_n[k]);
            effects = (int16_t)(effects | (1 << x->effects_n[k]));
        }
        EF_PLANE(EF_P_EFFECTS) = effects;
        if (x->has_improvement_n && ef_improvement_known(t, x->improvement.kind_n)) {
            const EfImprovement *m = &x->improvement;
            EF_PLANE(EF_P_IMPROVEMENT) = ef_i16(m->kind_n);
            EF_PLANE(EF_P_IMPROVEMENT_LEVEL) = m->kind_n == EF_CITY ? ef_i16(m->level_n) : 0;
            int fi = ef_player_index(g, m->founder_n);
            EF_PLANE(EF_P_FOUNDER_TRIBE) = fi == EF_NONE ? 0 : ef_i16(g->players[fi].tribe);
            if (m->kind_n == EF_CITY) {
                EF_PLANE(EF_P_CITY_PARKS) = (int16_t)m->rewards_n[EF_PARK_REWARD];
                EF_PLANE(EF_P_CITY_SUPER_UNITS) = (int16_t)m->rewards_n[EF_SUPER_UNIT_REWARD];
            }
            if (m->kind_n == EF_CITY && x->owner_n == viewer) {
                EF_PLANE(EF_P_CITY_POPULATION) = ef_i16(m->population_n);
                EF_PLANE(EF_P_CITY_PROGRESS) = ef_i16(m->xp_n);
                EF_PLANE(EF_P_CITY_CONNECTED) = ef_i16(m->connected_to_capital_of_player_n);
                EF_PLANE(EF_P_CITY_WALL) = (int16_t)(m->rewards_n[EF_CITY_WALL_REWARD] != 0);
            }
        } else {
            EF_PLANE(EF_P_IMPROVEMENT) = 0;
            EF_PLANE(EF_P_IMPROVEMENT_LEVEL) = 0;
        }
        EF_PLANE(EF_P_UNIT_KIND) = 0;
        EF_PLANE(EF_P_UNIT_OWNER) = 0;
        if (x->unit_n < 0 || x->unit_n >= g->unit_cap) continue;
        const EfUnit *u = &ef_cunits(g)[x->unit_n];
        const EfEffectList *fx = ef_effect_list(g, u->effects_n);
        /* an invisible foreign unit shows only to a player at peace with its owner */
        if (u->owner_n != viewer && ef_has_effect(fx, EF_INVISIBLE) && ef_relation(g, viewer, u->owner_n) != EF_PEACE)
            continue;
        EF_PLANE(EF_P_UNIT_KIND) = ef_i16(u->kind_n);
        EF_PLANE(EF_P_UNIT_OWNER) = ef_i16(u->owner_n);
        /* health as the game displays it: (health as u16).div_ceil(10) as i16 */
        uint32_t health = (uint16_t)(uint32_t)u->health_n;
        EF_PLANE(EF_P_UNIT_HEALTH) = (int16_t)((health + 9u) / 10u);
        EF_PLANE(EF_P_UNIT_VETERAN) = (int16_t)(u->promotion_level_n != 0);
        EF_PLANE(EF_P_UNIT_KILLS) = ef_i16(u->xp_n);
        if (u->owner_n == viewer) {
            int16_t unit_effects = 0;
            for (int k = 0; k < fx->len_n; k++) {
                if (fx->items_n[k] < 0 || fx->items_n[k] >= 15) return ef_err(EF_ERR_VIEW_EFFECT, fx->items_n[k]);
                unit_effects = (int16_t)(unit_effects | (1 << fx->items_n[k]));
            }
            EF_PLANE(EF_P_UNIT_EFFECTS) = unit_effects;
            EF_PLANE(EF_P_UNIT_PASSENGER) =
                (u->passenger_n >= 0 && u->passenger_n < g->unit_cap) ? ef_i16(ef_cunits(g)[u->passenger_n].kind_n) : 0;
            EF_PLANE(EF_P_UNIT_MOVED) = (int16_t)(u->moved_n != 0);
            EF_PLANE(EF_P_UNIT_ATTACKED) = (int16_t)(u->attacked_n != 0);
            int32_t home = ef_tile_index(g, u->home);
            EF_PLANE(EF_P_UNIT_HOME) = home == EF_NONE ? EF_VIEW_UNKNOWN : ef_i16(home);
        }
    }
#undef EF_PLANE
    /* the viewer's techs, ascending */
    out->tech_len = me->available_tech_len;
    for (int i = 0; i < EF_MAX_TECH; i++) out->tech[i] = 0;
    for (int i = 0; i < me->available_tech_len; i++) {
        int16_t v = ef_i16(me->available_tech[i]);
        int k = i;
        while (k > 0 && out->tech[k - 1] > v) { out->tech[k] = out->tech[k - 1]; k--; }
        out->tech[k] = v;
    }
    out->revealed_resources = revealed;
    int32_t start = ef_tile_index(g, me->start_tile);
    out->start_tile = start == EF_NONE ? EF_VIEW_UNKNOWN : ef_i16(start);
    out->units_lost = me->casualties;
    out->monuments_len = me->built_unique_len;
    for (int k = 0; k < 8; k++) out->monuments[k] = k < me->built_unique_len ? ef_i16(me->built_unique[k]) : 0;
    /* the tasks as stored, with the counter the task keeps (peaceful turns, kills, conversions) */
    out->tasks_len = me->tasks_len;
    for (int k = 0; k < EF_MAX_TASKS; k++) {
        EfTaskView v;
        v.kind = 0; v.completed = 0; v.counter = 0;
        if (k < me->tasks_len) {
            const EfTask *task = &me->tasks[k];
            int has = 0;
            int64_t c = 0;
            if (task->kind == EF_VIEW_TASK_PACIFIST) { has = task->has_peaceful_turns; c = task->peaceful_turns; }
            else if (task->kind == EF_VIEW_TASK_KILLER) { has = task->has_kills; c = task->kills; }
            else if (task->kind == EF_VIEW_TASK_CONVERT) { has = task->has_conversions; c = task->conversions; }
            if (has && (c < INT32_MIN || c > INT32_MAX)) return ef_err(EF_ERR_VIEW_TASK_COUNTER, 0);
            v.kind = ef_i16(task->kind);
            v.completed = (int16_t)(task->completed != 0);
            v.counter = has ? (int32_t)c : EF_VIEW_UNKNOWN;
        }
        out->tasks[k] = v;
    }
    /* the lighthouses on board corners the viewer has explored, with their explorers in order */
    out->lighthouses_len = 0;
    for (int k = 0; k < 4; k++) { EfLighthouseView z; z.tile = 0; for (int e = 0; e < 5; e++) z.explorers[e] = 0; out->lighthouses[k] = z; }
    {
        int32_t w = g->width - 1, h = n / g->width - 1;
        const int32_t cx[4] = { w, 0, 0, w }, cy[4] = { h, 0, h, 0 };
        for (int k = 0; k < 4; k++) {
            EfXy corner;
            corner.x = cx[k];
            corner.y = cy[k];
            int32_t i = ef_tile_index(g, corner);
            if (i == EF_NONE) continue;
            const EfTile *x = &ef_ctiles(g)[i];
            int lighthouse = x->has_improvement_n && ef_improvement_known(t, x->improvement.kind_n)
                             && t->improvements[x->improvement.kind_n].named == EF_NAMED_LIGHTHOUSE;
            if (!lighthouse || !ef_contains_u8(x->explorers_n, x->explorers_len_n, viewer)) continue;
            EfLighthouseView v;
            v.tile = ef_i16(i);
            int e = 0;
            for (int q = 0; q < 5; q++) v.explorers[q] = 0;
            for (int q = 0; q < x->explorers_len_n; q++)
                if (x->explorers_n[q] != EF_NATURE) v.explorers[e++] = (int16_t)x->explorers_n[q];
            out->lighthouses[out->lighthouses_len++] = v;
        }
    }
    /* every seat with its colour, nature included */
    out->colors_len = g->player_count;
    for (int k = 0; k < EF_MAX_PLAYERS; k++) {
        EfColorView c;
        c.seat = 0; c.pad[0] = c.pad[1] = c.pad[2] = 0; c.color = 0;
        if (k < g->player_count) { c.seat = ef_i16(g->players[k].id); c.color = g->players[k].color; }
        out->colors[k] = c;
    }
    out->pad = 0;
    /* the viewer first, then the players it has met, by seat */
    for (int i = 0; i < EF_MAX_PLAYERS; i++) { EfPlayerView z; z.seat = 0; z.tribe = 0; z.relation = 0; z.pad = 0; z.score = 0; out->players[i] = z; }
    int count = 0;
    out->players[count++] = ef_view_player(me, me);
    for (int i = 0; i < g->player_count; i++) {
        const EfPlayer *p = &g->players[i];
        if (p->id == viewer || p->id == EF_NATURE || !ef_contains(me->known_players, me->known_players_len, p->id)) continue;
        EfPlayerView v = ef_view_player(me, p);
        int k = count++;
        /* stable insertion by seat among the met players (index 0 is the viewer) */
        while (k > 1 && out->players[k - 1].seat > v.seat) { out->players[k] = out->players[k - 1]; k--; }
        out->players[k] = v;
    }
    out->players_len = count;
    return ef_ok();
}

#endif /* EF_VIEW_H */
