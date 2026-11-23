/* movement.h - port of engine-fast/src/movement.rs: the per-unit search profile, step costs and
  * the game's unit path search (A* with an open list kept sorted, ties first in first out).
  * The move-option flood the legal list uses comes with the legal list. */
#ifndef EF_MOVEMENT_H
#define EF_MOVEMENT_H

#include "core.h"

#define EF_FIELD 3
#define EF_WETLAND 7
#define EF_FLOODED 1   /* tile effects */
#define EF_SWAMPED 2
#define EF_TENTACLE_EFFECT 3
#define EF_ALGAE 4
#define EF_FOAM 5
#define EF_SWAMP_SKIN 17
#define EF_CUTE_SKIN 19
#define EF_BRIDGE 48
#define EF_BOOSTED 2
#define EF_BUBBLE 4
#define EF_SWIFT 6
#define EF_STOP_COST 1000
#define EF_MOVE_PATH_TILES 11   /* action.rs PATH_TILES */
#define EF_OPEN_MAX 1024

EF_FN int ef_is_water_tile(const EfTile *t) { return t->terrain_n == EF_WATER || t->terrain_n == EF_OCEAN; }
/* aquarion.rs flooded and wetland */
EF_FN int ef_flooded(const EfTile *t) { return ef_contains_i8(t->effects_n, t->effects_len_n, EF_FLOODED); }
EF_FN int ef_wetland(const EfTile *t) { return ef_flooded(t) || ef_contains_i8(t->effects_n, t->effects_len_n, EF_ALGAE); }

/* a tile effect added unless present (Inline::try_push on the tile's 5) */
EF_FN EfResult ef_tile_effect_add(EfTile *x, int32_t effect) {
    if (ef_contains_i8(x->effects_n, x->effects_len_n, effect)) return ef_ok();
    if (x->effects_len_n >= 5) return ef_short_list_full(5);
    x->effects_n[x->effects_len_n] = (int8_t)effect; /* narrow: a tile effect id below 8 */
    x->effects_len_n = (int8_t)(x->effects_len_n + 1); /* narrow: below 5, checked above */
    return ef_ok();
}

/* the first `effect` taken off a tile's effects, if there */
EF_FN void ef_tile_effect_remove(EfTile *x, int32_t effect) {
    int k = ef_index_of_i8(x->effects_n, x->effects_len_n, effect);
    if (k >= 0) ef_remove_at_i8(x->effects_n, &x->effects_len_n, k);
}

/* aquarion.rs floodable: not water, mountain or ice, and not flooded yet */
EF_FN int ef_floodable(const EfTile *x) {
    int32_t k = x->terrain_n;
    return k != EF_WATER && k != EF_OCEAN && k != EF_MOUNTAIN && k != EF_ICE && !ef_flooded(x);
}

/* aquarion.rs drain_tile: the first Algae and Flooded go, and Swamped on a
 * Swamp-skin tile */
EF_FN void ef_drain_tile(EfTile *x) {
    ef_tile_effect_remove(x, EF_ALGAE);
    ef_tile_effect_remove(x, EF_FLOODED);
    if (x->skin_n == EF_SWAMP_SKIN) ef_tile_effect_remove(x, EF_SWAMPED);
}


/* movement.rs Mover: what a player's units share in a search */
typedef struct {
    uint32_t movements;
    int32_t can_embark, glide, pad;
    uint8_t guarded[EF_MAX_TILES];   /* 0 not asked yet, 1 no, 2 yes */
} EfMover;

/* movement.rs Profile */
typedef struct {
    const EfGame *g;
    const EfTables *t;
    const EfUnit *unit;
    EfMover *mover;
    int32_t owner;
    int slips_past, hide, creep, aquatic, carry, infiltrate, required_port, can_embark, glide, allow_occupied;
    /* fly: every terrain, no embarking, no port, a flat step after zone of control; skate: a step
     * costs 10 onto ice and ends movement anywhere else; water_bound: a swimmer that carries or
     * lists its own terrain, whose movement a flooded tile ends */
    int fly, skate, water_bound, pad;
    uint32_t allowed;
} EfProfile;

typedef struct { int32_t len; EfXy tiles[EF_MOVE_PATH_TILES]; } EfPath;

/* the search's arrays: cost and predecessor per tile, and the open list */
typedef struct {
    int32_t cost[EF_MAX_TILES];
    EfXy came_from[EF_MAX_TILES];
    int32_t open_len, pad;
    EfXy open_xy[EF_OPEN_MAX];
    float open_priority[EF_OPEN_MAX];
} EfSearch;

EF_FN int ef_profile_improvement_has(const EfProfile *s, const EfTile *t, int ability) {
    return t->has_improvement_n && ef_improvement_has(s->t, t->improvement.kind_n, ability);
}
EF_FN const EfUnit *ef_occupant(const EfGame *g, const EfTile *t) {
    return t->unit_n != EF_NONE ? &ef_cunits(g)[t->unit_n] : 0;
}

/* movement.rs Profile::visible */
EF_FN const EfUnit *ef_profile_visible(const EfProfile *s, const EfTile *t) {
    if (!ef_tile_explored(t, s->owner)) return 0;
    const EfUnit *u = ef_occupant(s->g, t);
    if (!u) return 0;
    int hidden = ef_unit_effect(s->g, u, EF_INVISIBLE_EFFECT) && u->owner_n != s->owner
                 && ef_relation(s->g, s->owner, u->owner_n) != EF_PEACE;
    return hidden ? 0 : u;
}

/* movement.rs Profile::disembarks */
EF_FN int ef_profile_disembarks(const EfProfile *s, const EfTile *target) {
    return s->carry && ((target->has_improvement_n && target->improvement.kind_n == EF_CITY)
                        || ((target->terrain_n == EF_FIELD || target->terrain_n == EF_MOUNTAIN || target->terrain_n == EF_FOREST
                             || target->terrain_n == EF_ICE)
                            && !ef_wetland(target)));
}

/* movement.rs Profile::embarks */
EF_FN int ef_profile_embarks(const EfProfile *s, const EfTile *target) {
    if (s->aquatic || s->fly || s->unit->leader != 0 || s->unit->follower != 0 || !(ef_is_water_tile(target) || ef_wetland(target))
        || !ef_profile_improvement_has(s, target, EF_AB_EMBARK) || !s->can_embark)
        return 0;
    return target->owner_n == s->owner || (target->owner_n != 0 && ef_relation(s->g, s->owner, target->owner_n) == EF_PEACE);
}

/* movement.rs Profile::can_finish */
EF_FN int ef_profile_can_finish(const EfProfile *s, const EfTile *t) {
    if (ef_profile_visible(s, t) && !s->allow_occupied) return 0;
    if (t->has_improvement_n && t->improvement.kind_n == EF_CITY) {
        if (s->infiltrate && t->owner_n != 0 && t->owner_n != s->owner) return 0;
        int32_t r = ef_relation(s->g, s->owner, t->owner_n);
        if (t->owner_n != 0 && (r == EF_PEACE || r == EF_COOL_DOWN)) return 0;
    }
    return 1;
}

/* movement.rs Profile::accessible */
EF_FN int ef_profile_accessible(const EfProfile *s, const EfTile *origin, const EfTile *target) {
    int owned_or_peace = target->owner_n == s->owner
                         || (target->owner_n != 0 && ef_relation(s->g, s->owner, target->owner_n) == EF_PEACE);
    if (!ef_tile_explored(target, s->owner)) return 0;
    const EfUnit *o = ef_profile_visible(s, target);
    if (o && !s->allow_occupied && o->owner_n != s->owner
        && !(o->owner_n != 0 && ef_relation(s->g, s->owner, o->owner_n) == EF_PEACE) && !s->hide)
        return 0;
    if (s->fly) return 1;
    if (ef_profile_improvement_has(s, target, EF_AB_BRIDGE)) {
        if (target->has_improvement_n && target->improvement.kind_n == EF_BRIDGE && !(s->allowed & ef_terrain_bit(target->terrain_n))) {
            EfXy c = target->coordinates;
            const EfTile *n;
#define EF_ANCHOR(dx, dy) ((n = ef_tile_c(s->g, ef_xy(c.x + (dx), c.y + (dy)))) && !ef_is_water_tile(n))
            int horizontal = 0, vertical = 0;
            if (EF_ANCHOR(0, -1) && EF_ANCHOR(0, 1)) horizontal = 1;
            else if (EF_ANCHOR(-1, 0) && EF_ANCHOR(1, 0)) vertical = 1;
#undef EF_ANCHOR
            int32_t dx = origin->coordinates.x - c.x, dy = origin->coordinates.y - c.y;
            if ((horizontal && dy == 0 && dx != 0) || (vertical && dx == 0 && dy != 0)) return 0;
        }
        return 1;
    }
    int wet = ef_wetland(target) && (s->allowed & ef_terrain_bit(EF_WETLAND));
    if (ef_is_water_tile(target)) {
        if (wet) return 1;
        if (s->required_port
            && (!ef_profile_improvement_has(s, target, EF_AB_EMBARK) || !owned_or_peace || !s->can_embark
                || s->unit->leader != 0 || s->unit->follower != 0))
            return 0;
    }
    return (target->has_improvement_n && target->improvement.kind_n == EF_CITY) || (s->allowed & ef_terrain_bit(target->terrain_n))
           || wet;
}

/* movement.rs Profile::guarded: a visible enemy not at peace stands beside the tile */
EF_FN int ef_profile_guarded(const EfProfile *s, const EfTile *target) {
    int32_t i = target->coordinates.y * s->g->width + target->coordinates.x;
    uint8_t known = s->mover->guarded[i];
    if (known != 0) return known == 2;
    int found = 0;
    for (int32_t dx = -1; dx <= 1 && !found; dx++) {
        for (int32_t dy = -1; dy <= 1 && !found; dy++) {
            if (dx == 0 && dy == 0) continue;
            const EfTile *n = ef_tile_c(s->g, ef_xy(target->coordinates.x + dx, target->coordinates.y + dy));
            if (!n) continue;
            const EfUnit *e = ef_occupant(s->g, n);
            found = e && e->owner_n != s->owner && ef_relation(s->g, s->owner, e->owner_n) != EF_PEACE
                    && !ef_unit_effect(s->g, e, EF_INVISIBLE_EFFECT) && e->leader == 0;
        }
    }
    s->mover->guarded[i] = (uint8_t)(1 + found);
    return found;
}

EF_FN int ef_road_tile(const EfTile *t) {
    return t->has_road_n || (t->has_improvement_n && (t->improvement.kind_n == EF_CITY || t->improvement.kind_n == EF_BRIDGE));
}

/* movement.rs Profile::step_cost, in tenths of a move */
EF_FN int32_t ef_profile_step_cost(const EfProfile *s, const EfTile *target, const EfTile *from) {
    if (ef_profile_embarks(s, target) || ef_profile_disembarks(s, target)) return EF_STOP_COST;
    if (!s->slips_past && ef_profile_guarded(s, target)) return EF_STOP_COST;
    if (s->fly) return 10;
    if (s->skate) return target->terrain_n == EF_ICE ? 10 : EF_STOP_COST;
    if (target->terrain_n == EF_ICE && s->glide) return 9;
    int creep = s->creep;
    if (ef_is_water_tile(target)) {
        if (creep) return 10;
    } else {
        if (s->aquatic && !ef_flooded(target)) return EF_STOP_COST;
        if (target->terrain_n != EF_MOUNTAIN && creep) return 10;
    }
    if (ef_road_tile(from) && ef_road_tile(target) && target->terrain_n != EF_ICE && !creep && !s->aquatic) {
        /* a road needs both tiles to allow it */
        int a = from->owner_n == 0 || from->owner_n == s->owner
                || (ef_player_index(s->g, from->owner_n) != EF_NONE && ef_relation(s->g, from->owner_n, s->owner) == EF_PEACE);
        int b = target->owner_n == 0 || target->owner_n == s->owner
                || (ef_player_index(s->g, target->owner_n) != EF_NONE && ef_relation(s->g, target->owner_n, s->owner) == EF_PEACE);
        if (a && b) return 5;
    }
    if (ef_profile_improvement_has(s, target, EF_AB_SLOW)) return EF_STOP_COST;
    if (target->terrain_n == EF_MOUNTAIN || target->terrain_n == EF_FOREST) return EF_STOP_COST;
    if (s->water_bound && ef_flooded(target)) return EF_STOP_COST;
    if (!s->aquatic && ef_contains_i8(target->effects_n, target->effects_len_n, EF_ALGAE)) return EF_STOP_COST;
    return 10;
}

/* movement.rs Profile::index */
EF_FN int32_t ef_profile_index(const EfProfile *s, EfXy c) {
    int32_t w = s->g->width, h = s->g->tile_count / w;
    return (c.x >= 0 && c.y >= 0 && c.x < w && c.y < h) ? c.y * w + c.x : EF_NONE;
}

/* movement.rs Profile::open_neighbours: the neighbours the unit can step to, in order */
EF_FN int ef_open_neighbours(const EfProfile *s, int32_t from, EfXy c, int32_t out[8]) {
    int n = 0;
    if (s->allowed == 0) return 0;
    for (int32_t dx = -1; dx <= 1; dx++) {
        for (int32_t dy = -1; dy <= 1; dy++) {
            if (dx == 0 && dy == 0) continue;
            int32_t next = ef_profile_index(s, ef_xy(c.x + dx, c.y + dy));
            if (next != EF_NONE && ef_profile_accessible(s, &ef_ctiles(s->g)[from], &ef_ctiles(s->g)[next])) out[n++] = next;
        }
    }
    return n;
}

/* movement.rs Profile::path: destination first and the unit's tile last;
 * *found = 0 when there is none */
EF_BIG EfResult ef_profile_path(const EfProfile *s, EfSearch *w, EfXy destination, int32_t max_cost, EfPath *path, int *found) {
    const EfGame *g = s->g;
    EfXy start = s->unit->coordinates;
    *found = 0;
    path->len = 0;
    int32_t dest = ef_profile_index(s, destination);
    if (dest == EF_NONE) return ef_ok();
    if (ef_xy_eq(start, destination)) {
        path->tiles[0] = destination;
        path->len = 1;
        *found = 1;
        return ef_ok();
    }
    if (!ef_profile_can_finish(s, &ef_ctiles(g)[dest])) return ef_ok();
    int32_t mx = destination.x - start.x, my = destination.y - start.y;
    int32_t manhattan = (mx < 0 ? -mx : mx) + (my < 0 ? -my : my);
    w->open_len = 1;
    w->open_xy[0] = start;
    w->open_priority[0] = (float)manhattan;
    for (int i = 0; i < g->tile_count; i++) { w->cost[i] = 0; w->came_from[i] = ef_xy(0, 0); }
    for (int iteration = 0; iteration < 1000; iteration++) {
        if (w->open_len == 0) return ef_ok();
        EfXy cur = w->open_xy[0];
        for (int k = 1; k < w->open_len; k++) { w->open_xy[k - 1] = w->open_xy[k]; w->open_priority[k - 1] = w->open_priority[k]; }
        w->open_len -= 1;
        if (ef_xy_eq(cur, destination)) {
            path->tiles[0] = destination;
            path->len = 1;
            EfXy c = destination;
            for (int k = 0; k <= g->tile_count; k++) {
                int32_t ci = ef_profile_index(s, c);
                if (ci == EF_NONE) return ef_ok();
                c = w->came_from[ci];
                if (path->len >= EF_MOVE_PATH_TILES) return ef_short_list_full(EF_MOVE_PATH_TILES);
                path->tiles[path->len++] = c;
                if (ef_xy_eq(c, start)) { *found = 1; return ef_ok(); }
            }
            path->len = 0;
            return ef_ok();
        }
        int32_t ci = ef_profile_index(s, cur);
        int32_t cc = w->cost[ci];
        if (ef_under_limit(cc, max_cost)) {
            int32_t valid[8];
            int n = ef_open_neighbours(s, ci, cur, valid);
            for (int k = 0; k < n; k++) {
                int32_t next = valid[k];
                int32_t nc = cc + ef_profile_step_cost(s, &ef_ctiles(g)[next], &ef_ctiles(g)[ci]);
                if (w->cost[next] < 1 || nc < w->cost[next]) {
                    w->cost[next] = nc;
                    w->came_from[next] = cur;
                    EfXy c = ef_ctiles(g)[next].coordinates;
                    float priority = ef_move_priority(destination.x - c.x, destination.y - c.y, nc);
                    int at = w->open_len;
                    for (int q = 0; q < w->open_len; q++)
                        if (w->open_priority[q] > priority) { at = q; break; }
                    if (w->open_len >= EF_OPEN_MAX) return ef_err(EF_ERR_OPEN_LIST_FULL, 0);
                    for (int q = w->open_len; q > at; q--) { w->open_xy[q] = w->open_xy[q - 1]; w->open_priority[q] = w->open_priority[q - 1]; }
                    w->open_xy[at] = c;
                    w->open_priority[at] = priority;
                    w->open_len += 1;
                }
            }
        }
    }
    return ef_ok();
}

/* movement.rs mover_context. A board with mangrove, which the search does not model, never
 * reaches the device: engine-fast refuses it at import (Game::import). */
EF_BIG EfResult ef_mover_context(const EfGame *g, const EfTables *t, int32_t owner, EfMover *m) {
    int pi = ef_player_index(g, owner);
    if (pi == EF_NONE) return ef_err(EF_ERR_MISSING_UNIT_OWNER, 0);
    const EfPlayer *p = &g->players[pi];
    uint32_t movements = 0;
    for (int k = 0; k < p->available_tech_len; k++) {
        int32_t tech = p->available_tech[k];
        if (tech < 0 || tech >= t->tech_count) return ef_err(EF_ERR_UNKNOWN_TECH, 0);
        if (t->techs[tech].movements_error) return ef_err(EF_ERR_TABLE, t->techs[tech].movements_error - 1);
        movements |= t->techs[tech].movements;
    }
    if (movements & ef_terrain_bit(EF_OCEAN)) movements |= ef_terrain_bit(EF_WATER);
    const EfTechTree *tree = ef_tribe_tree(t, p->tribe);
    if (!tree) return ef_err(EF_ERR_UNKNOWN_TRIBE, 0);
    int can_embark = 0, glide = 0;
    for (int k = 0; k < p->available_tech_len; k++) {
        int32_t tech = p->available_tech[k];
        if (tech < 0 || tech >= t->tech_count) return ef_err(EF_ERR_UNKNOWN_TECH, 0);
        if (tree->links[tech].embark_unlock_error) return ef_err(EF_ERR_TABLE, tree->links[tech].embark_unlock_error - 1);
        can_embark |= tree->links[tech].embark_unlock;
    }
    for (int k = 0; k < p->available_tech_len; k++) {
        const EfTechData *d = ef_tech_data(t, p->available_tech[k]);
        if (d && ef_has(d->abilities, EF_AB_GLIDE)) glide = 1;
    }
    m->movements = movements;
    m->can_embark = can_embark;
    m->glide = glide;
    m->pad = 0;
    memset(m->guarded, 0, sizeof m->guarded);
    return ef_ok();
}

/* a unit's movement with its effects: the move search's range and the unit panel's figure
 * (movement.rs profile_with, queries.rs panel) */
EF_FN int32_t ef_unit_movement(const EfEffectList *fx, const EfUnitData *d) {
    int32_t r = d->movement + ef_has_effect(fx, EF_BOOSTED);
    if (ef_has_effect(fx, EF_SWIFT) || ef_has_effect(fx, EF_BUBBLE)) r += 1;
    if (ef_has_effect(fx, EF_POISONED) && r > 1) r -= 1;
    return r;
}

/* movement.rs profile_with: the unit's range and search profile (the unit profile, which
 * does not depend on the range; *range = 0 when it cannot move) */
EF_BIG EfResult ef_profile_with(const EfGame *g, const EfTables *t, const EfUnit *unit, EfMover *mover, EfProfile *s, int32_t *range) {
    const EfEffectList *fx = ef_effect_list(g, unit->effects_n);
    const EfUnitData *d = ef_unit_data(t, unit->kind_n);
    if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    *range = ef_unit_movement(fx, d);
    if (d->movement_terrain_error) return ef_err(EF_ERR_TABLE, d->movement_terrain_error - 1);
    uint32_t allowed;
    if (!d->has_movement_terrain) allowed = mover->movements;
    else if (ef_has(d->abilities, EF_AB_NAVIGATE)) allowed = d->movement_terrain;
    else allowed = d->movement_terrain & mover->movements;
    int aquatic = ef_has(d->abilities, EF_AB_SWIM), fly = ef_has(d->abilities, EF_AB_FLY);
    if (fly) {
        /* allowed terrain: every catalog terrain but None */
        allowed = 0;
        for (int32_t k = 1; k < t->terrain_count && k < EF_MAX_TERRAINS; k++)
            if (t->terrain_known[k]) allowed |= ef_terrain_bit(k);
    } else if (aquatic) {
        allowed |= ef_terrain_bit(EF_WATER);
    }
    int hide = ef_has(d->abilities, EF_AB_HIDE);
    s->g = g;
    s->t = t;
    s->unit = unit;
    s->mover = mover;
    s->owner = unit->owner_n;
    s->slips_past = ef_has(d->abilities, EF_AB_SNEAK) || hide || ef_has_effect(fx, EF_INVISIBLE_EFFECT);
    s->hide = hide;
    s->creep = ef_has(d->abilities, EF_AB_CREEP);
    s->aquatic = aquatic;
    s->carry = ef_has(d->abilities, EF_AB_CARRY);
    s->infiltrate = ef_has(d->abilities, EF_AB_INFILTRATE);
    s->fly = fly;
    s->skate = ef_has(d->abilities, EF_AB_SKATE);
    s->water_bound = aquatic && (s->carry || d->has_movement_terrain);
    s->allowed = allowed;
    s->required_port = !aquatic && !fly;
    s->can_embark = mover->can_embark;
    s->glide = mover->glide;
    s->allow_occupied = 0;
    return ef_ok();
}

/* The calling thread's scratch of type T: a search's arrays are too large for a GPU thread's
 * stack, which the runtime reserves for every thread that can be resident. On the device one
 * region per thread holds whichever of the three kinds (EfSearch, EfRouteWork, EfConnectWork)
 * is in use, so no two may be live at once; the host build keeps a thread-local one of each
 * and counts every take while another is held (ef_twin_scratch_overlaps), which the tests and
  * the long runs require to stay zero. Every EF_SCRATCH_TAKE is followed by EF_SCRATCH_GIVE
 * on the same path, with no return between them. */
#ifdef __CUDA_ARCH__
#define EF_SCRATCH_TAKE(T, t, field) \
    ((T *)(void *)((char *)(uintptr_t)(t)->scratch + (size_t)(blockIdx.x * blockDim.x + threadIdx.x) * (size_t)(t)->scratch_size))
#define EF_SCRATCH_GIVE() ((void)0)
#else
static EF_THREAD_LOCAL int ef_tls_scratch_busy;
static int64_t ef_scratch_overlaps;
static inline void ef_scratch_take(void) {
    if (ef_tls_scratch_busy) __atomic_fetch_add(&ef_scratch_overlaps, 1, __ATOMIC_RELAXED);
    ef_tls_scratch_busy = 1;
}
#define EF_SCRATCH_TAKE(T, t, field) ((void)(t), ef_scratch_take(), &ef_tls_##field)
#define EF_SCRATCH_GIVE() (ef_tls_scratch_busy = 0)
static EF_THREAD_LOCAL EfSearch ef_tls_search_scratch;
#endif

/* ef_profile_path in the calling thread's scratch */
EF_FN EfResult ef_scratch_path(const EfTables *t, const EfProfile *s, EfXy to, int32_t max_cost, EfPath *path, int *found) {
    EfSearch *w = EF_SCRATCH_TAKE(EfSearch, t, search_scratch);
    EfResult r = ef_profile_path(s, w, to, max_cost, path, found);
    EF_SCRATCH_GIVE();
    return r;
}

/* movement.rs unit_path: the game path for a Move, destination first; *found = 0 for none */
EF_FN EfResult ef_unit_path(const EfGame *g, const EfTables *t, const EfUnit *unit, EfXy to, EfPath *path, int *found) {
    EfMover mover;
    EF_TRY(ef_mover_context(g, t, unit->owner_n, &mover));
    EfProfile s;
    int32_t range;
    EF_TRY(ef_profile_with(g, t, unit, &mover, &s, &range));
    *found = 0;
    path->len = 0;
    if (range == 0) return ef_ok();
    return ef_scratch_path(t, &s, to, range, path, found);
}

#endif /* EF_MOVEMENT_H */
