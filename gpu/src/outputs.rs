//! The batch's optional outputs on the host build (`kernel/outputs.h`), for the tests against
//! the CPU batch (`engine_fast_batch::outputs`).

use crate::state::{Block, EfGame};
use crate::step::Stop;
use crate::tables::{DeviceTables, EfTables};

extern "C" {
    fn ef_twin_defence(g: *const EfGame, t: *const EfTables, out: *mut i16, arg: *mut i64) -> i32;
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
