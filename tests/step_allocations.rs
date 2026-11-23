//! How many heap allocations the step makes, by the kind of command stepped (applying a
//! move allocates nothing per action in the common cases).
//!
//! One test in its own binary, counting this thread's allocations inside `step::advance`
//! (not the viewer memory or the frame) over whole games of random play: EndTurn one time in
//! ten, never Resign. It runs on a continuing game and again with every step made on a fresh
//! copy of the game.
//!
//! Asserted zero: Attack, Recover, Research, Destroy, Disband, EndTurn, the reply to a peace
//! request and BreakPeace, in steps that leave no choice pending.
//!
//! Asserted rare (at most one allocation in ten steps): Build, Move and PeaceTreaty. A Move
//! that embarks creates a unit; the rest is lists kept per thread growing past what the first
//! game needed.
//!
//! Allowed, with a bound per step:
//! - Train, which creates a unit: the unit list and effect lists grow;
//! - any step that leaves a choice pending (a city levelling up, a peace request): the choice
//!   and the actions waiting behind it are stored in the game. Such a step is counted here
//!   whatever its command, so an allocation elsewhere in it is not told apart.
//!
//! Not asserted: capturing a city (its name and area), a city reward (widening the border
//! allocates), examining ruins, and the commands this play makes too seldom to count
//! (Upgrade, Promote, HealOthers, Disembark, EstablishEmbassy).
//!
//! Not covered: three- and four-seat games, other board sizes and tribes.

use engine_fast::tables::Tables;
use engine_fast::{menu, queries, reset, step};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeMap;

const CATALOG: &str = include_str!("../rules/rules-124.json");

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// The system allocator, counting each thread's allocations.
struct Counting;
// SAFETY: every call is passed to the system allocator unchanged
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        System.dealloc(p, layout)
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        System.realloc(p, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

const END_TURN: i32 = 15;
const RESIGN: i32 = 35;

/// The key for steps that end with a choice pending, whatever the command.
const PENDING_CHOICE: i32 = -1;

/// (steps, allocations) by command kind over `seeds`, 600 decisions each. With `copies`,
/// every step is made on a fresh copy of the game, whose lists have no spare room.
fn play(t: &Tables, seeds: std::ops::Range<i32>, copies: bool) -> BTreeMap<i32, (u64, u64)> {
    let mut by_kind = BTreeMap::new();
    for seed in seeds {
        let config = reset::Config {
            rules: 124,
            seed,
            size: 11,
            map: 2,
            mode: 4,
            tribes: vec![7, 7],
            action_limit: 600,
        };
        let mut g = reset::reset(&config, CATALOG, t).unwrap();
        let mut rng = (seed as u64)
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        loop {
            if queries::winner(&g).unwrap().is_some() || g.decisions == 600 {
                break;
            }
            let candidates = menu::menu(&g, t).unwrap();
            assert!(!candidates.is_empty(), "a playing game with no legal move");
            let kinds: Vec<i32> = candidates.iter().map(|c| c.action.kind).collect();
            let pick: Vec<usize> = (0..kinds.len())
                .filter(|&i| kinds[i] != END_TURN && kinds[i] != RESIGN)
                .collect();
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let slot = match kinds.iter().position(|&k| k == END_TURN) {
                Some(e) if pick.is_empty() || (rng >> 33).is_multiple_of(10) => e,
                _ => pick[((rng >> 33) as usize) % pick.len()],
            };
            let action = &candidates[slot].action;
            if copies {
                g = g.clone();
            }
            let before = ALLOCATIONS.with(Cell::get);
            step::advance(&mut g, t, action).unwrap();
            // a step that leaves a choice pending is counted apart
            let kind = if g.triggers.is_empty() {
                action.kind
            } else {
                PENDING_CHOICE
            };
            let entry: &mut (u64, u64) = by_kind.entry(kind).or_default();
            entry.0 += 1;
            entry.1 += ALLOCATIONS.with(Cell::get) - before;
        }
    }
    by_kind
}

const BUILD: i32 = 1;
const ATTACK: i32 = 2;
const RECOVER: i32 = 3;
const TRAIN: i32 = 5;
const MOVE: i32 = 6;
const RESEARCH: i32 = 8;
const DESTROY: i32 = 9;
const DISBAND: i32 = 10;
const PEACE_TREATY: i32 = 27;
const PEACE_RESPONSE: i32 = 28;
const BREAK_PEACE: i32 = 29;

#[test]
fn step_allocations_by_command_kind() {
    let t = Tables::compile(CATALOG).unwrap();
    // one game first, so the lists kept per thread have grown to the board
    play(&t, 1..2, false);
    for copies in [false, true] {
        let by_kind = play(&t, 2..8, copies);
        let of = |kind: i32| by_kind.get(&kind).copied().unwrap_or_default();
        for kind in [
            ATTACK,
            RECOVER,
            RESEARCH,
            DESTROY,
            DISBAND,
            END_TURN,
            PEACE_RESPONSE,
            BREAK_PEACE,
        ] {
            let (steps, allocations) = of(kind);
            assert!(steps > 0, "no command of kind {kind} was played");
            assert_eq!(
                allocations, 0,
                "kind {kind}, copies {copies}: {steps} steps"
            );
        }
        // see the header: an embarking Move, and per-thread lists still growing
        for kind in [BUILD, MOVE, PEACE_TREATY] {
            let (steps, allocations) = of(kind);
            assert!(
                steps >= 100 && allocations * 10 <= steps,
                "kind {kind}, copies {copies}: {allocations} allocations in {steps} steps"
            );
        }
        // the choice, and the actions waiting behind it, are stored in the game
        let (steps, allocations) = of(PENDING_CHOICE);
        assert!(
            steps >= 100 && allocations <= steps * 3,
            "pending choice, copies {copies}: {allocations} allocations in {steps} steps"
        );
        // a new unit grows the unit list and the effect lists; a copy has no spare room in either
        let (steps, allocations) = of(TRAIN);
        assert!(
            steps >= 100 && allocations <= steps * if copies { 3 } else { 1 },
            "Train, copies {copies}: {allocations} allocations in {steps} steps"
        );
    }
}
