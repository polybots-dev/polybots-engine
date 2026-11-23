//! The batch interface on the GPU: the same
//! slots, statuses, legal rows and views as `engine_fast_batch::Batch`, with the games kept
//! in GPU memory and every step, status, view and legal list run there (`kernel/status.h`).
//! Games start on the CPU (`reset` on `workers` threads, or ahead of time on the background
//! bank: `queue` and `restart_finished`) and are uploaded.
//!
//! Where it cannot match the CPU batch: a slot the engine fails on during a step (a broken
//! invariant) is failed here too, but its reason is the kernel's text, not the Rust panic
//! message (a reset runs the engine itself and keeps it); a game
//! larger than the device block (units, actions, legal rows) stops with a device reason where
//! the CPU batch plays on. `game` rebuilds the state from the block with the city names and
//! viewer memory of the game's start (the block holds neither), which it starts again from
//! the slot's configuration: a slot keeps its configuration, not its starting game.

use crate::device::{DeviceBatch, Outputs};
use crate::menu::Row;
use crate::restarts::{Bank, Page, Rules};
use crate::state::{self as device};
use crate::step;
use crate::tables::DeviceTables;
use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::view::View;
use engine_fast_batch::outputs::{Plane, PROGRESSION_LEN};
use engine_fast_batch::{Applied, Status, SKIP};
use std::sync::Arc;

const THREADS: usize = 32;
/// Games a `reset` starts before it uploads them, so its host memory does not grow with the
/// batch.
const RESET_ROUND: usize = 4096;

#[derive(Default)]
struct Slot {
    status: Status,
    reason: String,
    winner: i32,
    moves: Vec<Row>,
    /// the configuration the slot's game started from, while the device holds the game
    config: Option<Config>,
    /// the board's tile count
    tiles: usize,
    /// the game as it started, for `game`'s parts the block does not hold; started again from
    /// `config` on the first `game` call
    start: Option<Game>,
}

pub struct GpuBatch {
    rules: Arc<Rules>,
    /// games queued ahead (`queue`), made on first use
    bank: Option<Bank>,
    flat: DeviceTables,
    device: DeviceBatch,
    slots: Vec<Slot>,
    workers: usize,
    /// the views of the last run, read from the device when first asked for
    views: HostViews,
    /// device steps ran since the slots above were last read from the device (`sync`)
    stale: bool,
    /// a `step_launch` has not been waited for (`step_wait`): every other device call waits
    in_flight: bool,
    /// a game on a board smaller than the batch's largest was started: the exported planes
    /// (laid out per board) no longer have one layout, so the device arrays are refused
    mixed_boards: bool,
    /// a restart or reset failed part way (a device error, or a bank worker's panic): the
    /// slots may be part placed, so every later call that changes them fails with this
    broken: Option<String>,
}

/// Per slot, the views of the last run copied to the host: the mover's, and the other seat's
/// (`Outputs::other_view`); each read when first asked for.
#[derive(Default)]
struct HostViews {
    mover: Option<Vec<Option<View>>>,
    other: Option<Vec<Option<View>>>,
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

    /// As `new`, computing the optional `outputs` too (`exports().previews`, `.plane_outputs`,
    /// `.income`, `.city_score`, `.progression`, `.other_tiles` and the rest of the other
    /// seat's view).
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
        let caps = device.caps();
        Ok(GpuBatch {
            rules: Arc::new(Rules {
                catalog: catalog_json.to_owned(),
                tables,
                caps,
            }),
            bank: None,
            flat,
            device,
            slots: (0..games).map(|_| Slot::default()).collect(),
            workers: workers.max(1),
            views: HostViews::default(),
            stale: false,
            in_flight: false,
            mixed_boards: false,
            broken: None,
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
        self.rules.tables.techs.len()
    }

    /// Run the slots by `index` on the device and take in the outcome of those run.
    fn run(&mut self, index: &[i32]) -> Result<(), String> {
        self.sync()?;
        self.views = HostViews::default();
        self.device.run_slots(index, THREADS)?;
        self.take_in(index)
    }

    /// Read every slot's outcome from the device into the slots above after device steps
    /// (`step_device`): the host calls (status, moves, views, reset) see the device's state.
    /// This copies every slot's status and rows to the host; the device loop does not need it.
    pub fn sync(&mut self) -> Result<(), String> {
        self.usable()?;
        if !self.stale {
            return Ok(());
        }
        self.stale = false;
        self.views = HostViews::default();
        let all: Vec<i32> = self
            .slots
            .iter()
            .map(|s| if s.config.is_some() { 0 } else { SKIP })
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
                    slot.config = None;
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
        let mut named = vec![false; self.slots.len()];
        for (slot, _) in games {
            let seen = named.get_mut(*slot).ok_or("slot out of range")?;
            if std::mem::replace(seen, true) {
                return Err(format!("slot {slot} named twice"));
            }
        }
        let placed = self.reset_checked(games);
        self.broke_on(placed)
    }

    /// A failed call's error, kept so that every later call fails too.
    fn broke_on<T>(&mut self, r: Result<T, String>) -> Result<T, String> {
        if let Err(e) = &r {
            self.broken.get_or_insert_with(|| e.clone());
        }
        r
    }

    fn usable(&self) -> Result<(), String> {
        if self.in_flight {
            return Err("a device step is in flight: step_wait first".into());
        }
        match &self.broken {
            Some(e) => Err(format!("the batch stopped on an earlier error: {e}")),
            None => Ok(()),
        }
    }

    /// `reset` of distinct slots in range.
    fn reset_checked(&mut self, games: &[(usize, Config)]) -> Result<(), String> {
        let mut games: Vec<&(usize, Config)> = games.iter().collect();
        games.sort_by_key(|(slot, _)| *slot);
        let mut index = vec![SKIP; self.slots.len()];
        for round in games.chunks(RESET_ROUND) {
            let configs: Vec<Config> = round.iter().map(|(_, c)| c.clone()).collect();
            let per_page = round.len().div_ceil(self.workers);
            let slots: Vec<usize> = round.iter().map(|(k, _)| *k).collect();
            let mut at = 0;
            for page in self.rules.pages(&configs, self.workers, per_page) {
                let n = page.left();
                self.place(&page, &slots[at..at + n], &mut index)?;
                at += n;
            }
        }
        self.run(&index)
    }

    /// Start games in the background, after any queued before, for `restart_finished`. Every
    /// queued game's block is held on the host until it is used (68 KB at 11 x 11), so the
    /// caller bounds the queue. Games must be on the batch's largest board, which the device
    /// arrays need.
    pub fn queue(&mut self, games: Vec<Config>) -> Result<(), String> {
        let tiles = self.rules.caps.0;
        if let Some(c) = games.iter().find(|c| c.size * c.size != tiles) {
            return Err(format!(
                "a queued game of side {} on a batch of {tiles} tiles",
                c.size
            ));
        }
        let (rules, workers) = (&self.rules, self.workers);
        self.bank
            .get_or_insert_with(|| Bank::new(rules.clone(), workers))
            .queue(games);
        Ok(())
    }

    /// Games queued and not yet started in a slot.
    pub fn queued(&self) -> usize {
        self.bank.as_ref().map_or(0, Bank::len)
    }

    /// Of the games `queued`, those already built (the next in queue order, none missing).
    pub fn ready(&self) -> Result<usize, String> {
        self.bank.as_ref().map_or(Ok(0), Bank::ready)
    }

    /// Give every slot not playing on the device (in slot order) the next queued game (in
    /// queue order), as `reset` with those slots and configurations would; slots past the
    /// queue's end are left as they are. With `wait`, a game still being started is waited
    /// for; without, only the `ready` games are used (which then depends on the threads'
    /// timing; each slot's game does not). A slot whose game was refused or failed is restarted
    /// too, and stopped. The device then holds the new games' statuses, lists and views; the
    /// other slots are untouched; the host calls see them after `sync`. Returns the slots
    /// restarted, in the order their games were queued.
    pub fn restart_finished(&mut self, wait: bool) -> Result<Vec<usize>, String> {
        self.usable()?;
        let r = self.restart_unchecked(wait);
        self.broke_on(r)
    }

    fn restart_unchecked(&mut self, wait: bool) -> Result<Vec<usize>, String> {
        let mut status = vec![0i32; self.slots.len()];
        let x = self.device.exports();
        self.device.read(x.status, &mut status)?;
        let finished: Vec<usize> = (0..self.slots.len())
            .filter(|&k| status_of(status[k]) != Status::Playing || self.slots[k].config.is_none())
            .collect();
        let Some(mut bank) = self.bank.take() else {
            return Ok(Vec::new());
        };
        let n = match if wait { Ok(bank.len()) } else { bank.ready() } {
            Ok(queued) => finished.len().min(queued),
            Err(e) => {
                self.bank = Some(bank);
                return Err(e);
            }
        };
        if n == 0 {
            self.bank = Some(bank);
            return Ok(Vec::new());
        }
        // from here the host's copy of the slots is not the device's
        self.views = HostViews::default();
        self.stale = true;
        let mut index = vec![SKIP; self.slots.len()];
        let mut at = 0;
        let mut placed = Ok(());
        while at < n && placed.is_ok() {
            placed = match bank.front() {
                Ok(Some(page)) => {
                    let take = page.left().min(n - at);
                    let r = self.place(page, &finished[at..at + take], &mut index);
                    bank.took(take);
                    at += take;
                    r
                }
                Ok(None) => unreachable!("n games are queued"),
                Err(e) => Err(e),
            };
        }
        self.bank = Some(bank);
        // an error here is the device's (a failed copy) or a bank worker's panic: the slots
        // may be part placed, so the batch stops (`broken`)
        placed?;
        self.device.run_slots(&index, THREADS)?;
        Ok(finished[..n].to_vec())
    }

    /// Put the next `slots.len()` games of `page` (from its `taken`) into `slots`: upload the
    /// blocks of those that started, run by run of consecutive games, and mark them in `index`
    /// for the device to take in (-2), or stop the slot with the host's status (-3 refused, -4
    /// failed). The caller marks them taken.
    fn place(&mut self, page: &Page, slots: &[usize], index: &mut [i32]) -> Result<(), String> {
        let first = page.taken;
        let mut run = first;
        for (i, &k) in slots.iter().enumerate() {
            let game = first + i;
            let s = &page.started[game];
            let slot = &mut self.slots[k];
            slot.moves.clear();
            slot.winner = 0;
            slot.reason.clear();
            slot.start = None;
            match &s.outcome {
                Ok(tiles) => {
                    self.mixed_boards |= *tiles != self.rules.caps.0 as usize;
                    slot.config = Some(s.config.clone());
                    slot.tiles = *tiles;
                    index[k] = -2;
                }
                Err((status, reason)) => {
                    // the device stops the slot too: it exports nothing of the game it held
                    index[k] = if *status == Status::Failed { -4 } else { -3 };
                    slot.status = *status;
                    slot.reason = reason.clone();
                    slot.config = None;
                    self.upload_run(page, slots, first, run, game)?;
                    run = game + 1;
                }
            }
        }
        self.upload_run(page, slots, first, run, first + slots.len())
    }

    /// Upload games `from..to` of `page`, which go to `slots[from - first..to - first]`.
    fn upload_run(
        &mut self,
        page: &Page,
        slots: &[usize],
        first: usize,
        from: usize,
        to: usize,
    ) -> Result<(), String> {
        if from == to {
            return Ok(());
        }
        self.device
            .upload_words(&slots[from - first..to - first], page.blocks(from, to))
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
        self.begin_step()?;
        self.device.step_device(actions, bytes, THREADS)
    }

    /// `step_device` without waiting, so the host and other streams (a policy acting on
    /// another batch) run while it does: the step and its status word's copy are queued on the
    /// batch's stream, and `step_wait` returns the word. The results are the same. `actions`
    /// must be complete when the step runs (written by `random_actions`, on the default stream,
    /// or waited for). Until `step_wait`, the batch's other device calls are refused. The
    /// step overlaps only work on non-blocking streams: anything on the default stream (a
    /// policy left on PyTorch's default stream, a synchronous copy) waits for it, and it for them.
    pub fn step_launch(&mut self, actions: u64, bytes: u64) -> Result<(), String> {
        self.begin_step()?;
        self.device.step_launch(actions, bytes, THREADS)?;
        self.in_flight = true;
        Ok(())
    }

    /// Wait for the last `step_launch` and return its status word (as `step_device`'s).
    pub fn step_wait(&mut self) -> Result<[u32; 2], String> {
        if !self.in_flight {
            return Err("step_wait: no step launched".into());
        }
        self.in_flight = false;
        self.device.step_wait()
    }

    fn begin_step(&mut self) -> Result<(), String> {
        self.usable()?;
        self.exports()?;
        self.stale = true;
        self.views = HostViews::default();
        Ok(())
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

    /// `len` entries of `slot` from the exported array at `base` (`per_slot` entries a slot),
    /// copied from the device; None when the slot is not playing. Refused when the batch was
    /// made without `what`.
    fn read_slot<T: Copy + Default>(
        &mut self,
        base: u64,
        per_slot: usize,
        len: usize,
        slot: usize,
        what: &str,
    ) -> Result<Option<Vec<T>>, String> {
        self.sync()?;
        if base == 0 {
            return Err(format!("the batch was made without {what}"));
        }
        let s = &self.slots[slot];
        if s.config.is_none() || s.status != Status::Playing {
            return Ok(None);
        }
        let mut out = vec![T::default(); len];
        let at = (slot * per_slot * std::mem::size_of::<T>()) as u64;
        self.device.read(base + at, &mut out)?;
        Ok(Some(out))
    }

    /// Plane `p` of `slot` (made with it in `Outputs::planes`), as the CPU batch's `plane`.
    pub fn plane(&mut self, slot: usize, p: Plane) -> Result<Option<Vec<i16>>, String> {
        let x = self.device.exports();
        // a slot's plane starts at its `cells` and has one entry per tile of its own board
        let tiles = self.slots[slot].tiles;
        self.read_slot(
            x.plane_outputs[p as usize],
            x.cells as usize,
            tiles,
            slot,
            p.name(),
        )
    }

    /// The income of `slot` (made with `Outputs::income`), as the CPU batch's `income`.
    pub fn income(&mut self, slot: usize) -> Result<Option<i32>, String> {
        let x = self.device.exports();
        Ok(self
            .read_slot::<i32>(x.income, 1, 1, slot, "income")?
            .map(|v| v[0]))
    }

    /// The city scores of `slot` (made with `Outputs::city_score`), as the CPU batch's.
    pub fn city_score(&mut self, slot: usize) -> Result<Option<Vec<i32>>, String> {
        let x = self.device.exports();
        let tiles = self.slots[slot].tiles;
        self.read_slot(x.city_score, x.cells as usize, tiles, slot, "city_score")
    }

    /// The progression row of `slot` (made with `Outputs::progression`), as the CPU batch's.
    pub fn progression(&mut self, slot: usize) -> Result<Option<Vec<i32>>, String> {
        let x = self.device.exports();
        self.read_slot(
            x.progression,
            PROGRESSION_LEN,
            PROGRESSION_LEN,
            slot,
            "progression",
        )
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
        if self.views.mover.is_none() {
            let (views, _) = self.device.read_views()?;
            let mut out = Vec::with_capacity(views.len());
            for k in 0..views.len() {
                out.push(match self.slots[k].status {
                    Status::Playing => Some(views.to_view(k)?),
                    _ => None,
                });
            }
            self.views.mover = Some(out);
        }
        Ok(self.views.mover.as_ref().unwrap()[slot].as_ref())
    }

    /// The view of the next seat after the player to move in a playing slot, as the CPU
    /// batch's `other_view` (the privileged critic's input). Needs `Outputs::other_view`.
    pub fn other_view(&mut self, slot: usize) -> Result<Option<&View>, String> {
        self.sync()?;
        if self.views.other.is_none() {
            let views = self.device.read_other_views()?;
            let mut out = Vec::with_capacity(views.len());
            for k in 0..views.len() {
                let shown = self.slots[k].status == Status::Playing && views.viewer(k) >= 0;
                out.push(shown.then(|| views.to_view(k)).transpose()?);
            }
            self.views.other = Some(out);
        }
        Ok(self.views.other.as_ref().unwrap()[slot].as_ref())
    }

    /// The game in a slot, rebuilt from its block (for tests that compare states).
    pub fn game(&mut self, slot: usize) -> Result<Option<Game>, String> {
        self.usable()?;
        let s = &mut self.slots[slot];
        let Some(config) = s.config.as_ref() else {
            return Ok(None);
        };
        if s.start.is_none() {
            s.start = Some(reset::reset(
                config,
                &self.rules.catalog,
                &self.rules.tables,
            )?);
        }
        let block = self.device.download(slot)?;
        device::from_device(&block, s.start.as_ref().unwrap()).map(Some)
    }
}
