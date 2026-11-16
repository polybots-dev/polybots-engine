/* capture.h - port of engine-fast/src/capture.rs: the Capture command, the CaptureCity action
 * and the city name it draws. The name's text is not kept in the block (no rule reads it),
 * but drawing it consumes the naming seeds and can stop the step, so the word is built in
 * full to decide both. */
#ifndef EF_CAPTURE_H
#define EF_CAPTURE_H

#include "ruins.h"

#define EF_WORD_UNITS 16

/* capture.rs admit_writes: the relation lookup toward the city's owner */
EF_FN EfResult ef_capture_admit_writes(EfGame *g, int32_t p, EfXy c) {
    const EfTile *tile = ef_tile_c(g, c);
    if (!tile) return ef_err(EF_ERR_CAPTURE_TILE, 0);
    int32_t owner = tile->owner_n;
    if (owner != 0) {
        int i = ef_player_index(g, p);
        if (i != EF_NONE) {
            EfRelation *r;
            EF_TRY(ef_relation_entry(g, i, owner, &r));
        }
    }
    return ef_ok();
}

/* capture.rs capture_command */
EF_BIG EfResult ef_capture_command(EfGame *g, int32_t p, uint32_t unit_id, EfXy c) {
    EF_TRY(ef_base(g, p, 7));
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_CAPTURE_TILE, 0);
    if (!ef_ctiles(g)[i].has_improvement_n) return ef_ok();
    int32_t old = ef_ctiles(g)[i].owner_n;
    if (ef_player_index(g, p) == EF_NONE) return ef_ok();
    int32_t node = ef_find_unit(g, unit_id);
    int invisible = 0;
    if (node != EF_NONE) {
        EfUnit u = ef_cunits(g)[node];
        invisible = ef_unit_effect(g, &u, EF_INVISIBLE_EFFECT);
        EF_TRY(ef_make_exhausted(g, node));
    }
    EfAction u = ef_action(EF_A_UPDATE_CITY_CONNECTIONS, p);
    u.len = 2;
    u.list.players[0] = p;
    u.list.players[1] = old;
    EF_TRY(ef_push(g, u));
    EF_TRY(ef_push(g, ef_action(EF_A_UPDATE_ROUTES, p)));
    EfAction a = ef_action_at(EF_A_CAPTURE_CITY, p, c);
    a.i[0] = old;
    EF_TRY(ef_push(g, a));
    if (invisible) {
        EfAction r = ef_action_at(EF_A_REVEAL, p, c);
        r.p[1] = ef_xy(-1, -1);
        r.i[0] = 0;
        EF_TRY(ef_push(g, r));
    }
    return ef_ok();
}

/* the next default-Random generator from the game's naming queue */
EF_FN EfResult ef_next_default(EfGame *g, EfSystemRandom *r) {
    if (g->naming_next >= g->naming_count) return ef_err(EF_ERR_NAME_SEEDS, 0);
    ef_random_new(r, g->naming_seeds[g->naming_next++]);
    return ef_ok();
}

EF_FN int ef_word_is(const int32_t *word, int32_t len, const char *text) {
    int k = 0;
    for (; text[k]; k++)
        if (k >= len || word[k] != (int32_t)(unsigned char)text[k]) return 0;
    return k == len;
}

/* capture.rs syllables: the language slot for a tribe and skin (a skin's language replaces its
 * tribe's; no tribe, or a tribe without a language, speaks the default) */
EF_FN EfResult ef_language(const EfTables *t, int32_t tribe, int32_t skin, const EfLanguage **out) {
    int32_t slot = 0;
    if (tribe != 0) {
        int k = 0;
        while (k < t->skin_language_count && t->skin_language_skin[k] != (int64_t)skin) k++;
        if (k < t->skin_language_count) slot = t->skin_language[k];
        else if (ef_tribe_flags(t, tribe) & EF_TRIBE_KNOWN) slot = t->tribes[tribe].language;
    }
    *out = &t->languages[slot];
    return ef_ok();
}

/* capture.rs make_word: the word drawn from `seed` (0: the next default-Random seed), cased
 * unless the tribe is 5; the units go to word[0..*len] */
EF_BIG EfResult ef_make_word(EfGame *g, const EfTables *t, int32_t tribe, int32_t skin, int32_t seed, int32_t *word,
                             int32_t *len_out) {
    const EfLanguage *d;
    EF_TRY(ef_language(t, tribe, skin, &d));
    EfSystemRandom rng;
    if (seed == 0) EF_TRY(ef_next_default(g, &rng));
    else ef_random_new(&rng, seed);
    float length = ef_name_length(ef_random_next(&rng));
    int frames = 1;
    int32_t len;
    for (;;) {
        len = 0;
        int remaining = 101;
        while (ef_name_short(len, length)) {
            remaining -= 1;
            if (remaining == 0) break;
            int32_t k = ef_random_next_range(&rng, 0, d->piece_count);
            int32_t n = d->piece_len[k];
            if (len + n > EF_WORD_UNITS) return ef_err(EF_ERR_WORD_FULL, 0);
            for (int32_t q = 0; q < n; q++) word[len + q] = d->pieces[k][q];
            len += n;
            if (len > 0 && (word[0] == ' ' || word[0] == '-')) len = 0;
        }
        if (!ef_word_is(word, len, "anus") && !ef_word_is(word, len, "loli") && !ef_word_is(word, len, "boner")
            && !ef_word_is(word, len, "niga"))
            break;
        EF_TRY(ef_next_default(g, &rng));
        frames += 1;
    }
    if (tribe != 5) {
        for (int f = 0; f < frames; f++) {
            if (len == 0) return ef_err(EF_ERR_EMPTY_NAME, 0);
            int32_t u = word[0];
            if (u == 0x69 && g->naming_case == 0) u = 0x49;
            else if (u == 0x69 && g->naming_case == 1) u = 0x130;
            else if (u == 0xfc) u = 0xdc;
            else if (u == 0x103) u = 0x102;
            else if (u > 127 || u == 0x69) return ef_err(EF_ERR_NAME_CASING, 0);
            else if (u >= 0x61 && u <= 0x7a) u -= 32;
            word[0] = u;
        }
    }
    *len_out = len;
    return ef_ok();
}

/* capture.rs city_name: the word for a city at c, from the city-name hash */
EF_BIG EfResult ef_city_name(EfGame *g, const EfTables *t, int32_t tribe, int32_t skin, EfXy c, int32_t *word, int32_t *len) {
    if (!g->has_village_name_hash) {
        g->has_village_name_hash = 1;
        g->village_name_hash = g->village_name_seed;
    }
    uint32_t words[2] = { (uint32_t)c.x, (uint32_t)c.y };
    int32_t seed = ef_hash_range_i32(ef_hash_words((uint32_t)(int32_t)g->village_name_hash, words, 2), 0, INT32_MAX);
    return ef_make_word(g, t, tribe, skin, seed, word, len);
}

/* capture.rs city_area: the sorted border area, keeping tiles ruled by nobody or this city */
EF_FN EfResult ef_city_area(const EfGame *g, EfXy city, EfArea *out) {
    const EfTile *tile = ef_tile_c(g, city);
    if (!tile || !tile->has_improvement_n) return ef_err(EF_ERR_CITY_AREA_IMPROVEMENT, 0);
    EfArea area;
    EF_TRY(ef_area_sorted(g, city, tile->improvement.border_size_n, &area));
    out->len = 0;
    for (int k = 0; k < area.len; k++) {
        EfXy r = ef_ctiles(g)[area.tiles[k]].ruling_city;
        if ((r.x == -1 && r.y == -1) || ef_xy_eq(r, city)) out->tiles[out->len++] = area.tiles[k];
    }
    return ef_ok();
}

EF_FN EfResult ef_embassy_level(EfGame *g, int i, int32_t other, int32_t *level) {
    EfRelation *r;
    EF_TRY(ef_relation_entry(g, i, other, &r));
    *level = r->embassy_level;
    return ef_ok();
}

/* capture.rs capture_city: the CaptureCity combat action. The score
 * children are worked out with the ownership change and queued after the metropolis ones. */
EF_BIG EfResult ef_capture_city(EfGame *g, const EfTables *t, int32_t actor, EfXy city) {
    int32_t center = ef_tile_index(g, city);
    if (center == EF_NONE) return ef_err(EF_ERR_CAPTURE_CITY_TILE, 0);
    int ai;
    EF_TRY(ef_player(g, actor, &ai));
    if (!ef_ctiles(g)[center].has_improvement_n) return ef_err(EF_ERR_CAPTURE_CITY_IMPROVEMENT, 0);
    if (!ef_ctiles(g)[center].improvement.named_n) {
        int32_t tribe = g->players[ai].tribe;
        if (!(ef_tribe_flags(t, tribe) & EF_TRIBE_KNOWN)) return ef_err(EF_ERR_NAME_UNKNOWN_TRIBE, 0);
        int32_t word[EF_WORD_UNITS], len;
        EF_TRY(ef_city_name(g, t, tribe, g->players[ai].skin, city, word, &len));
        ef_tiles(g)[center].improvement.named_n = len > 0; /* narrow: 0 or 1 */
    }
    int32_t here = ef_ctiles(g)[center].unit_n;
    if (here != EF_NONE) {
        const EfUnitData *d = ef_unit_data(t, ef_cunits(g)[here].kind_n);
        if (!(d && ef_has(d->abilities, EF_AB_INDEPENDENT))) ef_units(g)[here].home = city;
    }
    for (int k = 0; k < g->tile_count; k++) {
        int32_t n = ef_ctiles(g)[k].unit_n;
        if (n == EF_NONE) continue;
        EfUnit *u = &ef_units(g)[n];
        if (u->owner_n != actor && ef_xy_eq(u->home, city)) u->home = ef_xy(-1, -1);
    }
    g->players[ai].cities += 1;
    int32_t old = ef_ctiles(g)[center].owner_n;
    int previous = ef_player_index(g, old);
    if (previous != EF_NONE) g->players[previous].cities -= 1;
    EfArea area;
    EF_TRY(ef_city_area(g, city, &area));
    int32_t claimed[EF_AREA_MAX], amounts[EF_AREA_MAX], owners[EF_AREA_MAX];
    int nc = 0;
    for (int k = area.len - 1; k >= 0; k--) {
        int32_t i = area.tiles[k];
        const EfTile *tile = &ef_ctiles(g)[i];
        if (tile->owner_n != actor) {
            int32_t amount = 20;
            if (tile->has_improvement_n && !ef_xy_eq(tile->coordinates, city))
                amount = ef_wadd(amount, ef_improvement_score(t, g, i));
            claimed[nc] = i;
            amounts[nc] = amount;
            owners[nc] = tile->owner_n;
            nc++;
        }
        EF_PUT(ef_tiles(g)[i].owner_n, actor, uint8_t);
        ef_tiles(g)[i].ruling_city = city;
    }
    if (ef_ctiles(g)[center].improvement.level_n > 1) {
        if (ef_task_slot(&g->players[ai], EF_TASK_METROPOLIS) < 0) {
            EfAction e = ef_action(EF_A_ENABLE_TASK, actor);
            e.i[0] = EF_TASK_METROPOLIS;
            EF_TRY(ef_push(g, e));
        }
        EF_TRY(ef_check_task(g, t, ai, EF_TASK_METROPOLIS));
    }
    for (int k = 0; k < nc; k++) {
        EfAction s = ef_action_at(EF_A_INCREASE_SCORE, actor, ef_ctiles(g)[claimed[k]].coordinates);
        s.i[0] = amounts[k];
        s.i[1] = 50;
        EF_TRY(ef_push(g, s));
        if (owners[k] != 0) {
            EfAction x = ef_action(EF_A_DECREASE_SCORE, owners[k]);
            x.i[0] = amounts[k];
            EF_TRY(ef_push(g, x));
        }
    }
    for (int k = 0; k < nc; k++) {
        if (owners[k] != 0) continue;
        const EfTile *tile = &ef_ctiles(g)[claimed[k]];
        int32_t population = 0;
        if (tile->has_improvement_n && ef_improvement_known(t, tile->improvement.kind_n))
            population = ef_population_at_level(&t->improvements[tile->improvement.kind_n], tile->improvement.level_n);
        for (int32_t q = 0; q < population; q++) {
            EfAction a = ef_action_at(EF_A_INCREASE_POPULATION, actor, tile->coordinates);
            a.p[1] = city;
            a.i[0] = 60;
            EF_TRY(ef_push(g, a));
        }
    }
    int32_t selected;
    int completed;
    EF_TRY(ef_winner(g, &selected, &completed));
    if (completed) {
        EfAction over = ef_action(EF_A_GAME_OVER, actor);
        over.i[0] = selected;
        EF_TRY(ef_push(g, over));
    }
    if (previous != EF_NONE && !ef_alive(g, old)) {
        EfAction w = ef_action(EF_A_WIPE_PLAYER, actor);
        w.i[0] = old;
        EF_TRY(ef_push(g, w));
    }
    int32_t score = ef_improvement_score(t, g, center);
    EF_TRY(ef_add_score(g, actor, (int64_t)score));
    if (old != 0) {
        EfAction x = ef_action(EF_A_DECREASE_SCORE, old);
        x.i[0] = score;
        EF_TRY(ef_push(g, x));
    }
    EF_TRY(ef_city_area(g, city, &area));
    EF_TRY(ef_claim_area(g, t, actor, ai, area.tiles, area.len));
    int32_t radius = ef_ctiles(g)[center].improvement.border_size_n;
    int32_t h = g->tile_count / g->width;
    int32_t x0 = city.x - radius < 0 ? 0 : city.x - radius, x1 = city.x + radius > g->width - 1 ? g->width - 1 : city.x + radius;
    int32_t y0 = city.y - radius < 0 ? 0 : city.y - radius, y1 = city.y + radius > h - 1 ? h - 1 : city.y + radius;
    for (int32_t xx = x0; xx <= x1; xx++) {
        for (int32_t yy = y0; yy <= y1; yy++) {
            EfXy c = ef_xy(xx, yy);
            const EfTile *x = ef_tile_c(g, c);
            if (!ef_contains_u8(x->explorers_n, x->explorers_len_n, actor)) EF_TRY(ef_push(g, ef_action_at(EF_A_EXPLORE, actor, c)));
        }
    }
    int32_t ids[EF_MAX_PLAYERS];
    int np = g->player_count;
    for (int k = 0; k < np; k++) ids[k] = g->players[k].id;
    int32_t capital = ef_ctiles(g)[center].capital_of_n;
    if (capital != 0) {
        int ci;
        EF_TRY(ef_player(g, capital, &ci));
        for (int k = 0; k < np; k++) {
            int32_t level;
            EF_TRY(ef_embassy_level(g, ci, ids[k], &level));
            if (level > 0) {
                EfAction x = ef_action(EF_A_DESTROY_EMBASSY, ids[k]);
                x.i[0] = capital;
                EF_TRY(ef_push(g, x));
            }
        }
    }
    if (old != 0) EF_TRY(ef_attack_diplomacy(g, actor, old));
    for (int k = 0; k < np; k++) {
        int32_t level;
        EF_TRY(ef_embassy_level(g, ai, ids[k], &level));
        const EfTile *x = &ef_ctiles(g)[center];
        if (level >= 2 && !ef_contains_u8(x->explorers_n, x->explorers_len_n, ids[k]))
            EF_TRY(ef_push(g, ef_action_at(EF_A_EXPLORE, ids[k], city)));
    }
    ef_tiles(g)[center].terrain_n = EF_FIELD; /* narrow: a terrain constant */
    /* the city tile becomes field, flooded by a flooding tribe and drained by any other */
    if (ef_tribe_has(t, g->players[ai].tribe, EF_AB_FLOODED)) return ef_flood_tile(g, actor, city);
    ef_drain_tile(&ef_tiles(g)[center]);
    return ef_ok();
}

#endif /* EF_CAPTURE_H */
