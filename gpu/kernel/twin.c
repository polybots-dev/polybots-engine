/* twin.c - the kernel sources as a host library: what the Rust tests call to compare the C
 * port with the Rust engine without a GPU. */
#define EF_COVERAGE 1
#include <stdint.h>
int64_t ef_coverage_commands[32];
int64_t ef_coverage_actions[96];
#include "floats.h"

void ef_twin_float_cases(int32_t op, int64_t count, const uint32_t *args, uint64_t *out) {
    for (int64_t i = 0; i < count; i++)
        out[i] = ef_float_case(op, args[4 * i], args[4 * i + 1], args[4 * i + 2], args[4 * i + 3]);
}

uint32_t ef_twin_hash_words(uint32_t seed, const uint32_t *words, int32_t count) {
    return ef_hash_words(seed, words, count);
}

/* `count` draws from a fresh stream: draw i is next() for kinds[i] = 0, and
 * next_range(lows[i], highs[i]) for kinds[i] = 1 */
void ef_twin_random_stream(int32_t seed, int32_t count, const uint8_t *kinds, const int32_t *lows,
                           const int32_t *highs, int32_t *out) {
    EfSystemRandom r;
    ef_random_new(&r, seed);
    for (int32_t i = 0; i < count; i++)
        out[i] = kinds[i] ? ef_random_next_range(&r, lows[i], highs[i]) : ef_random_next(&r);
}

#include <stddef.h>
#include "state.h"

/* The sizes and offsets tests/state_roundtrip.rs compares with the Rust mirror's, in its
 * order; returns how many were written. */
int32_t ef_twin_layout(int64_t *out, int32_t capacity) {
    const int64_t v[] = {
        sizeof(EfXy), sizeof(EfImprovement), sizeof(EfTile), sizeof(EfUnit), sizeof(EfEffectList), sizeof(EfTask),
        sizeof(EfRelation), sizeof(EfPlayer), sizeof(EfTrigger), sizeof(EfGame),
        offsetof(EfImprovement, effects_n), offsetof(EfTile, improvement), offsetof(EfTile, effects_n),
        offsetof(EfUnit, birth_climate_skin_n), offsetof(EfEffectList, items_n), offsetof(EfTask, explored_tiles),
        offsetof(EfPlayer, tasks), offsetof(EfPlayer, relations), offsetof(EfPlayer, message_kinds),
        offsetof(EfPlayer, capital_count_cache), offsetof(EfTrigger, coordinates), offsetof(EfGame, rules_mode),
        offsetof(EfGame, current_state), offsetof(EfGame, triggers), offsetof(EfGame, players),
        (int64_t)ef_lists_offset(EF_MAX_TILES, EF_MAX_UNITS), (int64_t)ef_units_offset(),
        (int64_t)ef_tiles_offset(EF_MAX_UNITS), offsetof(EfGame, naming_seeds),
        (int64_t)ef_block_size(EF_MAX_TILES, EF_MAX_UNITS), offsetof(EfGame, tile_cap),
        sizeof(EfAction), offsetof(EfAction, w), offsetof(EfAction, p), offsetof(EfAction, list),
        offsetof(EfGame, actions),
    };
    int32_t n = (int32_t)(sizeof v / sizeof v[0]);
    for (int32_t i = 0; i < n && i < capacity; i++) out[i] = v[i];
    return n < capacity ? n : capacity;
}

uint64_t ef_twin_game_digest(const EfGame *g) { return ef_game_digest(g, ef_block_size(g->tile_cap, g->unit_cap)); }
void ef_twin_game_probe(const EfGame *g, int64_t *out) { ef_game_probe(g, out); }

#include "view.h"

int32_t ef_twin_tables_size(void) { return (int32_t)sizeof(EfTables); }
int64_t ef_twin_view_size(int32_t tile_cap) { return (int64_t)ef_view_size(tile_cap); }
const char *ef_twin_error_text(int32_t code) { return ef_error_text(code); }

/* The host build's entry points check a block against its own capacities, and a view passed with
 * it has room for them: the Rust side (state.rs Block::as_ptr, view.rs Views) makes sure of both. */

/* view.rs write; the result's code, with its argument in *arg */
int32_t ef_twin_view(const EfGame *g, const EfTables *t, int32_t viewer, EfView *out, int64_t *arg) {
    EfResult r = ef_game_check(g, g->tile_cap, g->unit_cap);
    if (r.code == EF_OK) r = ef_view_write(g, t, viewer, out);
    *arg = r.arg;
    return r.code;
}

#include "dump.h"

int32_t ef_twin_game_dump(const EfGame *g, int64_t *out, int32_t capacity) { return ef_game_dump(g, out, capacity); }
int32_t ef_twin_err_table(void) { return EF_ERR_TABLE; }
int32_t ef_twin_error_count(void) { return EF_ERROR_COUNT; }

#include "step.h"
#include "status.h"
#include "outputs.h"
/* a kind past a table's end would go uncounted (EF_COVER bounds it) */
_Static_assert(EF_COMMAND_KINDS <= 32, "command kinds fit the coverage table");
_Static_assert(EF_ACTION_KINDS <= 96, "action kinds fit the coverage table");

/* step.rs advance on the host build; the result's code, with its argument in *arg and the
 * elapsed-turns flag in *elapsed */
int32_t ef_twin_advance(EfGame *g, const EfTables *t, const EfMove *move, int32_t *elapsed, int64_t *arg) {
    EfResult r = ef_game_check(g, g->tile_cap, g->unit_cap);
    if (r.code == EF_OK) r = ef_advance(g, t, move, elapsed);
    *arg = r.arg;
    return r.code;
}
/* capture.rs city_name on the host build: the word's UTF-16 units go to word[0..*len]
 * (capacity EF_WORD_UNITS) */
int32_t ef_twin_city_name(EfGame *g, const EfTables *t, int32_t tribe, int32_t skin, int32_t x, int32_t y, int32_t *word,
                          int32_t *len, int64_t *arg) {
    EfResult r = ef_city_name(g, t, tribe, skin, ef_xy(x, y), word, len);
    *arg = r.arg;
    return r.code;
}
/* menu.h ef_menu on the host build, with caller-provided work and list (sizes from
 * ef_twin_menu_sizes) */
int32_t ef_twin_menu(const EfGame *g, const EfTables *t, EfMenuWork *w, EfMenu *m, int64_t *arg) {
    EfResult r = ef_game_check(g, g->tile_cap, g->unit_cap);
    m->len = 0;
    if (r.code == EF_OK) r = ef_menu(g, t, w, m);
    *arg = r.arg;
    return r.code;
}
/* status.h on the host build: index < 0 refreshes the slot (after a reset), else steps it by
 * row `index` of `m` (the slot's list from the last refresh) and refreshes it */
int32_t ef_twin_slot(EfGame *g, const EfTables *t, EfMenuWork *w, EfMenu *m, EfView *v, int32_t index, int32_t *status,
                     int32_t *winner, int64_t *arg) {
    EfResult r = ef_game_check(g, g->tile_cap, g->unit_cap);
    if (r.code == EF_OK) r = index < 0 ? ef_refresh(g, t, w, m, v, status, winner) : ef_slot_step(g, t, w, m, v, index, status, winner);
    *arg = r.arg;
    return r.code;
}
/* menu.h ef_menu_previews on the host build: `out` holds int16 x 4 per row of `m`, a list
 * ef_twin_menu just made for `g` */
int32_t ef_twin_previews(const EfGame *g, const EfTables *t, const EfMenu *m, int16_t *out, int64_t *arg) {
    EfResult r = ef_menu_previews(g, t, m, (int16_t (*)[4])(void *)out);
    *arg = r.arg;
    return r.code;
}
void ef_twin_menu_sizes(int64_t *work, int64_t *menu) {
    *work = (int64_t)sizeof(EfMenuWork);
    *menu = (int64_t)sizeof(EfMenu);
}
int32_t ef_twin_max_rows(void) { return EF_MAX_ROWS; }
/* step.rs command on the host build */
int32_t ef_twin_command(EfGame *g, const EfTables *t, const EfMove *move, int64_t *arg) {
    EfResult r = ef_game_check(g, g->tile_cap, g->unit_cap);
    if (r.code == EF_OK) r = ef_command(g, t, move);
    *arg = r.arg;
    return r.code;
}
int32_t ef_twin_error_failed(int32_t code) { return ef_error_is_failure(code); }
/* EF_PUT at work: store v in an int8, uint8, int16 and uint16 field in turn; the result's code
 * per type in out[0..4] (0 stored, else the refusal) */
static EfResult ef_put_i8(int8_t *f, int64_t v) { EF_PUT(*f, v, int8_t); return ef_ok(); }
static EfResult ef_put_u8(uint8_t *f, int64_t v) { EF_PUT(*f, v, uint8_t); return ef_ok(); }
static EfResult ef_put_i16(int16_t *f, int64_t v) { EF_PUT(*f, v, int16_t); return ef_ok(); }
static EfResult ef_put_u16(uint16_t *f, int64_t v) { EF_PUT(*f, v, uint16_t); return ef_ok(); }
void ef_twin_put(int64_t v, int32_t out[4], int64_t stored[4]) {
    int8_t a = 0; uint8_t b = 0; int16_t c = 0; uint16_t d = 0;
    out[0] = ef_put_i8(&a, v).code;
    out[1] = ef_put_u8(&b, v).code;
    out[2] = ef_put_i16(&c, v).code;
    out[3] = ef_put_u16(&d, v).code;
    stored[0] = a; stored[1] = b; stored[2] = c; stored[3] = d;
}
int32_t ef_twin_error_capacity(int32_t code) { return ef_error_is_capacity(code); }
int32_t ef_twin_move_size(void) { return (int32_t)sizeof(EfMove); }

void ef_twin_coverage_reset(void) {
    memset(ef_coverage_commands, 0, sizeof ef_coverage_commands);
    memset(ef_coverage_actions, 0, sizeof ef_coverage_actions);
}
void ef_twin_coverage_read(int64_t *commands, int64_t *actions) {
    memcpy(commands, ef_coverage_commands, sizeof ef_coverage_commands);
    memcpy(actions, ef_coverage_actions, sizeof ef_coverage_actions);
}

/* takes of the search scratch while another was held, since the process started (movement.h) */
int64_t ef_twin_scratch_overlaps(void) { return __atomic_load_n(&ef_scratch_overlaps, __ATOMIC_RELAXED); }

/* outputs.h ef_defence_plane for the player to move: one int16 per tile of `g` in `out` */
int32_t ef_twin_defence(const EfGame *g, const EfTables *t, int16_t *out, int64_t *arg) {
    EfResult r = ef_game_check(g, g->tile_cap, g->unit_cap);
    if (r.code == EF_OK) r = ef_defence_plane(g, t, g->players[g->current_player_index].id, out);
    *arg = r.arg;
    return r.code;
}

/* outputs.h ef_other_view for the player to move; the view's viewer is EF_NONE when there is no
 * other seat */
int32_t ef_twin_other_view(const EfGame *g, const EfTables *t, EfView *out, int64_t *arg) {
    EfResult r = ef_game_check(g, g->tile_cap, g->unit_cap);
    if (r.code == EF_OK) r = ef_other_view(g, t, out);
    *arg = r.arg;
    return r.code;
}
