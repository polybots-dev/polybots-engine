//! A batch of game blocks that stays in GPU memory (`kernel/device.cu`). Blocks are uploaded
//! into slots, kernels run over all slots, and outputs are read back when asked for.

use crate::state::{Block, EfGame};
use crate::step::EfMove;
use crate::tables::{DeviceTables, EfTables, KernelResult};
use crate::view::{EfView, Views};
use std::os::raw::c_void;

extern "C" {
    fn ef_cuda_create(
        games: i32,
        tile_cap: i32,
        unit_cap: i32,
        techs: i32,
        outputs: i32,
        error: *mut i32,
    ) -> *mut c_void;
    fn ef_cuda_exports(device: *mut c_void, x: *mut Exports) -> i32;
    fn ef_cuda_on_device(ptr: u64) -> i32;
    fn ef_cuda_step_device(device: *mut c_void, actions: u64, threads: i32, word: *mut u32) -> i32;
    fn ef_cuda_random_actions(device: *mut c_void, seed: u64, actions: u64, threads: i32) -> i32;
    fn ef_cuda_traffic(device: *mut c_void, to_device: *mut u64, to_host: *mut u64);
    fn ef_cuda_read(device: *mut c_void, from: u64, bytes: u64, to: *mut c_void) -> i32;
    fn ef_cuda_alloc(bytes: u64, ptr: *mut u64) -> i32;
    fn ef_cuda_free(ptr: u64);
    fn ef_cuda_peek(from: u64, bytes: u64, to: *mut c_void) -> i32;
    fn ef_cuda_poke(to: u64, bytes: u64, from: *const c_void) -> i32;
    fn ef_cuda_destroy(device: *mut c_void);
    fn ef_cuda_set_tables(device: *mut c_void, tables: *const EfTables) -> i32;
    fn ef_cuda_upload(
        device: *mut c_void,
        count: i32,
        slots: *const i32,
        games: *const EfGame,
    ) -> i32;
    fn ef_cuda_run_views(device: *mut c_void, viewers: *const i32, threads: i32) -> i32;
    fn ef_cuda_read_views(
        device: *mut c_void,
        views: *mut EfView,
        results: *mut KernelResult,
    ) -> i32;
    fn ef_cuda_memory(free: *mut u64, total: *mut u64) -> i32;
    fn ef_cuda_set_moves(device: *mut c_void, moves: *const EfMove) -> i32;
    fn ef_cuda_run_advance(device: *mut c_void, threads: i32) -> i32;
    fn ef_cuda_read_results(device: *mut c_void, results: *mut KernelResult) -> i32;
    fn ef_cuda_download(device: *mut c_void, slot: i32, game: *mut EfGame) -> i32;
    fn ef_cuda_run_slots(device: *mut c_void, index: *const i32, threads: i32) -> i32;
    fn ef_cuda_read_slots(
        device: *mut c_void,
        status: *mut i32,
        winners: *mut i32,
        results: *mut KernelResult,
        counts: *mut i32,
    ) -> i32;
    fn ef_cuda_read_rows(
        device: *mut c_void,
        offsets: *const i64,
        total: i64,
        rows: *mut crate::menu::Row,
        threads: i32,
    ) -> i32;
}

/// The device addresses and layouts of a batch's exported arrays (`kernel/device.cu`
/// EfExports): slot g's legal rows from `rows + g * menu_stride` (`counts[g]` rows of six i32),
/// its view planes from `tiles + g * view_stride` (`planes` planes of `cells` i16).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Exports {
    pub status: u64,
    pub winners: u64,
    pub counts: u64,
    pub applied: u64,
    pub rows: u64,
    pub tiles: u64,
    pub scalars: u64,
    pub players: u64,
    pub tech: u64,
    /// int16 x 4 per legal row (`max_rows` per slot), or 0 when the batch was made without
    pub previews: u64,
    /// int16 per tile (`cells` per slot), or 0 when the batch was made without
    pub defence: u64,
    pub menu_stride: u64,
    pub view_stride: u64,
    pub max_rows: i32,
    pub planes: i32,
    pub cells: i32,
    pub techs: i32,
}

/// The optional outputs a batch computes: each is
/// allocated and computed only when asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outputs {
    /// the attack preview of each legal row
    pub previews: bool,
    /// the defence-bonus plane of the units the mover sees
    pub defence: bool,
}

impl Outputs {
    fn flags(self) -> i32 {
        i32::from(self.previews) | (i32::from(self.defence) << 1)
    }
}

/// Whether `ptr` is memory on GPU 0 (a policy's output, before a device step reads it).
pub fn on_device(ptr: u64) -> bool {
    // SAFETY: the C side only asks the runtime about the address.
    unsafe { ef_cuda_on_device(ptr) != 0 }
}

/// Device memory outside any batch, for tests and benchmarks; freed on drop.
pub struct Buffer {
    pub ptr: u64,
    pub bytes: u64,
}

impl Buffer {
    pub fn new(bytes: u64) -> Result<Buffer, String> {
        let mut ptr = 0;
        // SAFETY: writes the new address.
        check(unsafe { ef_cuda_alloc(bytes.max(1), &mut ptr) })?;
        Ok(Buffer { ptr, bytes })
    }
    pub fn write<T: Copy>(&mut self, from: &[T]) -> Result<(), String> {
        assert!(std::mem::size_of_val(from) as u64 <= self.bytes);
        // SAFETY: the source slice and a buffer at least as large.
        check(unsafe {
            ef_cuda_poke(
                self.ptr,
                std::mem::size_of_val(from) as u64,
                from.as_ptr().cast(),
            )
        })
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: allocated by ef_cuda_alloc and freed once.
        unsafe { ef_cuda_free(self.ptr) }
    }
}

/// Copy `out.len()` values from device address `from` (tests and benchmarks; not counted in any
/// batch's traffic).
pub fn peek<T: Copy + Default>(from: u64, out: &mut [T]) -> Result<(), String> {
    // SAFETY: the C side writes exactly the slice's bytes from device memory the caller names.
    check(unsafe {
        ef_cuda_peek(
            from,
            std::mem::size_of_val(out) as u64,
            out.as_mut_ptr().cast(),
        )
    })
}

/// Per slot after `run_slots`: status, winner, the kernel's result and the number of rows.
pub struct SlotResults {
    pub status: Vec<i32>,
    pub winners: Vec<i32>,
    pub results: Vec<KernelResult>,
    pub counts: Vec<i32>,
}

fn check(code: i32) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("CUDA error {code}"))
    }
}

/// Free and total memory of GPU 0, in bytes.
pub fn memory() -> Result<(u64, u64), String> {
    let (mut free, mut total) = (0, 0);
    // SAFETY: the C side writes the two values.
    check(unsafe { ef_cuda_memory(&mut free, &mut total) })?;
    Ok((free, total))
}

pub struct DeviceBatch {
    handle: *mut c_void,
    games: usize,
    /// every slot's capacities: tiles, units
    caps: (i32, i32),
}

// SAFETY: the handle is owned by this value alone and every call through it takes `&mut
// self` (the `&self` methods read only `games`), so it may move to and be shared with other
// threads. kernel/device.cu uses the CUDA runtime API only, whose primary context is shared by
// every thread of the process; a context made current per thread would end this.
unsafe impl Send for DeviceBatch {}
unsafe impl Sync for DeviceBatch {}

impl DeviceBatch {
    /// Device memory for `games` slots of blocks with room for `caps` (tiles, units), all
    /// zero, with the rule tables uploaded.
    pub fn new(
        games: usize,
        caps: (i32, i32),
        tables: &DeviceTables,
    ) -> Result<DeviceBatch, String> {
        DeviceBatch::with_outputs(games, caps, 0, Outputs::default(), tables)
    }

    /// As `new`, with room for `techs` technology ids per slot in the exported arrays and the
    /// optional `outputs`.
    pub fn with_outputs(
        games: usize,
        caps: (i32, i32),
        techs: usize,
        outputs: Outputs,
        tables: &DeviceTables,
    ) -> Result<DeviceBatch, String> {
        let mut error = 0;
        let games_i = i32::try_from(games).map_err(|_| "too many games")?;
        let techs = i32::try_from(techs).map_err(|_| "too many technologies")?;
        // SAFETY: the C side returns a handle it owns, or null with the error code set.
        let handle =
            unsafe { ef_cuda_create(games_i, caps.0, caps.1, techs, outputs.flags(), &mut error) };
        if handle.is_null() {
            return Err(format!("CUDA error {error}"));
        }
        let batch = DeviceBatch {
            handle,
            games,
            caps,
        };
        // SAFETY: a live handle and a valid tables block.
        check(unsafe { ef_cuda_set_tables(handle, &*tables.block) })?;
        Ok(batch)
    }

    pub fn len(&self) -> usize {
        self.games
    }

    /// Every slot's capacities: tiles, units.
    pub fn caps(&self) -> (i32, i32) {
        self.caps
    }

    pub fn is_empty(&self) -> bool {
        self.games == 0
    }

    /// Put a block of the batch's capacities into a slot.
    pub fn upload(&mut self, slot: usize, game: &Block) -> Result<(), String> {
        self.upload_many(&[(slot, game)])
    }

    /// Put blocks of the batch's capacities into distinct slots, in one copy and one check.
    pub fn upload_many(&mut self, games: &[(usize, &Block)]) -> Result<(), String> {
        let mut slots = Vec::with_capacity(games.len());
        let mut bytes = Vec::with_capacity(games.iter().map(|(_, g)| g.bytes().len()).sum());
        for &(slot, game) in games {
            assert_eq!(
                (game.tile_cap, game.unit_cap),
                self.caps,
                "a block of the batch's capacities"
            );
            if slots.contains(&(slot as i32)) {
                return Err("a slot uploaded twice in one call".into());
            }
            slots.push(i32::try_from(slot).map_err(|_| "slot out of range")?);
            bytes.extend_from_slice(game.bytes());
        }
        let count = i32::try_from(slots.len()).map_err(|_| "too many blocks")?;
        // SAFETY: a live handle, `count` slots and `count` contiguous blocks of the batch's
        // capacities (bytes() is the whole block, 8-aligned in size); the C side checks the slots.
        check(unsafe {
            ef_cuda_upload(
                self.handle,
                count,
                slots.as_ptr(),
                bytes.as_ptr().cast::<EfGame>(),
            )
        })
    }

    /// Write every slot's view for its viewer on the device and wait for the kernel.
    pub fn run_views(&mut self, viewers: &[i32], threads_per_block: usize) -> Result<(), String> {
        assert_eq!(viewers.len(), self.games);
        // SAFETY: a live handle and one viewer per slot.
        check(unsafe { ef_cuda_run_views(self.handle, viewers.as_ptr(), threads_per_block as i32) })
    }

    /// The views of the last run and, per slot, the kernel's result code and argument.
    pub fn read_views(&mut self) -> Result<(Views, Vec<KernelResult>), String> {
        let mut views = Views::new(self.games, self.caps.0);
        let mut results = vec![KernelResult::default(); self.games];
        // SAFETY: `views` has room for one view of the batch's tile capacity per slot, which
        // the C side writes; `results` has one result per slot.
        check(unsafe {
            ef_cuda_read_views(self.handle, views.as_mut_ptr(), results.as_mut_ptr())
        })?;
        Ok((views, results))
    }
}

impl DeviceBatch {
    /// The move each slot takes on the next `run_advance`.
    pub fn set_moves(&mut self, moves: &[EfMove]) -> Result<(), String> {
        assert_eq!(moves.len(), self.games);
        // SAFETY: a live handle and one move per slot.
        check(unsafe { ef_cuda_set_moves(self.handle, moves.as_ptr()) })
    }

    /// One decision in every slot whose last check passed, waiting for the kernel; the blocks
    /// stay on the device.
    pub fn run_advance(&mut self, threads_per_block: usize) -> Result<(), String> {
        // SAFETY: a live handle.
        check(unsafe { ef_cuda_run_advance(self.handle, threads_per_block as i32) })
    }

    /// The kernel's result per slot from the last `run_advance`.
    pub fn read_results(&mut self) -> Result<Vec<KernelResult>, String> {
        let mut out = vec![KernelResult::default(); self.games];
        // SAFETY: a live handle and one result per slot.
        check(unsafe { ef_cuda_read_results(self.handle, out.as_mut_ptr()) })?;
        Ok(out)
    }

    /// One slot's block.
    pub fn download(&mut self, slot: usize) -> Result<Block, String> {
        let mut g = Block::new(self.caps.0, self.caps.1);
        let slot = i32::try_from(slot).map_err(|_| "slot out of range")?;
        // SAFETY: a live handle and one block of the batch's capacities to write.
        check(unsafe { ef_cuda_download(self.handle, slot, g.as_mut_ptr()) })?;
        Ok(g)
    }
}

impl DeviceBatch {
    /// Run every slot by its entry of `index` (`kernel/device.cu` k_slots: -2 refresh after an
    /// upload, -1 leave, k take row k) and wait.
    pub fn run_slots(&mut self, index: &[i32], threads_per_block: usize) -> Result<(), String> {
        assert_eq!(index.len(), self.games);
        // SAFETY: a live handle and one entry per slot.
        check(unsafe { ef_cuda_run_slots(self.handle, index.as_ptr(), threads_per_block as i32) })
    }

    pub fn read_slots(&mut self) -> Result<SlotResults, String> {
        let n = self.games;
        let mut out = SlotResults {
            status: vec![0; n],
            winners: vec![0; n],
            results: vec![KernelResult::default(); n],
            counts: vec![0; n],
        };
        // SAFETY: a live handle and one entry per slot in each array.
        check(unsafe {
            ef_cuda_read_slots(
                self.handle,
                out.status.as_mut_ptr(),
                out.winners.as_mut_ptr(),
                out.results.as_mut_ptr(),
                out.counts.as_mut_ptr(),
            )
        })?;
        Ok(out)
    }

    /// Every slot's rows from the last run, packed in slot order (`counts` from `read_slots`).
    pub fn read_rows(
        &mut self,
        counts: &[i32],
        threads_per_block: usize,
    ) -> Result<Vec<crate::menu::Row>, String> {
        assert_eq!(counts.len(), self.games);
        let mut offsets = Vec::with_capacity(self.games);
        let mut total = 0i64;
        for &c in counts {
            offsets.push(total);
            total += i64::from(c);
        }
        let mut rows = vec![[0i32; 6]; total as usize];
        // SAFETY: a live handle, one offset per slot, room for `total` rows.
        check(unsafe {
            ef_cuda_read_rows(
                self.handle,
                offsets.as_ptr(),
                total,
                rows.as_mut_ptr(),
                threads_per_block as i32,
            )
        })?;
        Ok(rows)
    }
}

impl DeviceBatch {
    /// The exported arrays' device addresses and layouts (fixed while the batch lives).
    pub fn exports(&mut self) -> Exports {
        let mut x = Exports::default();
        // SAFETY: a live handle; the C side fills the struct.
        unsafe { ef_cuda_exports(self.handle, &mut x) };
        x
    }

    /// One decision per slot from `actions`, one i32 per slot in GPU memory (-1 skips), and
    /// the status word: slots not playing afterwards, slots that stopped in this call. Refused
    /// unless `actions` is device memory of at least `bytes` bytes holding one i32 per slot.
    pub fn step_device(
        &mut self,
        actions: u64,
        bytes: u64,
        threads_per_block: usize,
    ) -> Result<[u32; 2], String> {
        if bytes < 4 * self.games as u64 || !on_device(actions) || !actions.is_multiple_of(4) {
            return Err("actions: one aligned i32 per slot in GPU 0 memory".into());
        }
        let mut word = [0u32; 2];
        // SAFETY: a live handle, actions checked above, two words written.
        check(unsafe {
            ef_cuda_step_device(
                self.handle,
                actions,
                threads_per_block as i32,
                word.as_mut_ptr(),
            )
        })?;
        Ok(word)
    }

    /// The benchmark's policy (a random legal row per playing slot, never Resign) into
    /// `actions`, device memory of one i32 per slot.
    pub fn random_actions(
        &mut self,
        seed: u64,
        actions: &Buffer,
        threads_per_block: usize,
    ) -> Result<(), String> {
        assert!(actions.bytes >= 4 * self.games as u64);
        // SAFETY: a live handle and a device buffer of one i32 per slot.
        check(unsafe {
            ef_cuda_random_actions(self.handle, seed, actions.ptr, threads_per_block as i32)
        })
    }

    /// Copy `out.len()` values from `from`, an address inside one of the batch's exported
    /// arrays, to the host (counted in `traffic`).
    pub fn read<T: Copy>(&mut self, from: u64, out: &mut [T]) -> Result<(), String> {
        // SAFETY: a live handle; the C side writes exactly the slice's bytes.
        check(unsafe {
            ef_cuda_read(
                self.handle,
                from,
                std::mem::size_of_val(out) as u64,
                out.as_mut_ptr().cast(),
            )
        })
    }

    /// The bytes this batch's calls have copied to and from the host so far.
    pub fn traffic(&self) -> (u64, u64) {
        let (mut to_device, mut to_host) = (0, 0);
        // SAFETY: a live handle; two values written.
        unsafe { ef_cuda_traffic(self.handle, &mut to_device, &mut to_host) };
        (to_device, to_host)
    }
}

impl Drop for DeviceBatch {
    fn drop(&mut self) {
        // SAFETY: the handle came from ef_cuda_create and is destroyed once.
        unsafe { ef_cuda_destroy(self.handle) }
    }
}
