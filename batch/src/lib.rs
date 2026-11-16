//! Many games per call.
//!
//! A `Batch` holds a fixed number of game slots. `reset` starts games, `step` applies one
//! legal move per game, and after either call every playing game has its view (what the
//! player to move may know) and its legal moves ready to read. The engine hands out no model
//! input: scaling, memory of earlier views and the layout a network wants are the caller's.
//!
//! Each game is advanced on its own, so a result never depends on the number of workers or
//! on which other games share the batch, and a game that the engine cannot continue stops
//! alone, with its reason, while the rest play on.

pub mod outputs;

use engine_fast::menu::{self, Cand};
use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::view::{self, View};
use engine_fast::{queries, step};
use rayon::prelude::*;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// The fields of a legal move, in order.
pub const MOVE_FIELDS: [&str; 6] = ["kind", "source", "target", "option", "opponent", "price"];

/// A legal move as integers; `-1` where a field does not apply. `source` and `target` are
/// tile indices (`y * width + x`). `option` is the unit, improvement, technology or reward
/// chosen, or for the reply to a peace request 1 to accept and 0 to decline. `price` is in
/// stars. The first five identify the move; a game's list is sorted by them.
pub type Move = [i32; 6];

/// The command kinds a move's first field can hold, by name.
pub const KINDS: [(&str, i32); 23] = [
    ("Build", 1),
    ("Attack", 2),
    ("Recover", 3),
    ("HealOthers", 4),
    ("Train", 5),
    ("Move", 6),
    ("Capture", 7),
    ("Research", 8),
    ("Destroy", 9),
    ("Disband", 10),
    ("CityReward", 11),
    ("Promote", 13),
    ("ExamineRuins", 14),
    ("EndTurn", 15),
    ("Upgrade", 16),
    ("EndMatch", 22),
    ("Decompose", 26),
    ("PeaceTreaty", 27),
    ("PeaceRequestResponse", 28),
    ("BreakPeace", 29),
    ("EstablishEmbassy", 30),
    ("Resign", 35),
    ("Disembark", 36),
];

/// Where a slot stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    /// no game has been started in the slot
    #[default]
    Empty = 0,
    /// a player is to move: the view and legal moves are current
    Playing = 1,
    /// the game is over; `Batch::winner` names the winner
    Ended = 2,
    /// the game reached its decision limit without ending
    Truncated = 3,
    /// the engine met a rule or a configuration outside its scope; `Batch::reason` says which
    Refused = 4,
    /// the engine failed on this game (a broken invariant); `Batch::reason` has the message
    Failed = 5,
}

/// What `step` did with a slot's move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Applied {
    /// no move was given (`SKIP`)
    Skipped = 0,
    /// the move was taken: the slot's status, view and legal moves are those after it. If
    /// the engine could not carry it out, that status is refused or failed
    Stepped = 1,
    /// the slot is not playing, or the index is not in its legal list; nothing changed
    Rejected = 2,
}

/// The move index that leaves a slot as it is.
pub const SKIP: i32 = -1;

#[derive(Default)]
struct Slot {
    game: Option<Game>,
    status: Status,
    reason: String,
    winner: i32,
    /// the menu's candidates in the order handed out, and the same as integers
    offered: Vec<Cand>,
    moves: Vec<Move>,
    view: View,
}

pub struct Batch {
    catalog: String,
    tables: Tables,
    slots: Vec<Slot>,
    /// None for one worker: the games are then played on the caller's thread
    pool: Option<rayon::ThreadPool>,
}

fn tile(width: i32, at: Option<engine_fast_frame::Xy>) -> i32 {
    at.map_or(-1, |c| c.y * width + c.x)
}

/// The key a game's legal moves are sorted by: the five fields that identify a move.
pub fn sort_key(m: &Move) -> [i32; 5] {
    [m[0], m[1], m[2], m[3], m[4]]
}

/// A menu candidate as the batch hands it out.
pub fn as_move(width: i32, c: &Cand) -> Move {
    let a = &c.action;
    [
        a.kind,
        tile(width, a.source),
        tile(width, a.target),
        a.option.or(a.accept.map(i32::from)).unwrap_or(-1),
        a.opponent.unwrap_or(-1),
        c.details.price.unwrap_or(-1),
    ]
}

impl Slot {
    fn stop(&mut self, status: Status, reason: String) {
        self.game = None;
        self.status = status;
        self.reason = reason;
        self.winner = 0;
        self.offered.clear();
        self.moves.clear();
    }

    /// Work out the status and, for a playing game, the view and legal moves.
    fn refresh(&mut self, t: &Tables) -> Result<(), String> {
        let g = self.game.as_ref().ok_or("no game")?;
        self.moves.clear();
        self.winner = 0;
        if let Some(winner) = queries::winner(g)? {
            self.offered.clear();
            self.winner = winner;
            self.status = Status::Ended;
            return Ok(());
        }
        if g.decisions == g.fixed.action_limit {
            self.offered.clear();
            self.status = Status::Truncated;
            return Ok(());
        }
        let mover = g.players[g.current_player_index as usize].action.id;
        view::write(g, t, mover, &mut self.view)?;
        self.offered = menu::menu(g, t)?;
        // a player to move always has a move; a game without one could never go on
        if self.offered.is_empty() {
            return Err("no legal move".into());
        }
        // (before the sort: the preparation takes the game's order)
        let width = g.width;
        self.offered
            .sort_by_cached_key(|c| sort_key(&as_move(width, c)));
        self.moves
            .extend(self.offered.iter().map(|c| as_move(width, c)));
        self.status = Status::Playing;
        Ok(())
    }

    /// Run `work` on the slot; an error refuses the game and a panic fails it.
    fn guarded(&mut self, work: impl FnOnce(&mut Slot) -> Result<(), String>) {
        match catch_unwind(AssertUnwindSafe(|| work(self))) {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => self.stop(Status::Refused, reason),
            Err(panic) => {
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("panic");
                self.stop(Status::Failed, message.to_owned());
            }
        }
    }
}

impl Batch {
    /// `games` empty slots played on `workers` threads (the caller's own for one or none), for
    /// the rules in `catalog_json`.
    pub fn new(catalog_json: &str, games: usize, workers: usize) -> Result<Batch, String> {
        let pool = if workers > 1 {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(workers);
            Some(pool.build().map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(Batch {
            catalog: catalog_json.to_owned(),
            tables: Tables::compile(catalog_json)?,
            slots: (0..games).map(|_| Slot::default()).collect(),
            pool,
        })
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// One past the largest technology id of the rules, which bounds a view's tech list.
    pub fn tech_ids(&self) -> usize {
        self.tables.techs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Start a new game in each named slot, replacing what was there. A slot may be named
    /// once.
    pub fn reset(&mut self, games: &[(usize, Config)]) -> Result<(), String> {
        let mut starts: Vec<Option<&Config>> = vec![None; self.slots.len()];
        for (slot, config) in games {
            let start = starts.get_mut(*slot).ok_or("slot out of range")?;
            if start.replace(config).is_some() {
                return Err(format!("slot {slot} named twice"));
            }
        }
        let (t, catalog) = (&self.tables, self.catalog.as_str());
        let start = |(slot, config): (&mut Slot, Option<&Config>)| {
            let Some(config) = config else { return };
            slot.guarded(|slot| {
                // (before the reset: a refused reset leaves nothing of the previous game)
                slot.game = Some(reset::reset(config, catalog, t)?);
                slot.reason.clear();
                slot.refresh(t)
            });
        };
        match &self.pool {
            Some(pool) => pool.install(|| self.slots.par_iter_mut().zip(starts).for_each(start)),
            None => self.slots.iter_mut().zip(starts).for_each(start),
        }
        Ok(())
    }

    /// Apply one move per slot: an index into the slot's legal moves, or `SKIP`.
    /// `moves` and `applied` have one entry per slot.
    pub fn step(&mut self, moves: &[i32], applied: &mut [Applied]) -> Result<(), String> {
        if moves.len() != self.slots.len() || applied.len() != self.slots.len() {
            return Err("one move and one result per slot".into());
        }
        let t = &self.tables;
        let play = |((slot, &index), applied): ((&mut Slot, &i32), &mut Applied)| {
            if index == SKIP {
                *applied = Applied::Skipped;
                return;
            }
            let offered = usize::try_from(index)
                .ok()
                .filter(|&i| slot.status == Status::Playing && i < slot.offered.len());
            let Some(i) = offered else {
                *applied = Applied::Rejected;
                return;
            };
            *applied = Applied::Stepped;
            slot.guarded(|slot| {
                let action = slot.offered[i].action.clone();
                let g = slot.game.as_mut().ok_or("no game")?;
                step::advance(g, t, &action)?;
                slot.refresh(t)
            });
        };
        match &self.pool {
            Some(pool) => {
                pool.install(|| (self.slots.par_iter_mut().zip(moves).zip(applied)).for_each(play))
            }
            None => (self.slots.iter_mut().zip(moves).zip(applied)).for_each(play),
        }
        Ok(())
    }

    pub fn status(&self, slot: usize) -> Status {
        self.slots[slot].status
    }

    /// Why a slot was refused or failed (empty otherwise).
    pub fn reason(&self, slot: usize) -> &str {
        &self.slots[slot].reason
    }

    /// The winner's player id once the slot has ended, else 0.
    pub fn winner(&self, slot: usize) -> i32 {
        self.slots[slot].winner
    }

    /// The legal moves of a playing slot, sorted (empty otherwise).
    pub fn moves(&self, slot: usize) -> &[Move] {
        &self.slots[slot].moves
    }

    /// The view of the player to move in a playing slot.
    pub fn view(&self, slot: usize) -> Option<&View> {
        let s = &self.slots[slot];
        (s.status == Status::Playing).then_some(&s.view)
    }

    /// The game in a slot, for tests that compare states; not part of the interface (None
    /// once it has been refused or has failed).
    pub fn game(&self, slot: usize) -> Option<&Game> {
        self.slots[slot].game.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_refuses_the_slot_and_a_panic_fails_it() {
        let mut slot = Slot {
            winner: 2,
            moves: vec![[0; 6]],
            ..Slot::default()
        };
        slot.guarded(|_| Err("outside scope".into()));
        assert_eq!(
            (slot.status, slot.reason.as_str()),
            (Status::Refused, "outside scope")
        );
        assert!(slot.moves.is_empty() && slot.winner == 0);
        slot.guarded(|_| panic!("broken {}", 1));
        assert_eq!(
            (slot.status, slot.reason.as_str()),
            (Status::Failed, "broken 1")
        );
        slot.guarded(|slot| {
            slot.status = Status::Playing;
            Ok(())
        });
        assert_eq!(slot.status, Status::Playing);
    }

    #[test]
    fn kinds_are_listed_once_each() {
        for (i, (name, id)) in KINDS.iter().enumerate() {
            assert!(KINDS[..i].iter().all(|(n, k)| n != name && k != id));
        }
    }
}
