/* outputs.h - the batch's optional outputs beside the view and the legal list
 * (engine-fast/batch/src/outputs.rs): engine outputs every model may read. Each is
 * computed only when a caller asks. Some show what the view hides; engine-fast/src/queries.rs
 * lists which. */
#ifndef EF_OUTPUTS_H
#define EF_OUTPUTS_H

#include "menu.h"

/* queries.rs unit_stats, its defence plane: the defence bonus in tenths (10: none) of every unit
 * with an info panel for `viewer` (combat.rs visible_root), -1 (queries UNKNOWN) elsewhere, one
 * entry per tile. Can show a foreign unit's owner's techs. */
EF_BIG EfResult ef_defence_plane(const EfGame *g, const EfTables *t, int32_t viewer, int16_t *out) {
    for (int i = 0; i < g->tile_count; i++) {
        out[i] = -1;
        const EfUnit *u = ef_visible_root(g, &ef_ctiles(g)[i], viewer);
        if (!u) continue;
        if (!ef_unit_data(t, u->kind_n)) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
        int32_t bonus;
        EF_TRY(ef_defence_bonus(g, t, u, &bonus));
        out[i] = (int16_t)bonus; /* narrow: 10, 15 or 40 */
    }
    return ef_ok();
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
