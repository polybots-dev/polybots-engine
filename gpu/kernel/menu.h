/* menu.h - port of the legal list: engine-fast/src/menu.rs menu (and the move flood of
 * movement.rs moves_at, the attack targets of combat.rs attacks_into) as the batch hands it
 * out (batch/src/lib.rs refresh and as_move): one row of six integers per move, tile
 * coordinates as indices y * width + x, -1 where a field does not apply, the reply to a
 * peace request as option 1 or 0.
 *
 * The candidates are made in the engine's order, and a row whose first five fields repeat
 * an earlier row's is dropped as it is made (menu.rs once_each keeps the first of two equal
 * commands, with its price); then the list is sorted by those five fields as the batch sorts
 * it. No two rows are equal there, so any correct sort gives the batch's order. The attack previews
 * and embark flags the engine attaches are not rows and are not made; the battle a preview
 * runs is, since it can stop the list. */
#ifndef EF_MENU_H
#define EF_MENU_H

#include "step.h"

/* distinct rows on every board: the longest lists measured are 1 141 (16 x 16, 2 seats) and 1 151
 * (30 x 30, 4 seats) in 6 000-decision random games; a longer list stops with a capacity
 * code */
#define EF_MAX_ROWS 2048
#define EF_ROW_HASH 4096        /* twice the rows, a power of two */
#ifdef __cplusplus
static_assert(EF_MAX_ROWS <= INT16_MAX, "row indices fit the int16 hash");
#else
_Static_assert(EF_MAX_ROWS <= INT16_MAX, "row indices fit the int16 hash");
#endif
#define EF_MAX_UNLOCKED 256
#define EF_FLOOD_QUEUE 1000

enum { EF_K_BUILD = 1, EF_K_ATTACK = 2, EF_K_RECOVER = 3, EF_K_HEAL_OTHERS = 4, EF_K_TRAIN = 5, EF_K_MOVE = 6,
       EF_K_CAPTURE = 7, EF_K_RESEARCH = 8, EF_K_DESTROY = 9, EF_K_DISBAND = 10, EF_K_CITY_REWARD = 11,
       EF_K_PROMOTE = 13, EF_K_EXAMINE_RUINS = 14, EF_K_END_TURN = 15, EF_K_UPGRADE = 16, EF_K_END_MATCH = 22, EF_K_EXPLODE = 24, EF_K_BOOST = 25, EF_K_SWARM = 38,
       EF_K_DECOMPOSE = 26, EF_K_PEACE_TREATY = 27, EF_K_PEACE_REPLY = 28, EF_K_BREAK_PEACE = 29,
       EF_K_FREEZE_AREA = 17, EF_K_BREAK_ICE = 18, EF_K_ESTABLISH_EMBASSY = 30, EF_K_RESIGN = 35,
       EF_K_DISEMBARK = 36, EF_K_CLEAR_TILE_EFFECT = 39 };

/* batch Move: kind, source, target, option, opponent, price */
typedef struct { int32_t f[6]; } EfRow;
/* seen: open addressing over the rows made so far (their index, -1 free) */
typedef struct {
    int32_t len, pad;
    EfRow rows[EF_MAX_ROWS];
    int16_t seen[EF_ROW_HASH];
} EfMenu;
#ifdef __cplusplus
static_assert(sizeof(EfMenu) % 8 == 0, "menus are laid end to end in u64 words");
#else
_Static_assert(sizeof(EfMenu) % 8 == 0, "menus are laid end to end in u64 words");
#endif

EF_FN void ef_menu_clear(EfMenu *m) {
    m->len = 0;
    m->pad = 0;
    for (int k = 0; k < EF_ROW_HASH; k++) m->seen[k] = -1;
}

EF_FN uint32_t ef_row_hash(const EfRow *r) {
    uint32_t h = 0x811c9dc5u;
    for (int k = 0; k < 5; k++) h = (h ^ (uint32_t)r->f[k]) * 0x01000193u;
    return h ^ (h >> 15);
}

/* the per-list work: per tile that rules territory, the limited improvement kinds already
 * built within 2 of it on tiles it rules (ef_limited_kinds); the unlocked improvements and build
 * options, the shared move context and the flood's arrays */
typedef struct {
    uint64_t limited[EF_MAX_TILES];
    int32_t have_limited;
    int32_t unlocked_len, options_len, have_unlocked, have_mover;
    int32_t unlocked[EF_MAX_UNLOCKED];
    int32_t option_kind[EF_MAX_IMPROVEMENTS], option_cost[EF_MAX_IMPROVEMENTS], option_freelance[EF_MAX_IMPROVEMENTS];
    EfMover mover;
    int32_t costs[EF_MAX_TILES];
    int32_t queue[EF_FLOOD_QUEUE];
} EfMenuWork;

EF_FN EfResult ef_row(const EfGame *g, EfMenu *m, int32_t kind, EfXy source, EfXy target, int32_t option, int32_t opponent,
                      int32_t price) {
    EfRow r;
    r.f[0] = kind;
    r.f[1] = source.x < 0 ? -1 : source.y * g->width + source.x;
    r.f[2] = target.x < 0 ? -1 : target.y * g->width + target.x;
    r.f[3] = option;
    r.f[4] = opponent;
    r.f[5] = price;
    uint32_t at = ef_row_hash(&r) & (EF_ROW_HASH - 1);
    while (m->seen[at] >= 0) {
        const EfRow *o = &m->rows[m->seen[at]];
        int same = 1;
        for (int k = 0; k < 5; k++) same &= o->f[k] == r.f[k];
        if (same) return ef_ok();
        at = (at + 1) & (EF_ROW_HASH - 1);
    }
    if (m->len >= EF_MAX_ROWS) return ef_err(EF_ERR_ROWS_FULL, 0);
    m->seen[at] = (int16_t)m->len; /* narrow: len < EF_MAX_ROWS <= INT16_MAX */
    m->rows[m->len++] = r;
    return ef_ok();
}
#define EF_NO ef_xy(-1, -1)

/* the player's entry for `other` without making one (menu.rs embassy_level) */
EF_FN int32_t ef_embassy_of(const EfPlayer *p, int32_t other) {
    int n = ef_index_of(p->relation_ids, p->relations_len, other);
    return n < 0 ? 0 : p->relations[n].embassy_level;
}

EF_FN EfResult ef_mover_player(const EfGame *g, int32_t mover, const EfPlayer **p) {
    int i = ef_player_index(g, mover);
    if (i == EF_NONE) return ef_err(EF_ERR_MISSING_MOVER, 0);
    *p = &g->players[i];
    return ef_ok();
}

/* menu.rs pending_choice */
EF_FN EfResult ef_pending_choice(const EfGame *g, const EfTrigger *x, EfMenu *m) {
    if (x->kind == EF_CITY_LEVEL_UP) {
        const EfTile *tile = ef_tile_c(g, x->coordinates);
        if (!tile || !tile->has_improvement_n || tile->improvement.kind_n != EF_CITY) return ef_err(EF_ERR_LEVEL_UP_NO_CITY, 0);
        int32_t level = tile->improvement.level_n;
        if (level < 2) return ef_err(EF_ERR_REWARD_BELOW_2, 0);
        const int32_t pairs[4][2] = { { 3, 4 }, { 1, 7 }, { 8, 5 }, { 2, 6 } };
        int32_t k = level - 2 < 3 ? level - 2 : 3;
        for (int r = 0; r < 2; r++) EF_TRY(ef_row(g, m, EF_K_CITY_REWARD, EF_NO, x->coordinates, pairs[k][r], -1, -1));
        return ef_ok();
    }
    if (x->kind == EF_PEACE_REQUEST_TRIGGER) {
        EF_TRY(ef_row(g, m, EF_K_PEACE_REPLY, EF_NO, EF_NO, 1, x->opponent, -1));
        return ef_row(g, m, EF_K_PEACE_REPLY, EF_NO, EF_NO, 0, x->opponent, -1);
    }
    return ef_err(EF_ERR_UNKNOWN_TRIGGER, 0);
}

EF_FN EfResult ef_resign_row(const EfGame *g, EfMenu *m) {
    return g->seats == 2 ? ef_row(g, m, EF_K_RESIGN, EF_NO, EF_NO, -1, -1, -1) : ef_ok();
}

/* menu.rs train_into */
EF_BIG EfResult ef_train_rows(const EfGame *g, const EfTables *t, int32_t mover, int i, EfMenu *m) {
    const EfTile *tile = &ef_ctiles(g)[i];
    if (tile->owner_n != mover || tile->unit_n != EF_NONE || !tile->has_improvement_n || tile->improvement.kind_n != EF_CITY)
        return ef_ok();
    const EfPlayer *p;
    EF_TRY(ef_mover_player(g, mover, &p));
    const EfTechTree *tree;
    EF_TRY(ef_known_tribe(t, p, &tree));
    if (p->block_train_units) return ef_ok();
    const EfTile *ruler = ef_tile_c(g, tile->ruling_city);
    int home = ruler && ruler->has_improvement_n && ruler->improvement.kind_n == EF_CITY;
    EfXy at = home ? ruler->coordinates : EF_NO;
    int32_t level = home ? ruler->improvement.level_n : 0;
    for (int k = 0; k < p->available_tech_len; k++) {
        const int32_t *list;
        int32_t len;
        EF_TRY(ef_tech_unlocks(t, tree, p->available_tech[k], 1, &list, &len));
        for (int q = 0; q < len; q++) {
            int32_t id = list[q];
            const EfUnitData *u = &t->units[id];
            if (u->hidden) continue;
            if (ef_has(u->abilities, EF_AB_UNIQUE) && ef_owns_kind(g, mover, id)) continue;
            if (u->has_movement_terrain) {
                if (u->movement_terrain_error) return ef_err(EF_ERR_TABLE, u->movement_terrain_error - 1);
                /* Terrain match: a neighbour of the listed terrain, wetland (Flooded or
                 * Algae) counting as terrain 7 */
                int wet = (u->movement_terrain & ef_terrain_bit(EF_WETLAND)) != 0;
                EfXy c = tile->coordinates;
                int near = 0;
                for (int32_t x = c.x - 1; x <= c.x + 1 && !near; x++)
                    for (int32_t y = c.y - 1; y <= c.y + 1 && !near; y++) {
                        if (x == c.x && y == c.y) continue;
                        const EfTile *n = ef_tile_c(g, ef_xy(x, y));
                        near = n && ((u->movement_terrain & ef_terrain_bit(n->terrain_n)) != 0 || (wet && ef_wetland(n)));
                    }
                if (!near) continue;
            }
            if (p->currency < u->cost) continue;
            if (!home) continue;
            int32_t count = 0;
            for (int s = 0; s < g->tile_count; s++) {
                int32_t n = ef_ctiles(g)[s].unit_n;
                if (n != EF_NONE && ef_xy_eq(ef_cunits(g)[n].home, at)) count++;
            }
            if (count > level) continue;
            EF_TRY(ef_row(g, m, EF_K_TRAIN, EF_NO, tile->coordinates, id, -1, u->cost));
        }
    }
    return ef_ok();
}

/* menu.rs unlocked_improvements */
EF_BIG EfResult ef_unlocked_improvements(const EfGame *g, const EfTables *t, int32_t mover, EfMenuWork *w) {
    const EfPlayer *p;
    EF_TRY(ef_mover_player(g, mover, &p));
    const EfTechTree *tree;
    EF_TRY(ef_known_tribe(t, p, &tree));
    w->unlocked_len = 0;
    for (int k = 0; k < p->available_tech_len; k++) {
        const int32_t *list;
        int32_t len;
        EF_TRY(ef_tech_unlocks(t, tree, p->available_tech[k], 0, &list, &len));
        for (int q = 0; q < len; q++) {
            if (t->improvements[list[q]].hidden) continue;
            if (w->unlocked_len >= EF_MAX_UNLOCKED) return ef_err(EF_ERR_UNLOCKED_FULL, 0);
            w->unlocked[w->unlocked_len++] = list[q];
        }
    }
    for (int n = 0; n < p->tasks_len; n++) {
        if (!p->tasks[n].completed) continue;
        int32_t kind = p->tasks[n].kind;
        if (kind < 0 || kind >= EF_TASK_KIND_COUNT) return ef_err(EF_ERR_INVARIANT, __LINE__);
        if (t->task_unlocks_error[kind]) return ef_err(EF_ERR_TABLE, t->task_unlocks_error[kind] - 1);
        for (int q = 0; q < t->task_unlocks_len[kind]; q++) {
            if (w->unlocked_len >= EF_MAX_UNLOCKED) return ef_err(EF_ERR_UNLOCKED_FULL, 0);
            w->unlocked[w->unlocked_len++] = t->task_unlocks[kind][q];
        }
    }
    return ef_ok();
}

/* menu.rs build_options */
EF_FN EfResult ef_build_options(const EfGame *g, const EfTables *t, int32_t mover, EfMenuWork *w) {
    const EfPlayer *p;
    EF_TRY(ef_mover_player(g, mover, &p));
    w->options_len = 0;
    for (int k = 0; k < w->unlocked_len; k++) {
        int32_t kind = w->unlocked[k];
        const EfImprovementData *d = &t->improvements[kind];
        int32_t cost = d->has_cost ? d->cost : 0;
        if (ef_has(d->abilities, EF_AB_MANUAL) || d->hidden || p->currency < cost
            || ef_contains(w->option_kind, w->options_len, kind))
            continue;
        w->option_kind[w->options_len] = kind;
        w->option_cost[w->options_len] = cost;
        w->option_freelance[w->options_len] = ef_has(d->abilities, EF_AB_FREELANCE);
        w->options_len++;
    }
    return ef_ok();
}

/* menu.rs adjacency */
EF_BIG EfResult ef_meets_adjacency(const EfGame *g, const EfTile *tile, const EfImprovementData *d, int *ok) {
    int32_t n = d->adjacency_requirement_len;
    *ok = 1;
    if (n == 0) return ef_ok();
    int positive = 0;
    for (int k = 0; k < n; k++) positive |= d->adjacency_requirements[k].positive;
    EfXy c = tile->coordinates;
    for (int32_t x = c.x - 1; x <= c.x + 1; x++) {
        for (int32_t y = c.y - 1; y <= c.y + 1; y++) {
            if (x == c.x && y == c.y) continue;
            const EfTile *o = ef_tile_c(g, ef_xy(x, y));
            if (!o) continue;
            for (int k = 0; k < n; k++) {
                const EfNames *e = &d->adjacency_requirements[k];
                if (e->not_improvement_error) return ef_err(EF_ERR_TABLE, e->not_improvement_error - 1);
                if (e->not_improvement != EF_NONE && o->has_improvement_n && o->improvement.kind_n == e->not_improvement) {
                    *ok = 0;
                    return ef_ok();
                }
                if (o->owner_n != tile->owner_n) continue;
                if (e->improvement_error) return ef_err(EF_ERR_TABLE, e->improvement_error - 1);
                if (e->improvement != EF_NONE && o->has_improvement_n && o->improvement.kind_n == e->improvement) return ef_ok();
                if (e->terrain_error) return ef_err(EF_ERR_TABLE, e->terrain_error - 1);
                if (e->terrain != EF_NONE && e->terrain != 0 && o->terrain_n == e->terrain) return ef_ok();
                if (e->resource_error) return ef_err(EF_ERR_TABLE, e->resource_error - 1);
                if (e->resource != EF_NONE && e->resource != 0) {
                    if (!o->has_resource_n) return ef_err(EF_ERR_ADJACENCY_NULL, 0);
                    if (o->resource_n == e->resource) return ef_ok();
                }
            }
        }
    }
    *ok = !positive;
    return ef_ok();
}

/* For ef_can_build's `limited` test, once per list: per tile, the improvement kinds built on
 * the other tiles it rules within 2 of it. The test asks whether such a tile holds the kind
 * around the candidate's ruling city; one pass over the board answers it for every city. */
EF_FN void ef_limited_kinds(const EfGame *g, EfMenuWork *w) {
    for (int i = 0; i < g->tile_count; i++) w->limited[i] = 0;
    for (int i = 0; i < g->tile_count; i++) {
        const EfTile *o = &ef_ctiles(g)[i];
        if (!o->has_improvement_n || o->improvement.kind_n < 0 || o->improvement.kind_n >= 64) continue;
        EfXy city = o->ruling_city;
        int32_t c = ef_tile_index(g, city);
        if (c == EF_NONE || ef_xy_eq(o->coordinates, city)) continue;
        int32_t dx = o->coordinates.x - city.x, dy = o->coordinates.y - city.y;
        if (dx < -2 || dx > 2 || dy < -2 || dy > 2) continue;
        w->limited[c] |= 1ull << o->improvement.kind_n;
    }
    w->have_limited = 1;
}

/* menu.rs can_build */
EF_BIG EfResult ef_can_build(const EfGame *g, const EfTables *t, int32_t mover, const EfTile *tile, int32_t kind, EfMask abilities,
                             const EfMenuWork *w, int *ok) {
    const EfImprovementData *d = &t->improvements[kind];
    *ok = 0;
    if (d->hidden) return ef_ok();
    int freelance = ef_has(d->abilities, EF_AB_FREELANCE), manual = ef_has(d->abilities, EF_AB_MANUAL);
    if (tile->owner_n != mover && (!freelance || (tile->owner_n != 0 && !manual))) return ef_ok();
    if (kind == EF_ROAD) {
        if (tile->has_road_n || (tile->has_improvement_n && (tile->improvement.kind_n == EF_CITY || tile->improvement.kind_n == EF_BRIDGE)))
            return ef_ok();
    } else if (tile->has_improvement_n && !ef_has(d->abilities, EF_AB_DISCRETE)) {
        return ef_ok();
    }
    if (kind == EF_BRIDGE) {
        int anchor[4];
        const int32_t dx[4] = { 0, 0, 1, -1 }, dy[4] = { -1, 1, 0, 0 };
        for (int k = 0; k < 4; k++) {
            const EfTile *n = ef_tile_c(g, ef_xy(tile->coordinates.x + dx[k], tile->coordinates.y + dy[k]));
            anchor[k] = n && (uint32_t)n->terrain_n - 3u < 0xfffffffeu;
        }
        int none = !(anchor[0] && anchor[1]) && !(anchor[2] && anchor[3]);
        if (none || ef_contains_i8(tile->effects_n, tile->effects_len_n, EF_FLOODED)) return ef_ok();
    }
    const EfUnit *visible = ef_visible_unit(g, tile, mover);
    if (visible && visible->owner_n != mover && ef_relation(g, mover, visible->owner_n) != EF_PEACE) return ef_ok();
    if (!visible && manual) return ef_ok();
    if (ef_has(d->abilities, EF_AB_LIMITED)) {
        EfXy city = tile->ruling_city;
        int32_t c = ef_tile_index(g, city);
        /* the list's masks hold every on-board city; a city off the board keeps the scan */
        if (w->have_limited && c != EF_NONE && kind >= 0 && kind < 64) {
            if ((w->limited[c] >> kind) & 1u) return ef_ok();
        } else
        for (int32_t y = city.y - 2; y <= city.y + 2; y++)
            for (int32_t x = city.x - 2; x <= city.x + 2; x++) {
                const EfTile *o = ef_tile_c(g, ef_xy(x, y));
                if (o && !ef_xy_eq(o->coordinates, city) && ef_xy_eq(o->ruling_city, city) && o->has_improvement_n
                    && o->improvement.kind_n == kind)
                    return ef_ok();
            }
    }
    if (ef_has(d->abilities, EF_AB_UNIQUE)) {
        int i = ef_player_index(g, mover);
        if (i != EF_NONE && ef_contains(g->players[i].built_unique, g->players[i].built_unique_len, kind)) return ef_ok();
    }
    for (int k = 0; k < d->creates_len; k++) {
        const EfCreates *e = &d->creates[k];
        if (e->effect_kind_error) return ef_err(EF_ERR_TABLE, e->effect_kind_error - 1);
        if (e->resource_error) return ef_err(EF_ERR_TABLE, e->resource_error - 1);
        int32_t effect = e->effect_kind;
        int has_resource = e->resource != EF_NONE;
        if ((has_resource && e->resource != 0 && tile->has_resource_n && tile->resource_n == e->resource && effect == 0)
            || (effect != 0 && ef_contains_i8(tile->effects_n, tile->effects_len_n, effect) && !has_resource))
            return ef_ok();
    }
    int flooded = ef_flooded(tile);
    int algae = ef_contains_i8(tile->effects_n, tile->effects_len_n, EF_ALGAE);
    int wetland = flooded || algae;
    /* Flood (Canal): not on a flooded tile; Fill: something to drain, and no unit stranded by it */
    if (ef_has(d->abilities, EF_AB_FLOOD) && flooded) return ef_ok();
    if (ef_has(d->abilities, EF_AB_FILL)) {
        if (!wetland) return ef_ok();
        if (visible
            && ((ef_unit_has(t, visible->kind_n, EF_AB_WATER) && flooded) || (ef_unit_has(t, visible->kind_n, EF_AB_LAND) && algae)))
            return ef_ok();
    }
    int has_res = 0, res_ok = 0, has_ter = 0, ter_ok = 0;
    for (int k = 0; k < d->requirement_len; k++) {
        const EfNames *r = &d->requirements[k];
        if (r->resource_error) return ef_err(EF_ERR_TABLE, r->resource_error - 1);
        if (r->resource != EF_NONE && r->resource != 0) {
            has_res = 1;
            res_ok |= tile->has_resource_n && tile->resource_n == r->resource;
        }
        if (r->terrain_error) return ef_err(EF_ERR_TABLE, r->terrain_error - 1);
        int32_t x = r->terrain;
        if (x != EF_NONE && x != 0) {
            has_ter = 1;
            ter_ok |= tile->terrain_n == x || (wetland && tile->terrain_n != EF_FOREST && x == 7)
                      || (flooded && tile->terrain_n == EF_FOREST && x == 8);
            if (x == 3 && tile->terrain_n == EF_FOREST) ter_ok |= ef_has(abilities, EF_AB_TREEHOUSE);
            if (x == 3 && (tile->terrain_n == EF_WATER || tile->terrain_n == EF_OCEAN) && kind != 3 && kind != 17 && kind != 52
                && kind != 14)
                ter_ok |= ef_has(abilities, EF_AB_PONTOON);
        }
    }
    if ((has_res && !res_ok) || (has_ter && !ter_ok)) return ef_ok();
    return ef_meets_adjacency(g, tile, d, ok);
}

/* menu.rs build_with */
EF_FN EfResult ef_build_rows(const EfGame *g, const EfTables *t, int32_t mover, int i, const EfMenuWork *w, EfMask abilities,
                             EfMenu *m) {
    const EfTile *tile = &ef_ctiles(g)[i];
    for (int k = 0; k < w->options_len; k++) {
        if (tile->owner_n != mover && !w->option_freelance[k]) continue;
        int ok;
        EF_TRY(ef_can_build(g, t, mover, tile, w->option_kind[k], abilities, w, &ok));
        if (ok) EF_TRY(ef_row(g, m, EF_K_BUILD, EF_NO, tile->coordinates, w->option_kind[k], -1, w->option_cost[k]));
    }
    return ef_ok();
}

/* menu.rs improvement_abilities_into */
EF_FN EfResult ef_improvement_rows(const EfGame *g, const EfTables *t, int32_t mover, int i, EfMask abilities, EfMenu *m) {
    const EfTile *tile = &ef_ctiles(g)[i];
    if (!tile->has_improvement_n) return ef_ok();
    const EfImprovement *imp = &tile->improvement;
    uint32_t kind = (uint32_t)imp->kind_n;
    if ((kind <= 47 && ((0x7ffffffffff9ull >> (kind & 63)) & 1) == 0) || tile->owner_n != mover) return ef_ok();
    const EfUnit *v = ef_visible_unit(g, tile, mover);
    if (v && v->owner_n != mover && ef_relation(g, mover, v->owner_n) != EF_PEACE) return ef_ok();
    const EfImprovementData *data = ef_improvement_data(t, imp->kind_n);
    if (tile->unit_n != EF_NONE && data && ef_has(data->abilities, EF_AB_BRIDGE)) return ef_ok();
    if (imp->kind_n == 0 || !data) return ef_ok();
    if (ef_has(abilities, EF_AB_DESTROY)) EF_TRY(ef_row(g, m, EF_K_DESTROY, EF_NO, tile->coordinates, -1, -1, -1));
    int decomposing = ef_contains_i8(imp->effects_n, imp->effects_len_n, 0);
    if (!decomposing && ef_has(abilities, EF_AB_DECOMPOSE))
        EF_TRY(ef_row(g, m, EF_K_DECOMPOSE, EF_NO, tile->coordinates, -1, -1, -1));
    return ef_ok();
}

/* menu.rs unit_actions_with */
EF_BIG EfResult ef_unit_rows(const EfGame *g, const EfTables *t, int32_t mover, int i, const EfMenuWork *w, EfMask abilities,
                             EfMenu *m) {
    const EfTile *tile = &ef_ctiles(g)[i];
    if (tile->unit_n == EF_NONE) return ef_ok();
    const EfUnit *u = &ef_cunits(g)[tile->unit_n];
    if (u->owner_n != mover) return ef_ok();
    const EfUnitData *d = ef_unit_data(t, u->kind_n);
    if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    EfXy c = tile->coordinates;
    int frozen = ef_unit_effect(g, u, EF_FROZEN);
    int can_move = !u->moved_n && !frozen;
    int can_attack = !u->attacked_n && !frozen && (!u->moved_n || ef_has(d->abilities, EF_AB_DASH));
    int leader = u->leader == 0;
    const EfUnit *passenger = u->passenger_n != EF_NONE ? &ef_cunits(g)[u->passenger_n] : 0;
    int has_improvement = tile->has_improvement_n;
    int32_t improvement = tile->improvement.kind_n;
    int32_t rel = ef_relation(g, mover, tile->owner_n);
    if (can_move && can_attack && leader && tile->owner_n != mover && has_improvement && improvement == EF_CITY && rel != EF_PEACE
        && rel != EF_COOL_DOWN)
        EF_TRY(ef_row(g, m, EF_K_CAPTURE, c, EF_NO, -1, -1, -1));
    if (can_move && can_attack && leader) {
        if (t->ruin_error) return ef_err(EF_ERR_TABLE, t->ruin_error - 1);
        if (has_improvement && improvement == t->ruin) EF_TRY(ef_row(g, m, EF_K_EXAMINE_RUINS, c, EF_NO, -1, -1, -1));
    }
    if (leader) {
        int32_t max;
        EF_TRY(ef_max_health(g, t, u, &max));
        if ((u->health_n < max || ef_unit_effect(g, u, EF_POISONED)) && !u->moved_n && !frozen && !u->attacked_n)
            EF_TRY(ef_row(g, m, EF_K_RECOVER, c, EF_NO, -1, -1, -1));
    }
    if (ef_has(d->abilities, EF_AB_HEAL) && can_move && !u->attacked_n) {
        int hurt = 0;
        for (int32_t x = c.x - 1; x <= c.x + 1; x++)
            for (int32_t y = c.y - 1; y <= c.y + 1; y++) {
                if (x == c.x && y == c.y) continue;
                const EfTile *n = ef_tile_c(g, ef_xy(x, y));
                if (!n || n->unit_n == EF_NONE || ef_cunits(g)[n->unit_n].owner_n != mover) continue;
                const EfUnit *o = &ef_cunits(g)[n->unit_n];
                int32_t max;
                EF_TRY(ef_max_health(g, t, o, &max));
                hurt |= max > o->health_n || ef_unit_effect(g, o, EF_POISONED);
            }
        if (hurt) EF_TRY(ef_row(g, m, EF_K_HEAL_OTHERS, c, EF_NO, -1, -1, -1));
    }
    /* Disembark: on wetland, or on an improvement with the Bridge ability */
    if (passenger && (ef_wetland(tile) || (has_improvement && improvement != 0 && ef_improvement_has(t, improvement, EF_AB_BRIDGE))))
        EF_TRY(ef_row(g, m, EF_K_DISEMBARK, c, EF_NO, -1, -1, -1));
    const EfUnit *selected = passenger ? passenger : u;
    const EfUnitData *sd = ef_unit_data(t, selected->kind_n);
    if (!sd) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    if (!ef_has(sd->abilities, EF_AB_STATIC) && !sd->hidden) {
        int32_t limit = 3 ^ ef_player_ability(g, t, selected->owner_n, EF_AB_DISCIPLINE);
        if (limit > 0 && selected->xp_n >= limit && selected->promotion_level_n == 0)
            EF_TRY(ef_row(g, m, EF_K_PROMOTE, c, EF_NO, -1, -1, -1));
    }
    int freeze;
    EF_TRY(ef_can_freeze_area(g, t, u, &freeze));
    if (freeze) EF_TRY(ef_row(g, m, EF_K_FREEZE_AREA, c, EF_NO, -1, -1, -1));
    /* the game's candidate carries the command's `what` field, 0 until displayed */
    if (ef_can_break_ice(g, t, u)) EF_TRY(ef_row(g, m, EF_K_BREAK_ICE, c, EF_NO, 0, -1, -1));
    const EfPlayer *p;
    if (!ef_has(d->abilities, EF_AB_GROW)) {
        EF_TRY(ef_mover_player(g, mover, &p));
        int territory = tile->owner_n == mover || (tile->owner_n != 0 && rel == EF_PEACE);
        int32_t ups[EF_MAX_UNIT_KINDS], n;
        EF_TRY(ef_upgradable(g, t, mover, u->kind_n, ups, EF_MAX_UNIT_KINDS, &n));
        for (int k = 0; k < n; k++) {
            const EfUnitData *x = &t->units[ups[k]];
            if (territory && p->currency >= x->cost) EF_TRY(ef_row(g, m, EF_K_UPGRADE, c, EF_NO, ups[k], -1, x->cost));
        }
    }
    if (can_move && !u->attacked_n && leader && ef_has(abilities, EF_AB_DISBAND))
        EF_TRY(ef_row(g, m, EF_K_DISBAND, c, EF_NO, -1, -1, -1));
    /* Boost and Swarm: a unit that can move, with someone to boost */
    if (can_move && ef_mover(g) == mover && ef_boost_options(g, mover, c, 0) > 0) {
        if (ef_has(d->abilities, EF_AB_BOOST)) EF_TRY(ef_row(g, m, EF_K_BOOST, c, EF_NO, -1, -1, -1));
        if (ef_has(d->abilities, EF_AB_SWARM)) EF_TRY(ef_row(g, m, EF_K_SWARM, c, EF_NO, -1, -1, -1));
    }
    if (ef_can_explode(g, t, u)) EF_TRY(ef_row(g, m, EF_K_EXPLODE, c, EF_NO, -1, -1, -1));
    if (can_attack && !u->moved_n && !frozen) {
        EF_TRY(ef_mover_player(g, mover, &p));
        for (int k = 0; k < w->unlocked_len; k++) {
            int32_t kind = w->unlocked[k];
            const EfImprovementData *x = &t->improvements[kind];
            if (!ef_has(x->abilities, EF_AB_MANUAL)) continue;
            int ok;
            EF_TRY(ef_can_build(g, t, mover, tile, kind, abilities, w, &ok));
            if (!ok) continue;
            int32_t cost = x->has_cost ? x->cost : 0;
            if (p->currency >= cost) EF_TRY(ef_row(g, m, EF_K_BUILD, EF_NO, c, kind, -1, cost));
        }
    }
    int32_t clear[2];
    ef_clear_options(g, t, mover, u, tile, can_move, can_attack, clear);
    for (int k = 0; k < 2; k++)
        if (clear[k] >= 0) EF_TRY(ef_row(g, m, EF_K_CLEAR_TILE_EFFECT, c, EF_NO, clear[k], -1, -1));
    return ef_ok();
}

/* movement.rs moves_at and Profile::options: a flood from the unit's tile over the edges and
 * costs of the path search; a tile is offered when first reached and the unit can end there */
EF_BIG EfResult ef_move_rows(const EfGame *g, const EfTables *t, int32_t mover, int i, EfMenuWork *w, EfMenu *m) {
    const EfTile *tile = &ef_ctiles(g)[i];
    if (tile->unit_n == EF_NONE) return ef_ok();
    const EfUnit *unit = &ef_cunits(g)[tile->unit_n];
    if (unit->owner_n != mover || unit->moved_n || ef_unit_effect(g, unit, EF_FROZEN)) return ef_ok();
    if (!w->have_mover) {
        EF_TRY(ef_mover_context(g, t, mover, &w->mover));
        w->have_mover = 1;
    }
    EfProfile s;
    int32_t range;
    EF_TRY(ef_profile_with(g, t, unit, &w->mover, &s, &range));
    if (range == 0) return ef_ok();
    /* a chain moves at its follower's pace, and only its head moves */
    if (unit->follower != 0) {
        int32_t f = ef_find_unit(g, unit->follower);
        if (f != EF_NONE) {
            EfProfile fs;
            int32_t follower_range;
            EF_TRY(ef_profile_with(g, t, &ef_cunits(g)[f], &w->mover, &fs, &follower_range));
            if (follower_range < range) range = follower_range;
        }
    }
    if (unit->leader != 0 || range == 0) return ef_ok();
    for (int k = 0; k < g->tile_count; k++) w->costs[k] = -1;
    int32_t first = ef_profile_index(&s, unit->coordinates);
    w->costs[first] = 0;
    int head = 0, tail = 1;
    w->queue[0] = first;
    int32_t limit = range * 10;
    while (head < tail) {
        int32_t cur = w->queue[head++];
        int32_t cost = w->costs[cur];
        int32_t next[8];
        int n = ef_open_neighbours(&s, cur, ef_ctiles(g)[cur].coordinates, next);
        for (int k = 0; k < n; k++) {
            int32_t candidate = cost + ef_profile_step_cost(&s, &ef_ctiles(g)[next[k]], &ef_ctiles(g)[cur]);
            int32_t old = w->costs[next[k]];
            if (old >= 0 && old <= candidate) continue;
            w->costs[next[k]] = candidate;
            if (old < 0 && ef_profile_can_finish(&s, &ef_ctiles(g)[next[k]]))
                EF_TRY(ef_row(g, m, EF_K_MOVE, tile->coordinates, ef_ctiles(g)[next[k]].coordinates, -1, -1, -1));
            if (candidate < limit && tail < EF_FLOOD_QUEUE) w->queue[tail++] = next[k];
        }
    }
    return ef_ok();
}

/* combat.rs visible_root */
EF_FN const EfUnit *ef_visible_root(const EfGame *g, const EfTile *tile, int32_t viewer) {
    const EfUnit *u = ef_visible_unit(g, tile, viewer);
    return u && u->health_n != 0 && !u->is_temporary_explorer_n ? u : 0;
}

/* combat.rs attacks_into; the preview's battle runs for its stops only */
EF_BIG EfResult ef_attack_rows(const EfGame *g, const EfTables *t, int32_t mover, int i, EfMenu *m) {
    const EfTile *tile = &ef_ctiles(g)[i];
    if (tile->unit_n == EF_NONE) return ef_ok();
    const EfUnit *unit = &ef_cunits(g)[tile->unit_n];
    if (unit->owner_n != mover || unit->attacked_n || ef_unit_effect(g, unit, EF_FROZEN)) return ef_ok();
    const EfUnitData *d = ef_unit_data(t, unit->kind_n);
    if (unit->moved_n) {
        if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
        if (!ef_has(d->abilities, EF_AB_DASH)) return ef_ok();
    }
    if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    EfTargets targets;
    EF_TRY(ef_attack_options(g, t, unit, d->range, 0, &targets));
    for (int k = 0; k < targets.len; k++) {
        const EfTile *target = ef_tile_c(g, targets.at[k]);
        const EfUnit *attacker = ef_visible_root(g, tile, mover);
        const EfUnit *defender = attacker ? ef_visible_root(g, target, mover) : 0;
        if (defender && defender->owner_n != mover) {
            int32_t dealt, back;
            int a, b;
            EF_TRY(ef_battle(g, t, unit, defender, &dealt, &back, &a, &b));
        }
        EF_TRY(ef_row(g, m, EF_K_ATTACK, tile->coordinates, targets.at[k], -1, -1, -1));
    }
    return ef_ok();
}

/* menu.rs research */
EF_FN EfResult ef_research_rows(const EfGame *g, const EfTables *t, int32_t mover, EfMenu *m) {
    const EfPlayer *p;
    EF_TRY(ef_mover_player(g, mover, &p));
    int32_t list[EF_MAX_TECHS], n;
    EF_TRY(ef_unlockable_techs(g, t, mover, list, &n));
    for (int k = 0; k < n; k++) {
        int32_t price = ef_tech_cost_for(g, t, p, list[k]);
        if (price <= p->currency) EF_TRY(ef_row(g, m, EF_K_RESEARCH, EF_NO, EF_NO, list[k], -1, price));
    }
    return ef_ok();
}

/* menu.rs diplomacy */
EF_BIG EfResult ef_diplomacy_rows(const EfGame *g, const EfTables *t, int32_t mover, EfMenu *m) {
    const EfPlayer *me;
    EF_TRY(ef_mover_player(g, mover, &me));
    for (int k = 0; k < me->known_players_len; k++) {
        int32_t opponent = me->known_players[k];
        if (opponent == mover || opponent == 0 || opponent == EF_NATURE) continue;
        int oi = ef_player_index(g, opponent);
        if (oi == EF_NONE) continue;
        const EfPlayer *other = &g->players[oi];
        if (!ef_alive(g, opponent)) continue;
        int peace = ef_relation(g, mover, opponent) == EF_PEACE;
        if (!peace && ef_player_ability(g, t, mover, EF_AB_PEACETREATY)) {
            int requested = 0;
            for (int n = 0; n < other->messages_len; n++)
                requested |= other->message_kinds[n] == EF_PEACE_REQUEST_MESSAGE && other->message_senders[n] == mover;
            if (!requested) EF_TRY(ef_row(g, m, EF_K_PEACE_TREATY, EF_NO, EF_NO, -1, opponent, -1));
        } else if (peace) {
            EF_TRY(ef_row(g, m, EF_K_BREAK_PEACE, EF_NO, EF_NO, -1, opponent, -1));
        }
        const EfTile *capital = ef_tile_c(g, other->start_tile);
        if (!capital) return ef_err(EF_ERR_CAPITAL_TILE, 0);
        int held = (capital->has_improvement_n && capital->improvement.kind_n == EF_CITY)
                   || (capital->unit_n != EF_NONE && ef_cunits(g)[capital->unit_n].owner_n == opponent);
        if (capital->owner_n != opponent || !ef_player_ability(g, t, mover, EF_AB_EMBASSY) || !held) continue;
        int32_t cost = 0;
        for (int q = 0; q < g->player_count; q++) cost += ef_embassy_of(&g->players[q], mover) > 0;
        cost = cost * 2 + 4;
        if (me->currency >= cost && ef_embassy_of(other, mover) == 0)
            EF_TRY(ef_row(g, m, EF_K_ESTABLISH_EMBASSY, EF_NO, EF_NO, -1, opponent, cost));
    }
    return ef_ok();
}

EF_FN int ef_row_before(const EfRow *a, const EfRow *b) {
    for (int k = 0; k < 5; k++)
        if (a->f[k] != b->f[k]) return a->f[k] < b->f[k];
    return 0;
}
/* a heap sort, for a list with a kind outside 0..63 (none today) */
EF_FN void ef_sift(EfRow *rows, int root, int n) {
    for (;;) {
        int child = 2 * root + 1;
        if (child >= n) return;
        if (child + 1 < n && ef_row_before(&rows[child], &rows[child + 1])) child++;
        if (!ef_row_before(&rows[root], &rows[child])) return;
        EfRow x = rows[root];
        rows[root] = rows[child];
        rows[child] = x;
        root = child;
    }
}
EF_FN void ef_heap_sort_rows(EfMenu *m) {
    int n = m->len;
    for (int k = n / 2 - 1; k >= 0; k--) ef_sift(m->rows, k, n);
    for (int end = n - 1; end > 0; end--) {
        EfRow x = m->rows[0];
        m->rows[0] = m->rows[end];
        m->rows[end] = x;
        ef_sift(m->rows, 0, end);
    }
}

/* The batch's sort. The rows are distinct, so any correct sort gives the batch's order. Rows
 * come tile by tile, so a stable counting sort by kind (the key's first field) leaves each kind's
 * rows nearly in order, and an insertion sort within each kind finishes. The places are kept in
 * the list's hash, which is free once the rows are made (ef_menu_clear refills it). */
#define EF_SORT_KINDS 64
EF_FN void ef_sort_rows(EfMenu *m) {
    int32_t n = m->len;
    if (n < 2) return;
    int32_t at[EF_SORT_KINDS + 1], next[EF_SORT_KINDS];
    for (int k = 0; k <= EF_SORT_KINDS; k++) at[k] = 0;
    for (int32_t r = 0; r < n; r++) {
        int32_t kind = m->rows[r].f[0];
        if (kind < 0 || kind >= EF_SORT_KINDS) {
            ef_heap_sort_rows(m);
            return;
        }
        at[kind + 1]++;
    }
    for (int k = 0; k < EF_SORT_KINDS; k++) {
        at[k + 1] += at[k];
        next[k] = at[k];
    }
    int16_t *to = m->seen; /* n <= EF_MAX_ROWS < EF_ROW_HASH */
    for (int32_t r = 0; r < n; r++) to[r] = (int16_t)next[m->rows[r].f[0]]++; /* narrow: < EF_MAX_ROWS */
    /* each row to its place: a swap settles the row at d, and position r takes d's row */
    for (int32_t r = 0; r < n; r++)
        while (to[r] != r) {
            int16_t d = to[r];
            EfRow x = m->rows[d];
            m->rows[d] = m->rows[r];
            m->rows[r] = x;
            to[r] = to[d];
            to[d] = d;
        }
    for (int k = 0; k < EF_SORT_KINDS; k++)
        for (int32_t r = at[k] + 1; r < at[k + 1]; r++) {
            EfRow x = m->rows[r];
            int32_t j = r - 1;
            while (j >= at[k] && ef_row_before(&x, &m->rows[j])) {
                m->rows[j + 1] = m->rows[j];
                j--;
            }
            m->rows[j + 1] = x;
        }
}

/* menu.rs menu, as the batch's sorted rows */
EF_BIG EfResult ef_menu(const EfGame *g, const EfTables *t, EfMenuWork *w, EfMenu *m) {
    ef_menu_clear(m);
    int32_t mover = g->players[g->current_player_index].id;
    for (int k = g->triggers_len - 1; k >= 0; k--) {
        if (g->triggers[k].player != mover) continue;
        EF_TRY(ef_pending_choice(g, &g->triggers[k], m));
        EF_TRY(ef_resign_row(g, m));
        ef_sort_rows(m);
        return ef_ok();
    }
    if (g->current_state == EF_FINAL_TURN) {
        EF_TRY(ef_row(g, m, EF_K_END_MATCH, EF_NO, EF_NO, -1, -1, -1));
        EF_TRY(ef_resign_row(g, m));
        ef_sort_rows(m);
        return ef_ok();
    }
    if (g->current_state != EF_STARTED) return ef_err(EF_ERR_MENU_PHASE, 0);
    if (g->actions_len != 0 || g->current_command != g->commands) return ef_ok();
    EfMask abilities = ef_player_abilities(g, t, mover);
    w->have_unlocked = 0;
    w->have_mover = 0;
    w->have_limited = 0;
    for (int i = 0; i < g->tile_count; i++) {
        if (!ef_tile_explored(&ef_ctiles(g)[i], mover)) continue;
        EF_TRY(ef_train_rows(g, t, mover, i, m));
        if (!w->have_unlocked) {
            EF_TRY(ef_unlocked_improvements(g, t, mover, w));
            EF_TRY(ef_build_options(g, t, mover, w));
            ef_limited_kinds(g, w);
            w->have_unlocked = 1;
        }
        EF_TRY(ef_build_rows(g, t, mover, i, w, abilities, m));
        EF_TRY(ef_improvement_rows(g, t, mover, i, abilities, m));
        EF_TRY(ef_unit_rows(g, t, mover, i, w, abilities, m));
        EF_TRY(ef_move_rows(g, t, mover, i, w, m));
        EF_TRY(ef_attack_rows(g, t, mover, i, m));
    }
    EF_TRY(ef_research_rows(g, t, mover, m));
    EF_TRY(ef_diplomacy_rows(g, t, mover, m));
    EF_TRY(ef_row(g, m, EF_K_END_TURN, EF_NO, EF_NO, -1, -1, -1));
    EF_TRY(ef_resign_row(g, m));
    ef_sort_rows(m);
    return ef_ok();
}

/* The attack preview of each row of a finished list (an engine output;
 * combat.rs attacks_into), int16 x 4 per row: present, damage shown on the target, damage shown
 * on the attacker (-1: the lethal icon, or no retaliation), flags (EF_PREVIEW_*). Rows other
 * than attacks between live units the mover sees are zero. Runs only when a caller asks. */
#define EF_PREVIEW_KILLS 1
#define EF_PREVIEW_DIES 2
#define EF_PREVIEW_RETALIATION 4
EF_BIG EfResult ef_menu_previews(const EfGame *g, const EfTables *t, const EfMenu *m, int16_t (*out)[4]) {
    int32_t mover = g->players[g->current_player_index].id;
    for (int32_t k = 0; k < m->len; k++) {
        const EfRow *r = &m->rows[k];
        for (int f = 0; f < 4; f++) out[k][f] = 0;
        if (r->f[0] != EF_K_ATTACK) continue;
        const EfTile *tile = &ef_ctiles(g)[r->f[1]];
        const EfTile *target = &ef_ctiles(g)[r->f[2]];
        if (tile->unit_n == EF_NONE) return ef_err(EF_ERR_INVARIANT, __LINE__);
        const EfUnit *unit = &ef_cunits(g)[tile->unit_n];
        const EfUnit *attacker = ef_visible_root(g, tile, mover);
        const EfUnit *defender = attacker ? ef_visible_root(g, target, mover) : 0;
        if (!defender || defender->owner_n == mover) continue;
        int32_t dealt, back;
        int a, b;
        EF_TRY(ef_battle(g, t, unit, defender, &dealt, &back, &a, &b));
        /* combat.rs shown: None (the lethal icon) when the damage reaches the health */
        int kills = dealt >= (int32_t)defender->health_n;
        int retaliation = back != -1;
        int dies = retaliation && back >= (int32_t)unit->health_n;
        out[k][0] = 1;
        out[k][1] = kills ? (int16_t)-1 : (int16_t)ef_damage_shown(dealt); /* narrow: dealt < health <= UINT16_MAX, shown a tenth */
        out[k][2] = !retaliation || dies ? (int16_t)-1 : (int16_t)ef_damage_shown(back); /* narrow: as above */
        out[k][3] = (int16_t)((kills ? EF_PREVIEW_KILLS : 0) | (dies ? EF_PREVIEW_DIES : 0) | (retaliation ? EF_PREVIEW_RETALIATION : 0));
    }
    return ef_ok();
}

#endif /* EF_MENU_H */
