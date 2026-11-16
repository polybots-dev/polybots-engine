/* status.h - a batch slot's status after a reset or a step (engine-fast/batch/src/lib.rs
  * Slot::refresh, with queries.rs winner): ended with its winner, truncated at the
 * decision limit, or playing with the mover's view and legal list; a player to move with no
 * legal move stops the game. */
#ifndef EF_STATUS_H
#define EF_STATUS_H

#include "menu.h"
#include "view.h"

/* batch Status */
enum { EF_EMPTY = 0, EF_PLAYING = 1, EF_ENDED_STATUS = 2, EF_TRUNCATED = 3, EF_REFUSED = 4, EF_FAILED = 5 };

/* queries.rs winner: *has = 1 and the winner's id where a resignation or the game's end decides
 * it. The engine runs turns.rs winner on a copy; its one write (each player's capital count
 * cache) is undone here. */
EF_BIG EfResult ef_frame_winner(EfGame *g, int *has, int32_t *winner) {
    *has = 0;
    *winner = 0;
    int32_t playable[EF_MAX_PLAYERS];
    int n = 0;
    for (int k = 0; k < g->player_count; k++)
        if (g->players[k].id != EF_NATURE) playable[n++] = g->players[k].id;
    for (int k = 0; k < g->player_count; k++) {
        if (g->players[k].resigned_at_command < 0) continue;
        if (n != 2) return ef_err(EF_ERR_RESIGN_BEYOND_TWO, 0);
        for (int q = 0; q < n; q++)
            if (playable[q] != g->players[k].id) {
                *has = 1;
                *winner = playable[q];
                break;
            }
        return ef_ok();
    }
    if (g->current_state != EF_ENDED) return ef_ok();
    int64_t cache[EF_MAX_PLAYERS];
    for (int k = 0; k < g->player_count; k++) cache[k] = g->players[k].capital_count_cache;
    int32_t selected;
    int completed;
    EfResult r = ef_winner(g, &selected, &completed);
    for (int k = 0; k < g->player_count; k++) g->players[k].capital_count_cache = cache[k];
    EF_TRY(r);
    *has = 1;
    *winner = selected;
    return ef_ok();
}

/* batch Slot::refresh: the status, the winner where it ended, and where it is playing the
 * mover's view (when `view` is not 0) and the legal list */
EF_BIG EfResult ef_refresh(EfGame *g, const EfTables *t, EfMenuWork *w, EfMenu *m, EfView *view, int32_t *status,
                           int32_t *winner) {
    m->len = 0;
    int has;
    EF_TRY(ef_frame_winner(g, &has, winner));
    if (has) {
        *status = EF_ENDED_STATUS;
        return ef_ok();
    }
    *winner = 0;
    if (g->decisions == (uint64_t)g->action_limit) {
        *status = EF_TRUNCATED;
        return ef_ok();
    }
    int32_t mover = g->players[g->current_player_index].id;
    if (view) EF_TRY(ef_view_write(g, t, mover, view));
    EF_TRY(ef_menu(g, t, w, m));
    if (m->len == 0) return ef_err(EF_ERR_NO_LEGAL_MOVE, 0);
    *status = EF_PLAYING;
    return ef_ok();
}

/* the move a batch row stands for (batch/src/lib.rs as_move read back): the peace reply's
 * option is its accept flag */
EF_FN void ef_move_of_row(const EfGame *g, const EfRow *r, EfMove *m) {
    memset(m, 0, sizeof *m);
    m->kind = r->f[0];
    m->accept = -1;
    if (r->f[1] >= 0) {
        m->has_source = 1;
        m->source = ef_xy(r->f[1] % g->width, r->f[1] / g->width);
    }
    if (r->f[2] >= 0) {
        m->has_target = 1;
        m->target = ef_xy(r->f[2] % g->width, r->f[2] / g->width);
    }
    if (r->f[0] == EF_K_PEACE_REPLY) {
        m->accept = r->f[3];
    } else if (r->f[3] >= 0) {
        m->has_option = 1;
        m->option = r->f[3];
    }
    if (r->f[4] >= 0) {
        m->has_opponent = 1;
        m->opponent = r->f[4];
    }
}

/* batch Batch::step for one slot that is playing: the chosen row's move, then the refresh */
EF_BIG EfResult ef_slot_step(EfGame *g, const EfTables *t, EfMenuWork *w, EfMenu *m, EfView *view, int32_t index,
                             int32_t *status, int32_t *winner) {
    if (index < 0 || index >= m->len) return ef_err(EF_ERR_INVARIANT, __LINE__);
    EfMove move;
    ef_move_of_row(g, &m->rows[index], &move);
    int32_t elapsed;
    EF_TRY(ef_advance(g, t, &move, &elapsed));
    return ef_refresh(g, t, w, m, view, status, winner);
}

#endif /* EF_STATUS_H */
