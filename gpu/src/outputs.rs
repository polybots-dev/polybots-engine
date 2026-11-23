//! The batch's optional outputs on the host build (`kernel/outputs.h`), for the tests against
//! the CPU batch (`engine_fast_batch::outputs`).

use crate::state::{Block, EfGame};
use crate::tables::{DeviceTables, EfTables};
use crate::view::{EfView, Views};
use engine_fast::view::View;
use engine_fast_batch::outputs::{Plane, PROGRESSION_LEN};

extern "C" {
    fn ef_twin_queries(
        g: *const EfGame,
        t: *const EfTables,
        viewer: i32,
        planes: *mut i16,
        income: *mut i32,
        city_score: *mut i32,
        arg: *mut i64,
    ) -> i32;
    fn ef_twin_progression(
        g: *const EfGame,
        t: *const EfTables,
        viewer: i32,
        view: *mut EfView,
        out: *mut i32,
        arg: *mut i64,
    ) -> i32;
    fn ef_twin_progression_len() -> i32;
    fn ef_twin_planes() -> i32;
    fn ef_twin_other_view(
        g: *const EfGame,
        t: *const EfTables,
        out: *mut EfView,
        arg: *mut i64,
    ) -> i32;
}

/// What `kernel/outputs.h` computes from the game for one viewer: every `Plane` (in
/// `Plane::ALL` order), the income and the city score.
#[derive(Clone, Debug, PartialEq)]
pub struct Queries {
    pub planes: Vec<Vec<i16>>,
    pub income: i32,
    pub city_score: Vec<i32>,
}

impl Queries {
    pub fn plane(&self, p: Plane) -> &[i16] {
        &self.planes[p as usize]
    }
}

/// Every output from the game for `viewer`, by the host build. `Err` carries the engine's
/// reason.
pub fn twin_queries(g: &Block, t: &DeviceTables, viewer: i32) -> Result<Queries, String> {
    // SAFETY: a constant; the C side writes this many planes
    assert_eq!(unsafe { ef_twin_planes() } as usize, Plane::ALL.len());
    let n = g.tile_count.max(1) as usize;
    let mut planes = vec![0i16; Plane::ALL.len() * n];
    let (mut income, mut city_score) = (0, vec![0i32; n]);
    let mut arg = 0;
    // SAFETY: a whole block (as_ptr checks its capacities), one i16 per tile and plane, one
    // i32 and one i32 per tile.
    let code = unsafe {
        ef_twin_queries(
            g.as_ptr(),
            &*t.block,
            viewer,
            planes.as_mut_ptr(),
            &mut income,
            city_score.as_mut_ptr(),
            &mut arg,
        )
    };
    if code != 0 {
        return Err(t.error_text(code, arg));
    }
    Ok(Queries {
        planes: planes.chunks(n).map(|p| p.to_vec()).collect(),
        income,
        city_score,
    })
}

/// `viewer`'s progression row by the host build.
pub fn twin_progression(g: &Block, t: &DeviceTables, viewer: i32) -> Result<Vec<i32>, String> {
    // SAFETY: a constant
    assert_eq!(
        unsafe { ef_twin_progression_len() } as usize,
        PROGRESSION_LEN
    );
    let mut view = Views::new(1, g.tile_cap);
    let mut out = vec![0i32; PROGRESSION_LEN];
    let mut arg = 0;
    // SAFETY: a whole block, one view of its tile capacity and one row.
    let code = unsafe {
        ef_twin_progression(
            g.as_ptr(),
            &*t.block,
            viewer,
            view.as_mut_ptr(),
            out.as_mut_ptr(),
            &mut arg,
        )
    };
    if code != 0 {
        return Err(t.error_text(code, arg));
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
