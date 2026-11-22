//! The batch's optional outputs on the host build (`kernel/outputs.h`), for the tests against
//! the CPU batch (`engine_fast_batch::outputs`).

use crate::state::{Block, EfGame};
use crate::step::Stop;
use crate::tables::{DeviceTables, EfTables};
use crate::view::{EfView, Views};
use engine_fast::view::View;

extern "C" {
    fn ef_twin_defence(g: *const EfGame, t: *const EfTables, out: *mut i16, arg: *mut i64) -> i32;
    fn ef_twin_other_view(
        g: *const EfGame,
        t: *const EfTables,
        out: *mut EfView,
        arg: *mut i64,
    ) -> i32;
}

/// The defence plane for the player to move: one entry per tile, -1 where no unit has a panel.
pub fn twin_defence(g: &Block, t: &DeviceTables) -> Result<Vec<i16>, Stop> {
    let mut out = vec![0i16; g.tile_count.max(0) as usize];
    let mut arg = 0;
    // SAFETY: a whole block (as_ptr checks its capacities) and one i16 per tile.
    let code = unsafe { ef_twin_defence(g.as_ptr(), &*t.block, out.as_mut_ptr(), &mut arg) };
    if code != 0 {
        return Err(crate::menu::stop(t, code, arg));
    }
    Ok(out)
}

/// The view of the next seat after the player to move (`engine_fast::turns::other_seat`), by
/// the host build; None when there is no other seat. `Err` carries the engine's reason.
pub fn twin_other_view(g: &Block, t: &DeviceTables) -> Result<Option<View>, String> {
    let mut out = Views::new(1, g.tile_cap);
    let mut arg = 0;
    // SAFETY: a whole block; the C side writes one view of the block's tile capacity and one
    // argument.
    let code = unsafe { ef_twin_other_view(g.as_ptr(), &*t.block, out.as_mut_ptr(), &mut arg) };
    if code != 0 {
        return Err(t.error_text(code, arg));
    }
    (out.viewer(0) >= 0).then(|| out.to_view(0)).transpose()
}
