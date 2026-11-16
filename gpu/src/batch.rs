//! The batch interface on the GPU: the same
//! slots, statuses, legal rows and views as `engine_fast_batch::Batch`, with the games kept
//! in GPU memory and every step, status, view and legal list run there (`kernel/status.h`).
//! Games start on the CPU (`reset` on `workers` threads) and are uploaded.
//!
//! Where it cannot match the CPU batch: a slot the engine fails on during a step (a broken
//! invariant) is failed here too, but its reason is the kernel's text, not the Rust panic
//! message (a reset runs the engine itself and keeps it); a game
//! larger than the device block (units, actions, legal rows) stops with a device reason where
//! the CPU batch plays on. `game` rebuilds the state from the block with the city names and
//! viewer memory of the game's start (the block holds neither).

use crate::device::{DeviceBatch, Outputs};
use crate::menu::Row;
use crate::state::{self as device};
use crate::step;
use crate::tables::DeviceTables;
use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::view::View;
use engine_fast_batch::{Applied, Status, SKIP};
use std::panic::{catch_unwind, AssertUnwindSafe};

const THREADS: usize = 32;

#[derive(Default)]
struct Slot {
    status: Status,
    reason: String,
    winner: i32,
    moves: Vec<Row>,
    /// the game as it started, for `game`'s parts the block does not hold
    start: Option<Game>,
}

pub struct GpuBatch {
    catalog: String,
    tables: Tables,
    flat: DeviceTables,
    device: DeviceBatch,
    slots: Vec<Slot>,
    workers: usize,
    /// the views of the last run, read from the device when first asked for
    views: Option<Vec<Option<View>>>,
    /// device steps ran since the slots above were last read from the device (`sync`)
    stale: bool,
    /// a game on a board smaller than the batch's largest was started: the exported planes
    /// (laid out per board) no longer have one layout, so the device arrays are refused
    mixed_boards: bool,
}

fn status_of(code: i32) -> Status {
    match code {
        1 => Status::Playing,
        2 => Status::Ended,
        3 => Status::Truncated,
        _ => Status::Failed,
    }
}

impl GpuBatch {
    /// `games` empty slots on GPU 0 with room for boards up to `max_side` x `max_side`, games
    /// started on `workers` CPU threads.
    pub fn new(
        catalog_json: &str,
        games: usize,
        workers: usize,
        max_side: usize,
    ) -> Result<GpuBatch, String> {
        GpuBatch::with_outputs(catalog_json, games, workers, max_side, Outputs::default())
    }

    /// As `new`, computing the optional `outputs` too (`exports().previews`, `.defence`).
    pub fn with_outputs(
        catalog_json: &str,
        games: usize,
        workers: usize,
        max_side: usize,
        outputs: Outputs,
    ) -> Result<GpuBatch, String> {
        if !(1..=device::MAX_SIDE).contains(&max_side) {
            return Err(format!(
                "the gpu backend holds boards up to {0} x {0}",
                device::MAX_SIDE
            ));
        }
        let tables = Tables::compile(catalog_json)?;
        let flat = DeviceTables::new(&tables)?;
        let device = DeviceBatch::with_outputs(
            games,
            device::default_caps(max_side * max_side),
            tables.techs.len(),
            outputs,
            &flat,
        )?;
        Ok(GpuBatch {
            catalog: catalog_json.to_owned(),
            tables,
            flat,
            device,
            slots: (0..games).map(|_| Slot::default()).collect(),
            workers: workers.max(1),
            views: None,
            stale: false,
            mixed_boards: false,
        })
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// One past the largest technology id of the rules.
    pub fn tech_ids(&self) -> usize {
        self.tables.techs.len()
    }

    /// Run the slots by `index` on the device and take in the outcome of those run.
    fn run(&mut self, index: &[i32]) -> Result<(), String> {
        self.sync()?;
        self.views = None;
        self.device.run_slots(index, THREADS)?;
        self.take_in(index)
    }

    /// Read every slot's outcome from the device into the slots above after device steps
    /// (`step_device`): the host calls (status, moves, views, reset) see the device's state.
    /// This copies every slot's status and rows to the host; the device loop does not need it.
    pub fn sync(&mut self) -> Result<(), String> {
        if !self.stale {
            return Ok(());
        }
        self.stale = false;
        self.views = None;
        let all: Vec<i32> = self
            .slots
            .iter()
            .map(|s| if s.start.is_some() { 0 } else { SKIP })
            .collect();
        self.take_in(&all)
    }

    /// The device's outcome for the slots whose `index` is not SKIP.
    fn take_in(&mut self, index: &[i32]) -> Result<(), String> {
        let out = self.device.read_slots()?;
        let rows = self.device.read_rows(&out.counts, THREADS)?;
        let mut at = 0;
        for (k, slot) in self.slots.iter_mut().enumerate() {
            let n = out.counts[k] as usize;
            // -3 and -4 stop a slot whose reset failed: its status and reason are the host's
            if index[k] != SKIP && index[k] > -3 {
                let r = out.results[k];
                slot.moves.clear();
                slot.winner = 0;
                slot.reason.clear();
                if r.code != 0 {
                    slot.status = if step::is_failure(r.code) {
                        Status::Failed
                    } else {
                        Status::Refused
                    };
                    slot.reason = self.flat.error_text(r.code, r.arg);
                    slot.start = None;
                } else {
                    slot.status = status_of(out.status[k]);
                    slot.winner = if slot.status == Status::Ended {
                        out.winners[k]
                    } else {
                        0
                    };
                    slot.moves.extend_from_slice(&rows[at..at + n]);
                }
            }
            at += n;
        }
        Ok(())
    }

    /// Start a new game in each named slot (each named once), as `Batch::reset`.
    pub fn reset(&mut self, games: &[(usize, Config)]) -> Result<(), String> {
        self.sync()?;
        let mut starts: Vec<Option<&Config>> = vec![None; self.slots.len()];
        for (slot, config) in games {
            let start = starts.get_mut(*slot).ok_or("slot out of range")?;
            if start.replace(config).is_some() {
                return Err(format!("slot {slot} named twice"));
            }
        }
        let named: Vec<(usize, &Config)> = starts
            .iter()
            .enumerate()
            .filter_map(|(k, c)| c.map(|c| (k, c)))
            .collect();
        let (t, catalog) = (&self.tables, self.catalog.as_str());
        // the games on the CPU, `workers` at a time; an error refuses a slot, a panic fails it
        let chunk = named.len().div_ceil(self.workers).max(1);
        let started: Vec<(usize, Result<Game, (Status, String)>)> = std::thread::scope(|s| {
            let handles: Vec<_> = named
                .chunks(chunk)
                .map(|part| {
                    s.spawn(move || {
                        part.iter()
                            .map(|&(k, config)| {
                                let r = catch_unwind(AssertUnwindSafe(|| {
                                    reset::reset(config, catalog, t)
                                }));
                                let r = match r {
                                    Ok(Ok(g)) => Ok(g),
                                    Ok(Err(reason)) => Err((Status::Refused, reason)),
                                    Err(panic) => {
                                        // the message, as engine_fast_batch's guarded keeps it
                                        let message = panic
                                            .downcast_ref::<String>()
                                            .cloned()
                                            .or_else(|| {
                                                panic
                                                    .downcast_ref::<&str>()
                                                    .map(|s| (*s).to_owned())
                                            })
                                            .unwrap_or_else(|| "panic".to_owned());
                                        Err((Status::Failed, message))
                                    }
                                };
                                (k, r)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("a reset worker"))
                .collect()
        });
        let mut index = vec![SKIP; self.slots.len()];
        let mut blocks = Vec::new();
        for (k, r) in started {
            let slot = &mut self.slots[k];
            slot.moves.clear();
            slot.winner = 0;
            slot.reason.clear();
            match r.and_then(|g| {
                let (tiles, units) = self.device.caps();
                device::to_device_with(&g, tiles, units)
                    .map(|b| (g, b))
                    .map_err(|e| (Status::Refused, e))
            }) {
                Ok((g, block)) => {
                    self.mixed_boards |= block.tile_count != block.tile_cap;
                    blocks.push((k, block));
                    slot.start = Some(g);
                    index[k] = -2;
                }
                Err((status, reason)) => {
                    // the device stops the slot too: it exports nothing of the game it held
                    index[k] = if status == Status::Failed { -4 } else { -3 };
                    slot.status = status;
                    slot.reason = reason;
                    slot.start = None;
                }
            }
        }
        let uploads: Vec<(usize, &device::Block)> = blocks.iter().map(|(k, b)| (*k, b)).collect();
        self.device.upload_many(&uploads)?;
        self.run(&index)
    }

    /// Apply one move per slot, as `Batch::step`.
    pub fn step(&mut self, moves: &[i32], applied: &mut [Applied]) -> Result<(), String> {
        if moves.len() != self.slots.len() || applied.len() != self.slots.len() {
            return Err("one move and one result per slot".into());
        }
        self.sync()?;
        let mut index = vec![SKIP; self.slots.len()];
        for (k, slot) in self.slots.iter().enumerate() {
            applied[k] = if moves[k] == SKIP {
                Applied::Skipped
            } else if slot.status == Status::Playing
                && (0..slot.moves.len() as i32).contains(&moves[k])
            {
                index[k] = moves[k];
                Applied::Stepped
            } else {
                Applied::Rejected
            };
        }
        self.run(&index)
    }

    /// One decision per slot from `actions`, one i32 per slot in GPU 0 memory (a policy's
    /// output; -1 skips), on the device alone: the views, lists and statuses stay there
    /// (`exports`) and only the status word comes back: slots not playing afterwards, slots
    /// that stopped in this call. Per slot: applied 0 skipped, 1 stepped, 2 rejected (not
    /// playing, or no such row), in `exports().applied`. The host calls see the result after
    /// `sync`.
    pub fn step_device(&mut self, actions: u64, bytes: u64) -> Result<[u32; 2], String> {
        self.exports()?;
        self.stale = true;
        self.views = None;
        self.device.step_device(actions, bytes, THREADS)
    }

    /// The device arrays (`DeviceBatch::exports`), whose planes have one layout only while every
    /// game is on the batch's largest board (`cells` tiles; refused otherwise).
    pub fn exports(&mut self) -> Result<crate::device::Exports, String> {
        if self.mixed_boards {
            return Err("the device arrays need every game on the batch's largest board".into());
        }
        Ok(self.device.exports())
    }

    /// The bytes copied to and from the host by this batch's calls so far.
    pub fn traffic(&self) -> (u64, u64) {
        self.device.traffic()
    }

    /// The attack preview of each legal row of `slot` (copied from the device; made with
    /// `Outputs::previews`), as the CPU batch's `previews`.
    pub fn previews(&mut self, slot: usize) -> Result<Vec<[i16; 4]>, String> {
        self.sync()?;
        let x = self.device.exports();
        if x.previews == 0 {
            return Err("the batch was made without previews".into());
        }
        let mut out = vec![[0i16; 4]; self.slots[slot].moves.len()];
        self.device
            .read(x.previews + slot as u64 * x.max_rows as u64 * 8, &mut out)?;
        Ok(out)
    }

    /// The defence plane of `slot` (copied from the device; made with `Outputs::defence`), as
    /// the CPU batch's `defence`.
    pub fn defence(&mut self, slot: usize) -> Result<Option<Vec<i16>>, String> {
        self.sync()?;
        let x = self.device.exports();
        if x.defence == 0 {
            return Err("the batch was made without the defence plane".into());
        }
        let Some(g) = self.slots[slot]
            .start
            .as_ref()
            .filter(|_| self.slots[slot].status == Status::Playing)
        else {
            return Ok(None);
        };
        // the slot's plane starts at its `cells` and has one entry per tile of its own board
        let mut out = vec![0i16; g.tiles.len()];
        self.device
            .read(x.defence + slot as u64 * x.cells as u64 * 2, &mut out)?;
        Ok(Some(out))
    }

    /// The benchmark's on-device random policy into `actions`.
    pub fn random_actions(
        &mut self,
        seed: u64,
        actions: &crate::device::Buffer,
    ) -> Result<(), String> {
        self.device.random_actions(seed, actions, THREADS)
    }

    pub fn status(&self, slot: usize) -> Status {
        assert!(!self.stale, "after step_device, sync before the host calls");
        self.slots[slot].status
    }

    pub fn reason(&self, slot: usize) -> &str {
        assert!(!self.stale, "after step_device, sync before the host calls");
        &self.slots[slot].reason
    }

    pub fn winner(&self, slot: usize) -> i32 {
        assert!(!self.stale, "after step_device, sync before the host calls");
        self.slots[slot].winner
    }

    pub fn moves(&self, slot: usize) -> &[Row] {
        assert!(!self.stale, "after step_device, sync before the host calls");
        &self.slots[slot].moves
    }

    /// The view of the player to move in a playing slot; every slot's view is read from the
    /// device on the first call after a reset or step.
    pub fn view(&mut self, slot: usize) -> Result<Option<&View>, String> {
        self.sync()?;
        if self.views.is_none() {
            let (views, _) = self.device.read_views()?;
            let mut out = Vec::with_capacity(views.len());
            for k in 0..views.len() {
                out.push(match self.slots[k].status {
                    Status::Playing => Some(views.to_view(k)?),
                    _ => None,
                });
            }
            self.views = Some(out);
        }
        Ok(self.views.as_ref().unwrap()[slot].as_ref())
    }

    /// The game in a slot, rebuilt from its block (for tests that compare states).
    pub fn game(&mut self, slot: usize) -> Result<Option<Game>, String> {
        let Some(start) = self.slots[slot].start.as_ref() else {
            return Ok(None);
        };
        let block = self.device.download(slot)?;
        device::from_device(&block, start).map(Some)
    }
}
