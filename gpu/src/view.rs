//! The view written by the kernel sources (`kernel/view.h`), as the Rust engine's `View`.

use crate::state::{EfGame, MAX_PLAYERS, MAX_TASKS, MAX_TECH, MAX_TILES};
#[cfg(feature = "cuda")]
use crate::tables::KernelResult;
use crate::tables::{DeviceTables, EfTables};
use engine_fast::view::{LighthouseView, PlayerView, TaskView, View, TILE_PLANES};
use std::mem::size_of;

pub const PLANES: usize = 29;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EfPlayerView {
    pub seat: i16,
    pub tribe: i16,
    pub relation: i16,
    pub pad: i16,
    pub score: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EfTaskView {
    pub kind: i16,
    pub completed: i16,
    pub counter: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EfLighthouseView {
    pub tile: i16,
    pub explorers: [i16; 5],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EfColorView {
    pub seat: i16,
    pub pad: [i16; 3],
    pub color: i64,
}

#[repr(C)]
pub struct EfView {
    pub viewer: i32,
    pub turn: i32,
    pub width: i32,
    pub height: i32,
    pub stars: i32,
    pub tech_len: i32,
    pub players_len: i32,
    pub tile_count: i32,
    pub rules: i32,
    pub mode: i32,
    pub revealed_resources: u32,
    pub start_tile: i32,
    pub units_lost: i32,
    pub monuments_len: i32,
    pub tasks_len: i32,
    pub lighthouses_len: i32,
    pub colors_len: i32,
    pub pad: i32,
    pub players: [EfPlayerView; MAX_PLAYERS],
    pub colors: [EfColorView; MAX_PLAYERS],
    pub tasks: [EfTaskView; MAX_TASKS],
    pub lighthouses: [EfLighthouseView; 4],
    pub monuments: [i16; 8],
    pub tech: [i16; MAX_TECH],
}

/// kernel/view.h's layout: the header, then the tile planes with room for `tile_cap` tiles,
/// padded to 8 bytes.
pub fn view_size(tile_cap: i32) -> usize {
    (size_of::<EfView>() + PLANES * tile_cap as usize * 2).div_ceil(8) * 8
}

/// Views end to end, each with room for `tile_cap` tiles: what a launch writes for blocks of
/// that tile capacity.
pub struct Views {
    words: Vec<u64>,
    tile_cap: i32,
}

impl Views {
    pub fn new(count: usize, tile_cap: i32) -> Views {
        assert!((1..=MAX_TILES as i32).contains(&tile_cap));
        Views {
            words: vec![0; count * view_size(tile_cap) / 8],
            tile_cap,
        }
    }

    pub fn len(&self) -> usize {
        self.words.len() / (view_size(self.tile_cap) / 8)
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn tile_cap(&self) -> i32 {
        self.tile_cap
    }

    /// The whole buffer for the C side, which writes `len` views of `view_size(tile_cap)`.
    pub fn as_mut_ptr(&mut self) -> *mut EfView {
        self.words.as_mut_ptr().cast()
    }

    /// View `k`, as the engine's `View`.
    pub fn to_view(&self, k: usize) -> Result<View, String> {
        let words = view_size(self.tile_cap) / 8;
        let at = &self.words[k * words..(k + 1) * words];
        // SAFETY: the header is at the start of the view, aligned to 8 bytes (the buffer holds
        // u64 words), integers only; the planes follow it, aligned to 2.
        let (header, tiles) = unsafe {
            let header = &*at.as_ptr().cast::<EfView>();
            let tiles = std::slice::from_raw_parts(
                at.as_ptr()
                    .cast::<u8>()
                    .add(size_of::<EfView>())
                    .cast::<i16>(),
                PLANES * self.tile_cap as usize,
            );
            (header, tiles)
        };
        header.to_view(tiles)
    }
}

impl EfView {
    /// The header and its tile planes as the engine's `View`.
    fn to_view(&self, planes: &[i16]) -> Result<View, String> {
        let n = usize::try_from(self.width * self.height)
            .ok()
            .filter(|&n| PLANES * n <= planes.len());
        let n = n.ok_or("view size out of range")?;
        let tech_len = usize::try_from(self.tech_len)
            .ok()
            .filter(|&n| n <= MAX_TECH)
            .ok_or("tech count")?;
        let players_len = usize::try_from(self.players_len)
            .ok()
            .filter(|&n| n <= MAX_PLAYERS)
            .ok_or("player count")?;
        let count = |len: i32, max: usize, what: &str| {
            usize::try_from(len)
                .ok()
                .filter(|&n| n <= max)
                .ok_or_else(|| what.to_owned())
        };
        let monuments = count(self.monuments_len, 8, "monument count")?;
        let tasks = count(self.tasks_len, MAX_TASKS, "task count")?;
        let lighthouses = count(self.lighthouses_len, 4, "lighthouse count")?;
        let colors = count(self.colors_len, MAX_PLAYERS, "colour count")?;
        Ok(View {
            viewer: self.viewer as i16,
            turn: self.turn,
            width: self.width as i16,
            height: self.height as i16,
            rules: self.rules as i16,
            mode: self.mode as i16,
            tiles: planes[..PLANES * n].to_vec(),
            stars: self.stars,
            tech: self.tech[..tech_len].to_vec(),
            revealed_resources: self.revealed_resources,
            start_tile: self.start_tile as i16,
            units_lost: self.units_lost,
            monuments: self.monuments[..monuments].to_vec(),
            tasks: self.tasks[..tasks]
                .iter()
                .map(|t| TaskView {
                    kind: t.kind,
                    completed: t.completed != 0,
                    counter: t.counter,
                })
                .collect(),
            lighthouses: self.lighthouses[..lighthouses]
                .iter()
                .map(|l| LighthouseView {
                    tile: l.tile,
                    explorers: l.explorers,
                })
                .collect(),
            colors: self.colors[..colors]
                .iter()
                .map(|c| (c.seat, c.color))
                .collect(),
            players: self.players[..players_len]
                .iter()
                .map(|p| PlayerView {
                    seat: p.seat,
                    tribe: p.tribe,
                    relation: p.relation,
                    score: p.score,
                })
                .collect(),
        })
    }
}

extern "C" {
    fn ef_twin_view_size(tile_cap: i32) -> i64;
    fn ef_twin_view(
        g: *const EfGame,
        t: *const EfTables,
        viewer: i32,
        out: *mut EfView,
        arg: *mut i64,
    ) -> i32;
}

#[cfg(feature = "cuda")]
extern "C" {
    fn ef_cuda_views(
        games: *const EfGame,
        count: i32,
        tile_cap: i32,
        unit_cap: i32,
        tables: *const EfTables,
        viewers: *const i32,
        views: *mut EfView,
        results: *mut KernelResult,
    ) -> i32;
}

/// The C compiler's size of a view with room for `tile_cap` tiles.
pub fn twin_size(tile_cap: i32) -> usize {
    // SAFETY: no side effects.
    unsafe { ef_twin_view_size(tile_cap) as usize }
}

/// The kernel sources' plane count must be the engine's.
pub fn planes_match() -> bool {
    TILE_PLANES.len() == PLANES
}

/// `viewer`'s view of the game, by the host build; `Err` carries the engine's reason.
pub fn twin(g: &crate::state::Block, t: &DeviceTables, viewer: i32) -> Result<View, String> {
    let mut out = Views::new(1, g.tile_cap);
    let mut arg = 0;
    // SAFETY: a whole block; the C side writes one view of the block's tile capacity and one
    // argument.
    let code = unsafe { ef_twin_view(g.as_ptr(), &*t.block, viewer, out.as_mut_ptr(), &mut arg) };
    if code != 0 {
        return Err(t.error_text(code, arg));
    }
    out.to_view(0)
}

/// Each game's view for its viewer, written on the GPU. The outer `Err` is a CUDA failure.
#[cfg(feature = "cuda")]
pub fn device(
    games: &[crate::state::Block],
    t: &DeviceTables,
    viewers: &[i32],
) -> Result<Vec<Result<View, String>>, String> {
    assert_eq!(games.len(), viewers.len());
    let (flat, (tile_cap, unit_cap)) = crate::state::pack(games);
    let mut views = Views::new(games.len(), tile_cap.max(1));
    let mut results = vec![KernelResult::default(); games.len()];
    // SAFETY: `flat` holds `count` game blocks of these capacities, `views` room for `count`
    // views of their tile capacity, and `results` one result per game.
    let code = unsafe {
        ef_cuda_views(
            flat.as_ptr().cast(),
            games.len() as i32,
            tile_cap,
            unit_cap,
            &*t.block,
            viewers.as_ptr(),
            views.as_mut_ptr(),
            results.as_mut_ptr(),
        )
    };
    if code != 0 {
        return Err(format!("CUDA error {code}"));
    }
    Ok(results
        .into_iter()
        .enumerate()
        .map(|(k, r)| {
            if r.code == 0 {
                views.to_view(k)
            } else {
                Err(t.error_text(r.code, r.arg))
            }
        })
        .collect())
}
