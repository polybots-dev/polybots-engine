/* routes.h - port of engine-fast/src/routes.rs: UpdateRoutes (the router path search),
 * UpdateCityConnections (the connected-cities search, with its bound on the game's
 * queue and the replay when the bound is not enough) and the CityConnection actions. */
#ifndef EF_ROUTES_H
#define EF_ROUTES_H

#include "moves.h"

#define EF_NETWORK_TASK EF_TASK_NETWORK
#define EF_DEQUEUE_CAP 10000u
#define EF_ROUTE_OPEN_MAX 1024
#define EF_CONNECTION_QUEUE 16384
#define EF_UNREACHED 0xffffu

/* routes.rs router: *has = 0 when the kind has none (outside the catalog) */
EF_FN EfResult ef_router(const EfTables *t, int32_t kind, int *has, const EfImprovementData **d) {
    *has = 0;
    if (!ef_improvement_known(t, kind)) return ef_ok();
    const EfImprovementData *x = &t->improvements[kind];
    if (x->router_error) return ef_err(EF_ERR_TABLE, x->router_error - 1);
    *has = 1;
    *d = x;
    return ef_ok();
}

EF_FN int ef_has_terrain(uint32_t terrains, int32_t id) { return (ef_terrain_bit(id) & terrains) != 0; }

/* routes.rs movements: the terrains p's techs unlock, from the refreshed cache */
EF_FN EfResult ef_route_movements(EfGame *g, const EfTables *t, int32_t p, uint32_t *out) {
    EF_TRY(ef_refresh_tech_cache(g, t, p));
    int i;
    EF_TRY(ef_player(g, p, &i));
    const EfPlayer *x = &g->players[i];
    if (!x->has_tech_cache) return ef_err(EF_ERR_NULL_TECH_CACHE, 0);
    uint32_t terrains = 0;
    for (int k = 0; k < x->tech_cache_len; k++) {
        int32_t tech = x->tech_cache[k];
        if (tech < 0 || tech >= t->tech_count) return ef_err(EF_ERR_UNKNOWN_TECH, 0);
        if (t->techs[tech].movements_error) return ef_err(EF_ERR_TABLE, t->techs[tech].movements_error - 1);
        terrains |= t->techs[tech].movements;
    }
    if (terrains & ef_terrain_bit(EF_OCEAN)) terrains |= ef_terrain_bit(EF_WATER);
    *out = terrains;
    return ef_ok();
}

EF_FN int ef_water_terrain(int32_t terrain) { return terrain == EF_WATER || terrain == EF_OCEAN; }

/* routes.rs accessible */
EF_BIG EfResult ef_route_accessible(EfGame *g, const EfTables *t, int32_t p, uint32_t allowed, int32_t from, int32_t to, int *out) {
    *out = 0;
    int32_t owner = ef_ctiles(g)[to].owner_n;
    int32_t r = -1;
    if (owner != 0) EF_TRY(ef_relation_state_of(g, p, owner, &r));
    int owned_or_peace = owner == p || (owner != 0 && r == EF_PEACE);
    if (owner != 0 && owner != p && !owned_or_peace) return ef_ok();
    if (!(p == EF_NATURE || ef_tile_explored(&ef_ctiles(g)[to], p))) return ef_ok();
    const EfTile *target = &ef_ctiles(g)[to];
    int has_kind = target->has_improvement_n;
    int32_t kind = target->improvement.kind_n;
    if (has_kind && ef_improvement_has(t, kind, EF_AB_BRIDGE)) {
        if (kind == EF_BRIDGE && !ef_has_terrain(allowed, target->terrain_n)) {
            EfXy c = target->coordinates;
            int32_t n;
#define EF_RANCHOR(dx, dy) ((n = ef_tile_index(g, ef_xy(c.x + (dx), c.y + (dy)))) != EF_NONE && !ef_water_terrain(ef_ctiles(g)[n].terrain_n))
            int h = 0, v = 0;
            if (EF_RANCHOR(0, -1) && EF_RANCHOR(0, 1)) h = 1;
            else if (EF_RANCHOR(-1, 0) && EF_RANCHOR(1, 0)) v = 1;
#undef EF_RANCHOR
            int32_t dx = ef_ctiles(g)[from].coordinates.x - c.x, dy = ef_ctiles(g)[from].coordinates.y - c.y;
            if ((h && dy == 0 && dx != 0) || (v && dx == 0 && dy != 0)) return ef_ok();
        }
        *out = 1;
        return ef_ok();
    }
    int32_t terrain = target->terrain_n, from_terrain = ef_ctiles(g)[from].terrain_n;
    int wet = ef_wetland(target) && ef_has_terrain(allowed, EF_WETLAND);
    if (ef_water_terrain(terrain) && wet) {
        *out = 1;
        return ef_ok();
    }
    if (ef_water_terrain(terrain) && (from_terrain == EF_FIELD || from_terrain == EF_MOUNTAIN || from_terrain == EF_FOREST || from_terrain == EF_ICE)
        && !ef_wetland(&ef_ctiles(g)[from])) {
        int embark = has_kind && ef_improvement_has(t, kind, EF_AB_EMBARK);
        if (!embark || !owned_or_peace) return ef_ok();
        int can;
        EF_TRY(ef_can_embark(g, t, p, &can));
        if (!can) return ef_ok();
    }
    *out = (has_kind && kind == EF_CITY) || ef_has_terrain(allowed, terrain) || wet;
    return ef_ok();
}

/* the route searches' working arrays */
typedef struct {
    int32_t costs[EF_MAX_TILES], came_from[EF_MAX_TILES];
    int32_t path[EF_MAX_TILES + 2], path_len;
    int32_t open_len;
    int32_t open_tile[EF_ROUTE_OPEN_MAX];
    float open_priority[EF_ROUTE_OPEN_MAX];
    int32_t routers[EF_MAX_TILES], cities[EF_MAX_TILES], old[EF_MAX_TILES];
} EfRouteWork;

/* routes.rs router_path: whether there is a path; it is left in w->path, destination first */
EF_BIG EfResult ef_router_path(EfGame *g, const EfTables *t, int32_t p, int32_t start, int32_t dest, uint32_t allowed,
                              int32_t max_cost, EfRouteWork *w, int *found) {
    *found = 0;
    w->path_len = 1;
    w->path[0] = dest;
    if (start == dest) { *found = 1; return ef_ok(); }
    if (ef_ctiles(g)[dest].has_improvement_n && ef_ctiles(g)[dest].improvement.kind_n == EF_CITY && ef_ctiles(g)[dest].owner_n != 0) {
        int32_t r;
        EF_TRY(ef_relation_state_of(g, p, ef_ctiles(g)[dest].owner_n, &r));
        if (r == EF_PEACE || r == EF_COOL_DOWN) return ef_ok();
    }
    EfXy s = ef_ctiles(g)[start].coordinates, d = ef_ctiles(g)[dest].coordinates;
    int32_t mx = d.x - s.x, my = d.y - s.y;
    w->open_len = 1;
    w->open_tile[0] = start;
    w->open_priority[0] = (float)((mx < 0 ? -mx : mx) + (my < 0 ? -my : my));
    for (int i = 0; i < g->tile_count; i++) { w->costs[i] = 0; w->came_from[i] = 0; }
    for (int iteration = 0; iteration < 1000; iteration++) {
        if (w->open_len == 0) return ef_ok();
        int32_t current = w->open_tile[0];
        for (int k = 1; k < w->open_len; k++) { w->open_tile[k - 1] = w->open_tile[k]; w->open_priority[k - 1] = w->open_priority[k]; }
        w->open_len -= 1;
        if (current == dest) {
            int32_t c = dest;
            for (int k = 0; k <= g->tile_count; k++) {
                c = w->came_from[c];
                w->path[w->path_len++] = c;
                if (c == start) { *found = 1; return ef_ok(); }
            }
            return ef_err(EF_ERR_ROUTER_CHAIN, 0);
        }
        int32_t cost = w->costs[current];
        if (ef_under_limit(cost, max_cost) && allowed != 0) {
            EfXy cc = ef_ctiles(g)[current].coordinates;
            for (int32_t dx = -1; dx <= 1; dx++) {
                for (int32_t dy = -1; dy <= 1; dy++) {
                    if (dx == 0 && dy == 0) continue;
                    int32_t next = ef_tile_index(g, ef_xy(cc.x + dx, cc.y + dy));
                    if (next == EF_NONE) continue;
                    int ok;
                    EF_TRY(ef_route_accessible(g, t, p, allowed, current, next, &ok));
                    if (!ok) continue;
                    const EfTile *n = &ef_ctiles(g)[next];
                    int32_t step = (n->has_road_n || n->has_route_n
                                    || (n->has_improvement_n && (n->improvement.kind_n == EF_CITY || n->improvement.kind_n == EF_BRIDGE)))
                                       ? 5 : 10;
                    int32_t new_cost = ef_wadd(cost, step);
                    if (w->costs[next] < 1 || new_cost < w->costs[next]) {
                        w->costs[next] = new_cost;
                        w->came_from[next] = current;
                        EfXy nc = n->coordinates;
                        float priority = ef_route_priority(d.x - nc.x, d.y - nc.y, new_cost);
                        int at = w->open_len;
                        for (int q = 0; q < w->open_len; q++)
                            if (w->open_priority[q] > priority) { at = q; break; }
                        if (w->open_len >= EF_ROUTE_OPEN_MAX) return ef_err(EF_ERR_ROUTE_OPEN_FULL, 0);
                        for (int q = w->open_len; q > at; q--) { w->open_tile[q] = w->open_tile[q - 1]; w->open_priority[q] = w->open_priority[q - 1]; }
                        w->open_tile[at] = next;
                        w->open_priority[at] = priority;
                        w->open_len += 1;
                    }
                }
            }
        }
    }
    return ef_ok();
}

/* routes.rs find_path_between */
EF_FN EfResult ef_find_path_between(EfGame *g, const EfTables *t, int32_t p, int32_t origin, int32_t dest, uint32_t moves, EfRouteWork *w) {
    int32_t ok = ef_ctiles(g)[origin].improvement.kind_n, dk = ef_ctiles(g)[dest].improvement.kind_n;
    int has_o, has_d;
    const EfImprovementData *o = 0, *dd = 0;
    EF_TRY(ef_router(t, ok, &has_o, &o));
    EF_TRY(ef_router(t, dk, &has_d, &dd));
    if (!has_o || !has_d) return ef_ok();
    EfXy a = ef_ctiles(g)[origin].coordinates, b = ef_ctiles(g)[dest].coordinates;
    int32_t ax = a.x - b.x, ay = a.y - b.y;
    ax = ax < 0 ? -ax : ax;
    ay = ay < 0 ? -ay : ay;
    if ((ax > ay ? ax : ay) > o->router_range) return ef_ok();
    uint32_t allowed = dk == EF_CITY ? o->router_terrain : (o->router_terrain & dd->router_terrain & moves);
    if (allowed == 0) return ef_ok();
    int found;
    EF_TRY(ef_router_path(g, t, p, origin, dest, allowed, o->router_range, w, &found));
    if (found)
        for (int k = 0; k < w->path_len; k++) ef_tiles(g)[w->path[k]].has_route_n = 1; /* narrow: a flag */
    return ef_ok();
}

/* routes.rs update_routes */
#ifndef __CUDA_ARCH__
static EF_THREAD_LOCAL EfRouteWork ef_tls_route_scratch;
#endif

EF_BIG EfResult ef_update_routes(EfGame *g, const EfTables *t, EfRouteWork *w) {
    int32_t olds = 0;
    for (int i = 0; i < g->tile_count; i++) {
        if (ef_ctiles(g)[i].has_route_n) {
            ef_tiles(g)[i].has_route_n = 0; /* narrow: a flag */
            ef_tiles(g)[i].had_route_n = 1; /* narrow: a flag */
            w->old[olds++] = i;
        }
    }
    for (int k = 0; k < g->player_count; k++) {
        int32_t p = g->players[k].id;
        if (p == EF_NATURE) continue;
        uint32_t moves;
        EF_TRY(ef_route_movements(g, t, p, &moves));
        int32_t nr = 0, nc = 0;
        for (int i = 0; i < g->tile_count; i++) {
            const EfTile *x = &ef_ctiles(g)[i];
            if (x->owner_n != p || !x->has_improvement_n) continue;
            int has;
            const EfImprovementData *r = 0;
            EF_TRY(ef_router(t, x->improvement.kind_n, &has, &r));
            if (!has) continue;
            if (r->router_terrain != 0) w->routers[nr++] = i;
            if (x->improvement.kind_n == EF_CITY) w->cities[nc++] = i;
        }
        for (int n = 0; n < nr; n++) {
            int32_t origin = w->routers[n];
            int has;
            const EfImprovementData *r = 0;
            EF_TRY(ef_router(t, ef_ctiles(g)[origin].improvement.kind_n, &has, &r));
            if (!has) continue;
            for (int m = n + 1; m < nr; m++) EF_TRY(ef_find_path_between(g, t, p, origin, w->routers[m], moves, w));
            if (r->router_connects)
                for (int m = 0; m < nc; m++) EF_TRY(ef_find_path_between(g, t, p, origin, w->cities[m], moves, w));
        }
    }
    for (int k = 0; k < olds; k++) ef_tiles(g)[w->old[k]].had_route_n = 0; /* narrow: a flag */
    return ef_ok();
}

/* the connected-cities search's working arrays */
typedef struct {
    uint16_t distance[EF_MAX_TILES];
    uint16_t lists[EF_MAX_TILES][8];
    uint8_t lens[EF_MAX_TILES];
    uint8_t connectable[EF_MAX_TILES];   /* 0 not asked yet, 1 no, 2 yes */
    uint8_t visited[EF_MAX_TILES], waiting[EF_MAX_TILES];
    int32_t order[EF_MAX_TILES], order_len;
    uint32_t count[EF_MAX_TILES], queued[EF_MAX_TILES];
    uint16_t queue[EF_CONNECTION_QUEUE];
} EfConnectWork;

/* routes.rs connectable */
EF_FN EfResult ef_connectable(const EfGame *g, const EfTables *t, int32_t i, int *out) {
    const EfTile *x = &ef_ctiles(g)[i];
    *out = 0;
    if (x->has_road_n) { *out = 1; return ef_ok(); }
    if (x->has_improvement_n) {
        int has;
        const EfImprovementData *r = 0;
        EF_TRY(ef_router(t, x->improvement.kind_n, &has, &r));
        if (has) *out = x->improvement.kind_n == EF_CITY || r->router_terrain != 0;
    }
    return ef_ok();
}

/* routes.rs owners_allow */
EF_FN int ef_owners_allow(const EfGame *g, int32_t from, int32_t to) {
    int32_t fo = ef_ctiles(g)[from].owner_n, to_owner = ef_ctiles(g)[to].owner_n;
    return to_owner == 0 || to_owner == fo || ef_player_index(g, fo) == EF_NONE || ef_relation(g, fo, to_owner) == EF_PEACE;
}

/* routes.rs Links::open */
EF_FN EfResult ef_link_open(const EfGame *g, const EfTables *t, EfConnectWork *w, int32_t from, int32_t to, int *out) {
    *out = 0;
    if (!ef_owners_allow(g, from, to)) return ef_ok();
    int32_t both[2] = { from, to };
    int all = 1;
    for (int k = 0; k < 2 && all; k++) {
        int32_t i = both[k];
        if (w->connectable[i] == 0) {
            int c;
            EF_TRY(ef_connectable(g, t, i, &c));
            w->connectable[i] = (uint8_t)(1 + c);
        }
        all = w->connectable[i] == 2;
    }
    if (all) { *out = 1; return ef_ok(); }
    *out = ef_ctiles(g)[from].has_route_n && ef_ctiles(g)[to].has_route_n;
    return ef_ok();
}

/* routes.rs dequeues_at_most: an upper bound on the dequeues of the game's queue */
EF_FN uint32_t ef_dequeues_at_most(EfConnectWork *w, int32_t tiles) {
    for (int i = 0; i < tiles; i++) { w->count[i] = 0; w->queued[i] = 0; }
    w->count[w->order[0]] = 1;
    uint32_t total = 1;
    int32_t from = 0;
    for (uint32_t generation = 0;; generation++) {
        int any = 0;
        for (int32_t i = from; i < w->order_len; i++) {
            int32_t tile = w->order[i];
            if (w->distance[tile] > generation) break;
            if (w->count[tile] == 0) continue;
            for (int k = 0; k < w->lens[tile]; k++) {
                int32_t n = w->lists[tile][k];
                if (w->distance[n] == generation || w->distance[n] == generation + 1) {
                    uint32_t sum = w->queued[n] + w->count[tile];
                    w->queued[n] = sum < w->queued[n] ? 0xffffffffu : sum;
                    any = 1;
                }
            }
        }
        if (!any) break;
        while (from < w->order_len && w->distance[w->order[from]] < generation) from += 1;
        for (int32_t i = from; i < w->order_len; i++) {
            int32_t tile = w->order[i];
            if (w->distance[tile] > generation + 1) break;
            w->count[tile] = w->queued[tile];
            w->queued[tile] = 0;
            uint32_t sum = total + w->count[tile];
            total = sum < total ? 0xffffffffu : sum;
        }
        if (total > EF_DEQUEUE_CAP) break;
    }
    return total;
}

/* routes.rs replay: the game's queue, entry by entry, up to its cap */
EF_FN EfResult ef_replay(EfConnectWork *w, int32_t tiles, int32_t start) {
    for (int i = 0; i < tiles; i++) { w->visited[i] = 0; w->waiting[i] = 0; }
    w->waiting[start] = 1;
    int32_t unvisited_waiting = 1;
    int32_t head = 0, tail = 0, size = 0;
    w->queue[tail] = (uint16_t)start; tail = (tail + 1) % EF_CONNECTION_QUEUE; size = 1;
    for (uint32_t k = 0; k < EF_DEQUEUE_CAP; k++) {
        if (size == 0) break;
        int32_t i = w->queue[head]; head = (head + 1) % EF_CONNECTION_QUEUE; size -= 1;
        if (!w->visited[i]) { w->visited[i] = 1; unvisited_waiting -= 1; }
        for (int q = 0; q < w->lens[i]; q++) {
            int32_t n = w->lists[i][q];
            if (!w->visited[n]) {
                if (size >= EF_CONNECTION_QUEUE) return ef_err(EF_ERR_CONNECTION_QUEUE_FULL, 0);
                w->queue[tail] = (uint16_t)n; tail = (tail + 1) % EF_CONNECTION_QUEUE; size += 1;
                if (!w->waiting[n]) { w->waiting[n] = 1; unvisited_waiting += 1; }
            }
        }
        if (unvisited_waiting == 0) break;
    }
    return ef_ok();
}

/* routes.rs connected: w->visited marks the tiles the search
 * from p's capital visits; *start = EF_NONE when the player has no capital to search from */
EF_BIG EfResult ef_connected(const EfGame *g, const EfTables *t, int32_t p, EfConnectWork *w, int32_t *start) {
    *start = EF_NONE;
    if (p == EF_NATURE) return ef_ok();
    int pi = ef_player_index(g, p);
    if (pi == EF_NONE) return ef_ok();
    int32_t s = ef_tile_index(g, g->players[pi].start_tile);
    if (s == EF_NONE) return ef_err(EF_ERR_START_TILE, 0);
    if (ef_ctiles(g)[s].owner_n != p) return ef_ok();
    int32_t tiles = g->tile_count;
    for (int i = 0; i < tiles; i++) { w->distance[i] = EF_UNREACHED; w->lens[i] = 0; w->connectable[i] = 0; }
    w->order[0] = s;
    w->order_len = 1;
    w->distance[s] = 0;
    for (int32_t next = 0; next < w->order_len; next++) {
        int32_t i = w->order[next];
        EfXy c = ef_ctiles(g)[i].coordinates;
        uint8_t len = 0;
        for (int32_t dx = -1; dx <= 1; dx++) {
            for (int32_t dy = -1; dy <= 1; dy++) {
                if (dx == 0 && dy == 0) continue;
                int32_t n = ef_tile_index(g, ef_xy(c.x + dx, c.y + dy));
                if (n == EF_NONE) continue;
                int open;
                EF_TRY(ef_link_open(g, t, w, i, n, &open));
                if (open) w->lists[i][len++] = (uint16_t)n;
            }
        }
        w->lens[i] = len;
        for (int k = 0; k < len; k++) {
            int32_t n = w->lists[i][k];
            if (w->distance[n] == EF_UNREACHED) {
                w->distance[n] = (uint16_t)(w->distance[i] + 1);
                w->order[w->order_len++] = n;
            }
        }
    }
    if (ef_dequeues_at_most(w, tiles) <= EF_DEQUEUE_CAP) {
        for (int i = 0; i < tiles; i++) w->visited[i] = w->distance[i] != EF_UNREACHED;
    } else {
        EF_TRY(ef_replay(w, tiles, s));
    }
    *start = s;
    return ef_ok();
}

#ifndef __CUDA_ARCH__
static EF_THREAD_LOCAL EfConnectWork ef_tls_connect_scratch;
#endif

/* routes.rs update_city_connections */
EF_BIG EfResult ef_update_city_connections(EfGame *g, const EfTables *t, const int32_t *players, int32_t count, EfConnectWork *w) {
    for (int k = 0; k < count; k++) {
        int32_t p = players[k];
        if (p == 0) continue;
        int pi;
        EF_TRY(ef_player(g, p, &pi));
        int32_t start;
        EF_TRY(ef_connected(g, t, p, w, &start));
        for (int i = 0; i < g->tile_count; i++) {
            const EfTile *x = &ef_ctiles(g)[i];
            if (x->owner_n != p || !x->has_improvement_n || x->improvement.kind_n != EF_CITY) continue;
            int32_t to = x->improvement.connected_to_capital_of_player_n;
            int reached = start != EF_NONE && start != i && w->visited[i];
            int32_t kind = -1;
            if (reached) {
                if (to == 0) kind = EF_A_CONNECT;
                else if (to != p) kind = EF_A_CHANGE;
            } else if (to != 0) {
                kind = EF_A_DISCONNECT;
            }
            if (kind >= 0) EF_TRY(ef_push(g, ef_action_at(kind, p, x->coordinates)));
        }
    }
    return ef_ok();
}

/* routes.rs start_tile */
EF_FN EfResult ef_start_tile(const EfGame *g, int32_t p, EfXy *out) {
    int i = ef_player_index(g, p);
    if (i == EF_NONE) return ef_err(EF_ERR_MISSING_PLAYER_ID, p);
    *out = g->players[i].start_tile;
    return ef_ok();
}

EF_FN EfAction ef_population(int32_t kind, int32_t player, EfXy source, EfXy target) {
    EfAction a = ef_action(kind, player);
    if (kind == EF_A_INCREASE_POPULATION) { a.p[0] = source; a.p[1] = target; }
    else a.p[0] = target;
    a.i[0] = 60;
    return a;
}

/* routes.rs city_connection (the CityConnection actions) */
EF_BIG EfResult ef_city_connection(EfGame *g, const EfTables *t, int32_t effect, int32_t p, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_CONNECTION_TILE, 0);
    if (!ef_ctiles(g)[i].has_improvement_n) return effect == EF_A_CHANGE ? ef_err(EF_ERR_CONNECTION_IMPROVEMENT, 0) : ef_ok();
    int32_t to = ef_ctiles(g)[i].improvement.connected_to_capital_of_player_n;
    if ((effect == EF_A_CONNECT && to == p) || (effect == EF_A_DISCONNECT && to == 0)) return ef_ok();
    int pi = ef_player_index(g, p);
    if (effect == EF_A_CONNECT) {
        EfXy start;
        EF_TRY(ef_start_tile(g, p, &start));
        if (pi == EF_NONE) return ef_err(EF_ERR_MISSING_PLAYER_ID, p);
        if (ef_task_slot(&g->players[pi], EF_NETWORK_TASK) < 0) {
            EfAction e = ef_action(EF_A_ENABLE_TASK, p);
            e.i[0] = EF_NETWORK_TASK;
            EF_TRY(ef_push(g, e));
        }
        EF_TRY(ef_push(g, ef_population(EF_A_INCREASE_POPULATION, p, start, start)));
        EF_TRY(ef_push(g, ef_population(EF_A_INCREASE_POPULATION, p, c, c)));
        EF_PUT(ef_tiles(g)[i].improvement.connected_to_capital_of_player_n, p, uint8_t);
        return ef_check_task(g, t, pi, EF_NETWORK_TASK);
    }
    if (effect == EF_A_CHANGE) {
        EfXy start, old;
        EF_TRY(ef_start_tile(g, p, &start));
        EF_TRY(ef_start_tile(g, to, &old));
        EF_TRY(ef_push(g, ef_population(EF_A_INCREASE_POPULATION, p, start, start)));
        EF_TRY(ef_push(g, ef_population(EF_A_DECREASE_POPULATION, p, old, old)));
        EF_PUT(ef_tiles(g)[i].improvement.connected_to_capital_of_player_n, p, uint8_t);
        if (pi == EF_NONE) return ef_err(EF_ERR_MISSING_PLAYER_ID, p);
        return ef_check_task(g, t, pi, EF_NETWORK_TASK);
    }
    EfXy old;
    EF_TRY(ef_start_tile(g, to, &old));
    EF_TRY(ef_push(g, ef_population(EF_A_DECREASE_POPULATION, p, old, old)));
    EF_TRY(ef_push(g, ef_population(EF_A_DECREASE_POPULATION, p, c, c)));
    ef_tiles(g)[i].improvement.connected_to_capital_of_player_n = 0; /* narrow: none */
    return ef_ok();
}

#endif /* EF_ROUTES_H */
