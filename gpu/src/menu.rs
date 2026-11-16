//! The legal list of the kernel sources (`kernel/menu.h`): the batch's rows of a game's moves,
//! sorted, as `engine_fast_batch::Move` holds them.

use crate::state::EfGame;
use crate::step::Stop;
use crate::tables::{DeviceTables, EfTables};

/// One legal move: kind, source, target, option, opponent, price (batch `MOVE_FIELDS`).
pub type Row = [i32; 6];

extern "C" {
    fn ef_twin_menu(
        g: *const EfGame,
        t: *const EfTables,
        work: *mut u64,
        menu: *mut u64,
        arg: *mut i64,
    ) -> i32;
    fn ef_twin_menu_sizes(work: *mut i64, menu: *mut i64);
    fn ef_twin_previews(
        g: *const EfGame,
        t: *const EfTables,
        menu: *const u64,
        out: *mut i16,
        arg: *mut i64,
    ) -> i32;
    fn ef_twin_slot(
        g: *mut EfGame,
        t: *const EfTables,
        work: *mut u64,
        menu: *mut u64,
        view: *mut crate::view::EfView,
        index: i32,
        status: *mut i32,
        winner: *mut i32,
        arg: *mut i64,
    ) -> i32;
    fn ef_twin_error_failed(code: i32) -> i32;
}

/// One batch slot on the host build (`kernel/status.h`): its legal list kept between calls,
/// as the device keeps it.
pub struct TwinSlot {
    work: Vec<u64>,
    menu: Vec<u64>,
    /// sized for the last block run, which a block of other capacities replaces
    view: crate::view::Views,
}

impl Default for TwinSlot {
    fn default() -> TwinSlot {
        let (mut work, mut menu) = (0i64, 0i64);
        // SAFETY: writes two integers.
        unsafe { ef_twin_menu_sizes(&mut work, &mut menu) };
        TwinSlot {
            work: vec![0; (work as usize).div_ceil(8)],
            menu: vec![0; (menu as usize).div_ceil(8)],
            view: crate::view::Views::new(0, 1),
        }
    }
}

impl TwinSlot {
    /// The slot's status and winner after a reset (`index` None) or after taking its legal
    /// row `index`; Err where the game stopped.
    pub fn run(
        &mut self,
        g: &mut crate::state::Block,
        t: &DeviceTables,
        index: Option<usize>,
    ) -> Result<(i32, i32), Stop> {
        if self.view.is_empty() || self.view.tile_cap() != g.tile_cap {
            self.view = crate::view::Views::new(1, g.tile_cap);
        }
        let (mut status, mut winner, mut arg) = (0, 0, 0);
        let index = index.map_or(-1, |i| i as i32);
        // SAFETY: a whole block; the buffers are the C sizes, the view the block's tile capacity.
        let code = unsafe {
            ef_twin_slot(
                g.as_mut_ptr(),
                &*t.block,
                self.work.as_mut_ptr(),
                self.menu.as_mut_ptr(),
                self.view.as_mut_ptr(),
                index,
                &mut status,
                &mut winner,
                &mut arg,
            )
        };
        if code != 0 {
            // SAFETY: no side effects.
            let failed = unsafe { ef_twin_error_failed(code) } != 0;
            return Err(Stop {
                code,
                arg,
                text: t.error_text(code, arg),
                failed,
            });
        }
        Ok((status, winner))
    }

    /// The view of the player to move from the last run that left the slot playing.
    pub fn view(&self) -> Result<engine_fast::view::View, String> {
        self.view.to_view(0)
    }

    /// The legal rows from the last run.
    pub fn rows(&self) -> Vec<Row> {
        rows(&self.menu)
    }
}

/// The sorted legal rows of the game in `g`, on the host build.
pub fn twin_menu(g: &crate::state::Block, t: &DeviceTables) -> Result<Vec<Row>, Stop> {
    twin_menu_with(g, t, false).map(|(rows, _)| rows)
}

/// `twin_menu` and the attack preview of each row (`kernel/menu.h` ef_menu_previews; the CPU
/// batch's `previews`).
pub fn twin_menu_previews(
    g: &crate::state::Block,
    t: &DeviceTables,
) -> Result<(Vec<Row>, Vec<[i16; 4]>), Stop> {
    twin_menu_with(g, t, true)
}

/// A kernel stop with its text.
pub(crate) fn stop(t: &DeviceTables, code: i32, arg: i64) -> Stop {
    // SAFETY: no side effects.
    let failed = unsafe { ef_twin_error_failed(code) } != 0;
    Stop {
        code,
        arg,
        text: t.error_text(code, arg),
        failed,
    }
}

fn twin_menu_with(
    g: &crate::state::Block,
    t: &DeviceTables,
    previews: bool,
) -> Result<(Vec<Row>, Vec<[i16; 4]>), Stop> {
    let (mut work_size, mut menu_size) = (0i64, 0i64);
    // SAFETY: writes two integers.
    unsafe { ef_twin_menu_sizes(&mut work_size, &mut menu_size) };
    // u64 words keep the C structs' alignment
    let mut work = vec![0u64; (work_size as usize).div_ceil(8)];
    let mut menu = vec![0u64; (menu_size as usize).div_ceil(8)];
    let mut arg = 0;
    // SAFETY: valid blocks; the buffers are at least the C sizes.
    let code = unsafe {
        ef_twin_menu(
            g.as_ptr(),
            &*t.block,
            work.as_mut_ptr(),
            menu.as_mut_ptr(),
            &mut arg,
        )
    };
    if code != 0 {
        return Err(stop(t, code, arg));
    }
    let rows = rows(&menu);
    let mut out = vec![[0i16; 4]; if previews { rows.len() } else { 0 }];
    if previews {
        // SAFETY: the list the call above made for this block, and four i16 per row.
        let code = unsafe {
            ef_twin_previews(
                g.as_ptr(),
                &*t.block,
                menu.as_ptr(),
                out.as_mut_ptr().cast(),
                &mut arg,
            )
        };
        if code != 0 {
            return Err(stop(t, code, arg));
        }
    }
    Ok((rows, out))
}

#[cfg(feature = "cuda")]
extern "C" {
    fn ef_cuda_menus(
        games: *const EfGame,
        count: i32,
        tile_cap: i32,
        unit_cap: i32,
        t: *const EfTables,
        menus: *mut u64,
        results: *mut crate::tables::KernelResult,
    ) -> i32;
}

/// The legal list of each block on the GPU: per block the kernel's result and its rows
/// (empty where it stopped).
#[cfg(feature = "cuda")]
pub fn device_menus(
    games: &[crate::state::Block],
    t: &DeviceTables,
) -> Result<Vec<(crate::tables::KernelResult, Vec<Row>)>, String> {
    let (flat, (tile_cap, unit_cap)) = crate::state::pack(games);
    let (mut work_size, mut menu_size) = (0i64, 0i64);
    // SAFETY: writes two integers.
    unsafe { ef_twin_menu_sizes(&mut work_size, &mut menu_size) };
    let words = menu_size as usize / 8;
    let mut menus = vec![0u64; games.len() * words];
    let mut results = vec![crate::tables::KernelResult::default(); games.len()];
    // SAFETY: `flat` holds the blocks; `menus` one EfMenu per block; `results` one per block.
    let code = unsafe {
        ef_cuda_menus(
            flat.as_ptr().cast(),
            games.len() as i32,
            tile_cap,
            unit_cap,
            &*t.block,
            menus.as_mut_ptr(),
            results.as_mut_ptr(),
        )
    };
    if code != 0 {
        return Err(format!("CUDA error {code}"));
    }
    Ok(results
        .into_iter()
        .enumerate()
        .map(|(k, r)| (r, rows(&menus[k * words..(k + 1) * words])))
        .collect())
}

/// The rows of one EfMenu (len, pad, then rows of six i32).
fn rows(menu: &[u64]) -> Vec<Row> {
    // SAFETY: the words hold an EfMenu's i32 fields.
    let words: &[i32] = unsafe { std::slice::from_raw_parts(menu.as_ptr().cast(), menu.len() * 2) };
    let len = words[0] as usize;
    words[2..2 + len * 6]
        .as_chunks::<6>()
        .0
        .iter()
        .map(|r| [r[0], r[1], r[2], r[3], r[4], r[5]])
        .collect()
}
