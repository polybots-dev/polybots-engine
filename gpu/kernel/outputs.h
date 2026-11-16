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

#endif /* EF_OUTPUTS_H */
