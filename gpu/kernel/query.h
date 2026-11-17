/* query.h - small read-only questions about the state that several rule files ask
 * (engine-fast/src/state.rs, menu.rs, infiltrate.rs). */
#ifndef EF_QUERY_H
#define EF_QUERY_H

#include "state.h"
#include "tables.h"
#include "errors.h"

#define EF_NATURE 255
#define EF_PEACE 1

EF_FN EfResult ef_ok(void) { EfResult r; r.code = EF_OK; r.pad = 0; r.arg = 0; return r; }
/* Store `v` in the narrowed field `field` of type T (int8_t, uint8_t, int16_t, uint16_t), or
 * stop with EF_ERR_FIELD_RANGE (its argument the kernel line) where it does not fit: a value
 * is never truncated. For functions returning EfResult. */
#define EF_PUT(field, v, T)                                                                              \
    do {                                                                                                 \
        int64_t ef_v_ = (int64_t)(v);                                                                    \
        if (ef_v_ < (int64_t)EF_MIN_##T || ef_v_ > (int64_t)EF_MAX_##T) return ef_err(EF_ERR_FIELD_RANGE, __LINE__); \
        (field) = (T)ef_v_; /* narrow: checked above */                                                  \
    } while (0)
#define EF_MIN_int8_t INT8_MIN
#define EF_MAX_int8_t INT8_MAX
#define EF_MIN_uint8_t 0
#define EF_MAX_uint8_t UINT8_MAX
#define EF_MIN_int16_t INT16_MIN
#define EF_MAX_int16_t INT16_MAX
#define EF_MIN_uint16_t 0
#define EF_MAX_uint16_t UINT16_MAX

EF_FN EfResult ef_err(int32_t code, int64_t arg) { EfResult r; r.code = code; r.pad = 0; r.arg = arg; return r; }

/* tables.rs tribe_unit: unit kind `kind` as tribe `tribe` gets it */
EF_FN EfResult ef_tribe_unit(const EfTables *t, int32_t tribe, int32_t kind, int32_t *out) {
    if (tribe < 0 || tribe >= EF_MAX_TRIBES || !(t->tribes[tribe].flags & EF_TRIBE_KNOWN)) return ef_err(EF_ERR_UNKNOWN_TRIBE, 0);
    if (kind < 0 || kind >= t->unit_count || !t->units[kind].known) return ef_err(EF_ERR_CATALOG_ENTRY, kind);
    int32_t id = t->unit_of[tribe][kind];
    if (id < 0) return ef_err(EF_ERR_UNIT_OVERRIDE_NULL, 0);
    *out = id;
    return ef_ok();
}

/* Whether the block is one the Rust engine could hold: its capacities those of the room it was
 * given (`tile_cap` tiles, `unit_cap` units: the batch's, or on the host the block's own), every length and index within its array, every unit and effect list a
 * tile reaches in use, list 0 empty. Rule functions may
 * then index without further checks; each entry point runs this on a block that comes from
 * outside (an upload), and the step will run it after each decision. A block that fails is a
 * broken invariant. */
/* An explorer's bit in a tile's explorers_mask_n: ids 0 to 6 their own bit, Nature bit 7, any
 * other id none (its tests scan the list). The mask is kept with the list: to_device and the
 * Explore action set it, and ef_game_check refuses a block where they differ. */
EF_FN uint8_t ef_explorer_bit(int32_t id) {
    if (id == EF_NATURE) return (uint8_t)0x80u;
    return id >= 0 && id < 7 ? (uint8_t)(1u << id) : (uint8_t)0u;
}

EF_BIG EfResult ef_game_check(const EfGame *g, int32_t tile_cap, int32_t unit_cap) {
#define EF_BAD(cond) do { if (cond) return ef_err(EF_ERR_INVARIANT, __LINE__); } while (0)
#define EF_LEN(len, cap) EF_BAD((len) < 0 || (len) > (cap))
    EF_BAD(g->width < 1 || g->width > EF_MAX_SIDE);
    EF_BAD(g->tile_count < 1 || g->tile_count > EF_MAX_TILES);
    /* the layout comes from the capacities alone, so a header whose capacities are in range and
     * the room's cannot point outside, nor a view of the same tile_cap overflow */
    EF_BAD(g->tile_cap < g->tile_count || g->tile_cap > EF_MAX_TILES);
    EF_BAD(g->unit_cap < 1 || g->unit_cap > EF_MAX_UNITS);
    EF_BAD(g->tile_cap != tile_cap || g->unit_cap != unit_cap);
    EF_BAD(g->player_count < 1 || g->player_count > EF_MAX_PLAYERS);
    EF_LEN(g->triggers_len, EF_MAX_TRIGGERS);
    EF_BAD(g->naming_count < 0 || g->naming_count > EF_NAMING_SEEDS || g->naming_next < 0 || g->naming_next > g->naming_count);
    EF_BAD(!ef_clists(g)[0].used_n || ef_clists(g)[0].len_n != 0);
    for (int i = 0; i <= g->unit_cap; i++) EF_LEN(ef_clists(g)[i].len_n, 9);
    for (int p = 0; p < g->player_count; p++) {
        const EfPlayer *x = &g->players[p];
        EF_LEN(x->tasks_len, EF_MAX_TASKS);
        EF_LEN(x->available_tech_len, EF_MAX_TECH);
        EF_LEN(x->known_players_len, EF_MAX_PLAYERS);
        EF_LEN(x->relations_len, EF_MAX_PLAYERS);
        EF_LEN(x->tech_cache_len, EF_MAX_TECH);
        EF_LEN(x->messages_len, EF_MAX_PLAYERS);
        EF_LEN(x->built_unique_len, 8);
    }
    for (int u = 0; u < g->unit_cap; u++) {
        const EfUnit *x = &ef_cunits(g)[u];
        if (!x->used_n) continue;
        EF_BAD(x->passenger_n != EF_NONE && (x->passenger_n < 0 || x->passenger_n >= g->unit_cap || !ef_cunits(g)[x->passenger_n].used_n));
        EF_BAD(x->effects_n < 0 || x->effects_n > g->unit_cap || !ef_clists(g)[x->effects_n].used_n);
    }
    for (int i = 0; i < g->tile_count; i++) {
        const EfTile *x = &ef_ctiles(g)[i];
        EF_LEN(x->explorers_len_n, 5);
        {
            uint8_t mask = 0;
            for (int k = 0; k < x->explorers_len_n; k++) mask = (uint8_t)(mask | ef_explorer_bit(x->explorers_n[k]));
            EF_BAD(x->explorers_mask_n != mask);
        }
        EF_LEN(x->effects_len_n, 5);
        EF_LEN(x->improvement.effects_len_n, 2);
        EF_BAD(x->unit_n != EF_NONE && (x->unit_n < 0 || x->unit_n >= g->unit_cap || !ef_cunits(g)[x->unit_n].used_n));
        /* a carrier chain is short: a unit and the one it carries */
        int depth = 0;
        for (int u = x->unit_n; u != EF_NONE; u = ef_cunits(g)[u].passenger_n) EF_BAD(++depth > 4);
    }
#undef EF_LEN
#undef EF_BAD
    return ef_ok();
}

/* state.rs Game::player: the index in players of the player with this id, or EF_NONE */
EF_FN int ef_player_index(const EfGame *g, int32_t id) {
    for (int i = 0; i < g->player_count; i++)
        if (g->players[i].id == id) return i;
    return EF_NONE;
}

/* state.rs Game::index: the tile index of an on-board coordinate, or EF_NONE */
EF_FN int32_t ef_tile_index(const EfGame *g, EfXy c) {
    int32_t i = ef_wadd(ef_wmul(c.y, g->width), c.x);
    return (c.x >= 0 && c.y >= 0 && c.x < g->width && i >= 0 && i < g->tile_count) ? i : EF_NONE;
}

EF_FN int ef_contains(const int32_t *items, int32_t len, int32_t v) {
    for (int i = 0; i < len; i++)
        if (items[i] == v) return 1;
    return 0;
}
/* ef_contains on a narrowed list: player ids (uint8) or small ids (int8) */
EF_FN int ef_contains_u8(const uint8_t *items, int32_t len, int32_t v) {
    for (int i = 0; i < len; i++)
        if (items[i] == v) return 1;
    return 0;
}
/* whether `id` explored the tile */
EF_FN int ef_tile_explored(const EfTile *t, int32_t id) {
    uint8_t b = ef_explorer_bit(id);
    return b ? (t->explorers_mask_n & b) != 0 : ef_contains_u8(t->explorers_n, t->explorers_len_n, id);
}
EF_FN int ef_contains_i8(const int8_t *items, int32_t len, int32_t v) {
    for (int i = 0; i < len; i++)
        if (items[i] == v) return 1;
    return 0;
}

/* state.rs Player::relation and menu.rs relation: the stored state toward `other`, 0 if none */
EF_FN int32_t ef_relation_state(const EfPlayer *p, int32_t other) {
    for (int i = 0; i < p->relations_len; i++)
        if (p->relation_ids[i] == other) return p->relations[i].state;
    return 0;
}
EF_FN int32_t ef_relation(const EfGame *g, int32_t me, int32_t other) {
    if (other == 0) return 0;
    int i = ef_player_index(g, me);
    return i == EF_NONE ? 0 : ef_relation_state(&g->players[i], other);
}

/* menu.rs unit_effect_list: the effect list in a slot, the empty list 0 for a slot out of
 * range (list 0 is empty: to_device and ef_game_check make sure of it) */
EF_FN const EfEffectList *ef_effect_list(const EfGame *g, int32_t slot) {
    return slot >= 0 && slot <= g->unit_cap ? &ef_clists(g)[slot] : &ef_clists(g)[0];
}
EF_FN int ef_has_effect(const EfEffectList *l, int64_t effect) {
    for (int i = 0; i < l->len_n; i++)
        if (l->items_n[i] == effect) return 1;
    return 0;
}

/* menu.rs unlockable_techs: children of known techs, and an unknown parent of a known child,
 * in catalog tree order; `out` has room for EF_MAX_TECHS */
EF_BIG EfResult ef_unlockable_techs(const EfGame *g, const EfTables *t, int32_t player, int32_t *out, int32_t *count) {
    int pi = ef_player_index(g, player);
    if (pi == EF_NONE) return ef_err(EF_ERR_MISSING_PLAYER, 0);
    const EfPlayer *p = &g->players[pi];
    const EfTechTree *tree = ef_tribe_tree(t, p->tribe);
    if (!tree) return ef_err(EF_ERR_UNKNOWN_TRIBE, 0);
    int32_t n = 0;
    if (tree->order_error) return ef_err(EF_ERR_TABLE, tree->order_error - 1);
    /* membership in the player's techs and in `out` as bit sets (ids below EF_MAX_TECHS, as the
     * catalog's are); an id outside that range keeps the list scans */
    uint64_t have[(EF_MAX_TECHS + 63) / 64], taken[(EF_MAX_TECHS + 63) / 64];
    int sets = 1;
    for (int w = 0; w < (EF_MAX_TECHS + 63) / 64; w++) have[w] = taken[w] = 0;
    for (int k = 0; k < p->available_tech_len; k++) {
        int32_t id = p->available_tech[k];
        if (id < 0 || id >= EF_MAX_TECHS) sets = 0;
        else have[id >> 6] |= 1ull << (id & 63);
    }
#define EF_HAS_TECH(id) (sets && (id) >= 0 && (id) < EF_MAX_TECHS ? (int)((have[(id) >> 6] >> ((id) & 63)) & 1u) \
                                                                  : ef_contains(p->available_tech, p->available_tech_len, (id)))
#define EF_TAKEN(id) ((id) >= 0 && (id) < EF_MAX_TECHS ? (int)((taken[(id) >> 6] >> ((id) & 63)) & 1u) : ef_contains(out, n, (id)))
#define EF_TAKE(id) do { out[n++] = (id); if ((id) >= 0 && (id) < EF_MAX_TECHS) taken[(id) >> 6] |= 1ull << ((id) & 63); } while (0)
    for (int o = 0; o < tree->order_len; o++) {
        int32_t parent = tree->order[o];
        if (tree->links[parent].children_error) return ef_err(EF_ERR_TABLE, tree->links[parent].children_error - 1);
        const int32_t *children = tree->links[parent].children;
        int32_t len = tree->links[parent].children_len;
        if (EF_HAS_TECH(parent)) {
            for (int k = 0; k < len; k++) {
                int32_t c = children[k];
                if (!EF_HAS_TECH(c) && !EF_TAKEN(c)) EF_TAKE(c);
            }
        } else {
            int any = 0;
            for (int k = 0; k < len; k++) any |= EF_HAS_TECH(children[k]);
            if (any && !EF_TAKEN(parent)) EF_TAKE(parent);
        }
    }
#undef EF_HAS_TECH
#undef EF_TAKEN
#undef EF_TAKE
    *count = n;
    return ef_ok();
}

/* infiltrate.rs revealed_kinds: the resource kinds the player's tech (unlocked, or unlockable
 * next) reveals, as a bit set; 0 for a missing player */
EF_BIG EfResult ef_revealed_kinds(const EfGame *g, const EfTables *t, int32_t player, uint32_t *kinds) {
    *kinds = 0;
    int pi = ef_player_index(g, player);
    if (pi == EF_NONE) return ef_ok();
    const EfPlayer *p = &g->players[pi];
    const EfTechTree *tree = ef_tribe_tree(t, p->tribe);
    if (!tree) return ef_err(EF_ERR_UNKNOWN_TRIBE, 0);
    uint32_t found = 0;
    for (int i = 0; i < p->available_tech_len; i++) {
        int32_t k = p->available_tech[i];
        if (!ef_tech_known(t, k)) continue;
        if (tree->links[k].reveals_error) return ef_err(EF_ERR_TABLE, tree->links[k].reveals_error - 1);
        found |= tree->links[k].reveals;
    }
    int32_t unlockable[EF_MAX_TECHS], count = 0;
    EF_TRY(ef_unlockable_techs(g, t, player, unlockable, &count));
    for (int i = 0; i < count; i++) {
        int32_t k = unlockable[i];
        /* tree.reveals.get(tech).ok_or("unknown tech"): every id here is inside the table */
        if (tree->links[k].reveals_error) return ef_err(EF_ERR_TABLE, tree->links[k].reveals_error - 1);
        found |= tree->links[k].reveals;
    }
    *kinds = found & ~1u;   /* kind 0 is "no resource" */
    return ef_ok();
}

/* the player's tribe's tech tree (tables.rs tribe_tree) */
EF_FN EfResult ef_known_tribe(const EfTables *t, const EfPlayer *p, const EfTechTree **tree) {
    *tree = ef_tribe_tree(t, p->tribe);
    if (!*tree) return ef_err(EF_ERR_UNKNOWN_TRIBE, 0);
    return ef_ok();
}

/* tables.rs TechTree units_of / improvements_of */
EF_FN EfResult ef_tech_unlocks(const EfTables *t, const EfTechTree *tree, int32_t tech, int units, const int32_t **list,
                               int32_t *len) {
    if (tech < 0 || tech >= t->tech_count) return ef_err(EF_ERR_UNKNOWN_TECH, 0);
    const EfTechLinks *d = &tree->links[tech];
    int32_t error = units ? d->units_error : d->improvements_error;
    if (error) return ef_err(EF_ERR_TABLE, error - 1);
    *list = units ? d->units : d->improvements;
    *len = units ? d->units_len : d->improvements_len;
    return ef_ok();
}

EF_FN int ef_owns_kind(const EfGame *g, int32_t owner, int32_t kind) {
    for (int k = 0; k < g->tile_count; k++) {
        int32_t n = ef_ctiles(g)[k].unit_n;
        if (n != EF_NONE && ef_cunits(g)[n].owner_n == owner && ef_cunits(g)[n].kind_n == kind) return 1;
    }
    return 0;
}

#endif /* EF_QUERY_H */
