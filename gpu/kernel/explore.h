/* explore.h - port of engine-fast/src/explore.rs: sorted areas (an introsort), exploring from
  * a tile, the Explore and Meet actions and the lighthouse corners. */
#ifndef EF_EXPLORE_H
#define EF_EXPLORE_H

#include "turns.h"

#define EF_MOUNTAIN 4
#define EF_LIGHTHOUSE_TASK EF_TASK_EXPLORE_LIGHTHOUSES
#define EF_AREA_MAX 121

typedef struct { float key; int32_t slot; } EfSlot;
typedef struct { int32_t len; int32_t tiles[EF_AREA_MAX]; } EfArea;

EF_FN void ef_swap_slots(EfSlot *p, int a, int b) { EfSlot x = p[a]; p[a] = p[b]; p[b] = x; }
EF_FN void ef_swap_greater(EfSlot *p, int a, int b) {
    if (a != b && p[a].key > p[b].key) ef_swap_slots(p, a, b);
}

EF_FN int ef_partition(EfSlot *p, int lo, int hi) {
    int mid = lo + (hi - lo) / 2;
    ef_swap_greater(p, lo, mid);
    ef_swap_greater(p, lo, hi);
    ef_swap_greater(p, mid, hi);
    float pivot = p[mid].key;
    ef_swap_slots(p, mid, hi - 1);
    int l = lo, r = hi - 1;
    for (;;) {
        l += 1;
        while (p[l].key < pivot) l += 1;
        r -= 1;
        while (pivot < p[r].key) r -= 1;
        if (l >= r) break;
        ef_swap_slots(p, l, r);
    }
    ef_swap_slots(p, l, hi - 1);
    return l;
}

EF_FN void ef_heap_down(EfSlot *p, int i, int n, int lo) {
    EfSlot saved = p[lo + i - 1];
    while (i <= n / 2) {
        int child = 2 * i;
        if (child < n && p[lo + child - 1].key < p[lo + child].key) child += 1;
        if (saved.key >= p[lo + child - 1].key) break;
        p[lo + i - 1] = p[lo + child - 1];
        i = child;
    }
    p[lo + i - 1] = saved;
}

EF_FN void ef_heap_sort(EfSlot *p, int lo, int hi) {
    int n = hi - lo + 1;
    for (int i = n / 2; i >= 1; i--) ef_heap_down(p, i, n, lo);
    for (int i = n; i >= 2; i--) {
        ef_swap_slots(p, lo, lo + i - 1);
        ef_heap_down(p, 1, i - 1, lo);
    }
}

/* explore.rs intro: the recursion on the right part is a loop over an explicit stack here
 * (the same order of work: the right part is finished before the left one continues) */
EF_FN void ef_intro(EfSlot *p, int lo0, int hi0, int depth0) {
    int los[32], his[32], depths[32], top = 0;
    los[0] = lo0; his[0] = hi0; depths[0] = depth0; top = 1;
    while (top > 0) {
        top -= 1;
        int lo = los[top], hi = his[top], depth = depths[top];
        while (hi > lo) {
            depth -= 1;
            int n = hi - lo + 1;
            if (n <= 16) {
                if (n == 2) {
                    ef_swap_greater(p, lo, hi);
                } else if (n == 3) {
                    ef_swap_greater(p, lo, hi - 1);
                    ef_swap_greater(p, lo, hi);
                    ef_swap_greater(p, hi - 1, hi);
                } else {
                    for (int i = lo + 1; i <= hi; i++) {
                        EfSlot saved = p[i];
                        int at = i;
                        while (at > lo && saved.key < p[at - 1].key) { p[at] = p[at - 1]; at -= 1; }
                        p[at] = saved;
                    }
                }
                break;
            }
            if (depth == -1) { ef_heap_sort(p, lo, hi); break; }
            int pivot = ef_partition(p, lo, hi);
            /* intro(p, pivot + 1, hi, depth) runs before the loop goes on with [lo, pivot - 1]:
             * the left part waits on the stack under the right one */
            los[top] = lo; his[top] = pivot - 1; depths[top] = depth; top += 1;
            lo = pivot + 1;
            /* the right part continues in this loop; the saved left part resumes after it */
        }
    }
}

/* explore.rs sort_slots */
EF_FN void ef_sort_slots(EfSlot *p, int n) {
    if (n > 1) {
        int bits = 0;
        for (uint32_t v = (uint32_t)n; v; v >>= 1) bits++;
        ef_intro(p, 0, n - 1, bits * 2);
    }
}

/* explore.rs area_sorted: the square of `radius` around c by Manhattan distance */
EF_BIG EfResult ef_area_sorted(const EfGame *g, EfXy c, int32_t radius, EfArea *area) {
    if (ef_tile_index(g, c) == EF_NONE) return ef_err(EF_ERR_AREA_CENTRE, 0);
    area->len = 0;
    if (radius < 0) return ef_ok();
    if (radius > 5) return ef_err(EF_ERR_AREA_RADIUS, 0);
    int32_t h = g->tile_count / g->width;
    EfSlot pairs[EF_AREA_MAX];
    int n = 0;
    int32_t x0 = c.x - radius < 0 ? 0 : c.x - radius, x1 = c.x + radius > g->width - 1 ? g->width - 1 : c.x + radius;
    int32_t y0 = c.y - radius < 0 ? 0 : c.y - radius, y1 = c.y + radius > h - 1 ? h - 1 : c.y + radius;
    for (int32_t x = x0; x <= x1; x++) {
        for (int32_t y = y0; y <= y1; y++) {
            int32_t dx = x - c.x, dy = y - c.y;
            pairs[n].key = (float)((dx < 0 ? -dx : dx) + (dy < 0 ? -dy : dy));
            pairs[n].slot = ef_tile_index(g, ef_xy(x, y));
            n++;
        }
    }
    ef_sort_slots(pairs, n);
    for (int k = 0; k < n; k++) area->tiles[k] = pairs[k].slot;
    area->len = n;
    return ef_ok();
}

EF_FN int ef_explored(const EfGame *g, int i, int32_t p) {
    return p == EF_NATURE || ef_contains_u8(ef_ctiles(g)[i].explorers_n, ef_ctiles(g)[i].explorers_len_n, p);
}

/* explore.rs explore_from: one Explore per unexplored tile in sight, far to near */
EF_BIG EfResult ef_explore_from(EfGame *g, int32_t p, EfXy c, int32_t sight) {
    int pi;
    EF_TRY(ef_player(g, p, &pi));
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_EXPLORE_ORIGIN, 0);
    int32_t radius = ef_ctiles(g)[i].terrain_n == EF_MOUNTAIN ? 2 : sight;
    EfArea area;
    EF_TRY(ef_area_sorted(g, c, radius, &area));
    for (int k = area.len - 1; k >= 0; k--) {
        int32_t j = area.tiles[k];
        if (!ef_explored(g, j, p)) EF_TRY(ef_push(g, ef_action_at(EF_A_EXPLORE, p, ef_ctiles(g)[j].coordinates)));
    }
    return ef_ok();
}

/* explore.rs knows */
EF_FN EfResult ef_knows_player(const EfGame *g, int32_t p, int32_t other, int *out) {
    int i = ef_player_index(g, p);
    if (i == EF_NONE) return ef_err(EF_ERR_MISSING_PLAYER, 0);
    *out = other == EF_NATURE || p == other || ef_contains(g->players[i].known_players, g->players[i].known_players_len, other);
    return ef_ok();
}

/* explore.rs visible_unit_owner: the owner of the unit on tile i that p sees (inserting a
 * relation for an invisible foreign unit); *has = 0 when there is none */
EF_FN EfResult ef_visible_unit_owner(EfGame *g, int32_t p, int i, int *has, int32_t *owner) {
    *has = 0;
    if (!ef_explored(g, i, p)) return ef_ok();
    int32_t n = ef_ctiles(g)[i].unit_n;
    if (n == EF_NONE) return ef_ok();
    int32_t o = ef_cunits(g)[n].owner_n;
    if (ef_has_effect(ef_effect_list(g, ef_cunits(g)[n].effects_n), EF_INVISIBLE_EFFECT)) {
        int pi;
        EF_TRY(ef_player(g, p, &pi));
        if (o != p) {
            if (o == 0) return ef_ok();
            EfRelation *r;
            EF_TRY(ef_relation_entry(g, pi, o, &r));
            if (r->state != EF_PEACE) return ef_ok();
        }
    }
    *has = 1;
    *owner = o;
    return ef_ok();
}

/* explore.rs reveal_lighthouse: what seeing a map corner brings */
EF_FN EfResult ef_reveal_lighthouse(EfGame *g, const EfTables *t, int32_t p, EfXy c) {
    int32_t w = g->width - 1, h = g->tile_count / g->width - 1;
    int corner = (c.x == w && c.y == h) || (c.x == 0 && c.y == 0) || (c.x == 0 && c.y == h) || (c.x == w && c.y == 0);
    if (!corner) return ef_ok();
    int pi;
    EF_TRY(ef_player(g, p, &pi));
    EF_TRY(ef_check_task(g, t, pi, EF_LIGHTHOUSE_TASK));
    if (ef_task_slot(&g->players[pi], EF_LIGHTHOUSE_TASK) < 0) {
        EfAction e = ef_action(EF_A_ENABLE_TASK, p);
        e.i[0] = EF_LIGHTHOUSE_TASK;
        EF_TRY(ef_push(g, e));
    }
    EfXy capital;
    EF_TRY(ef_current_capital(g, p, &capital));
    if (!(capital.x == -1 && capital.y == -1)) {
        EfAction a = ef_action_at(EF_A_INCREASE_POPULATION, p, c);
        a.p[1] = capital;
        a.i[0] = 60;
        EF_TRY(ef_push(g, a));
    }
    return ef_push(g, ef_action_at(EF_A_LEVEL_UP_IMPROVEMENT, p, c));
}

/* explore.rs explore (the Explore action) */
EF_BIG EfResult ef_explore(EfGame *g, const EfTables *t, int32_t p, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE || ef_explored(g, i, p)) return ef_ok();
    EfTile *tile = &ef_tiles(g)[i];
    if (tile->explorers_len_n >= 5) return ef_short_list_full(5);
    EF_PUT(tile->explorers_n[tile->explorers_len_n], p, uint8_t);
    tile->explorers_len_n++; /* narrow: below 5, checked above */
    g->needs_routes = 1;
    EfAction score = ef_action_at(EF_A_INCREASE_SCORE, p, c);
    score.i[0] = 5;
    score.i[1] = 20;
    EF_TRY(ef_push(g, score));
    int has_unit;
    int32_t unit_owner = 0;
    EF_TRY(ef_visible_unit_owner(g, p, i, &has_unit, &unit_owner));
    int unit_contact = 0;
    if (has_unit) {
        int known;
        EF_TRY(ef_knows_player(g, p, unit_owner, &known));
        unit_contact = !known;
    }
    int has_city = ef_ctiles(g)[i].has_improvement_n && ef_ctiles(g)[i].improvement.kind_n == EF_CITY;
    int32_t city_owner = ef_ctiles(g)[i].owner_n;
    int city_contact = 0;
    if (has_city && city_owner != 0 && city_owner != p) {
        int known;
        EF_TRY(ef_knows_player(g, p, city_owner, &known));
        city_contact = !known;
    }
    if (unit_contact) EF_TRY(ef_push(g, ef_meet(p, unit_owner, c)));
    /* unit_owner != city_owner compares two Options: different unless both are present and equal */
    if (city_contact && (!unit_contact || !(has_unit && has_city && unit_owner == city_owner)))
        EF_TRY(ef_push(g, ef_meet(p, city_owner, c)));
    return ef_reveal_lighthouse(g, t, p, c);
}

/* explore.rs meet_action (the Meet action) */
EF_BIG EfResult ef_meet_action(EfGame *g, const EfTables *t, int32_t p, int32_t other, EfXy coordinates) {
    if (ef_player_index(g, p) == EF_NONE) return ef_ok();
    int known;
    EF_TRY(ef_knows_player(g, p, other, &known));
    if (known) return ef_ok();
    int oi = ef_player_index(g, other);
    if (oi == EF_NONE) return ef_ok();
    EF_TRY(ef_refresh_tech_cache(g, t, p));
    if (ef_player_ability(g, t, p, EF_AB_CAPITALVISION)) {
        EfXy start = g->players[oi].start_tile;
        int32_t si = ef_tile_index(g, start);
        if (si == EF_NONE) return ef_err(EF_ERR_CAPITAL_OFF_BOARD, 0);
        if (!ef_explored(g, si, p)) EF_TRY(ef_push(g, ef_action_at(EF_A_EXPLORE, p, start)));
    }
    int pi;
    EF_TRY(ef_player(g, p, &pi));
    EfPlayer *x = &g->players[pi];
    if (x->known_players_len >= EF_MAX_PLAYERS) return ef_short_list_full(EF_MAX_PLAYERS);
    x->known_players[x->known_players_len++] = other;
    EfRelation *r;
    EF_TRY(ef_relation_entry(g, pi, other, &r));
    r->first_meet = g->current_turn;
    uint32_t other_score = (uint32_t)(uint64_t)g->players[oi].score;
    int32_t tiers = ef_meet_tiers(other_score);
    int32_t count = g->player_count - 1;
    int32_t amount = (tiers < 5 ? tiers : 5) * 2 + (count == 2 && other == 1 ? 3 : 1);
    EfAction a = ef_action_at(EF_A_INCREASE_CURRENCY, p, coordinates);
    a.i[0] = amount;
    a.i[1] = 40;
    return ef_push(g, a);
}

#endif /* EF_EXPLORE_H */
