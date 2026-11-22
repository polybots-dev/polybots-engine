//! The fixed action table: one slot per possible move of a
//! game, a function of the five fields that identify a move (kind, source, target, option,
//! opponent). `encode` maps a legal row to its slot and refuses a row whose fields do not
//! match its kind's pattern, so two rows can never share a slot through an ignored field;
//! `decode` maps a slot back to the five fields, or to nothing for a slot no move uses.
//!
//! Layout, `T` tiles, `t` a tile index (`y * side + x`):
//! - global, 96 slots from 0: EndTurn, EndMatch, Resign; diplomacy by the opponent's seat
//!   relative to the mover; Research by technology id;
//! - unit, 20 per tile: kinds with a source and no target;
//! - attack, 49 per tile: the target in the 7 x 7 window around the source;
//! - tile, 138 per tile: kinds with a target and no source (Build and Train by id);
//! - move, `T` per tile: the absolute destination.

use crate::Move;

/// The layout's name and widths, for a run's configuration to record (and hash): a model
/// trained on one layout reads only that layout.
pub const SCHEMA: &str = "action-table v1: global 96 (end turn 0, end match 1, resign 2, \
diplomacy 14 + 5 (seat - mover - 1 mod seats) + j with j treaty, decline, accept, break, embassy, \
research 32 + id), unit 20 per tile (recover, heal others, capture, disband, promote, examine \
ruins, disembark, freeze area, break ice, explode, boost, swarm, clear flood, clear algae, \
upgrade to 41, 42, 44), attack 49 per tile (7 x 7 window, row by dy), tile 138 per tile (destroy \
0, decompose 1, reward 1 + r, build 10 + id, train 74 + id), move T per tile (destination); \
unit, improvement and technology ids below 64; slot i is bit i % 32 of word i / 32";

pub const GLOBAL: usize = 96;
pub const UNIT_CHANNELS: usize = 20;
pub const ATTACK_WINDOW: usize = 49;
pub const TILE_CHANNELS: usize = 138;
/// The widest id block (units, improvements, technologies): ids 0 to 63.
pub const ID_BLOCK: i32 = 64;
/// The farthest an attack reaches each way (`combat::ATTACK_RANGE`).
pub const ATTACK_REACH: i32 = 3;

const END_TURN: i32 = 15;
const END_MATCH: i32 = 22;
const RESIGN: i32 = 35;
const PEACE_TREATY: i32 = 27;
const PEACE_RESPONSE: i32 = 28;
const BREAK_PEACE: i32 = 29;
const EMBASSY: i32 = 30;
const RESEARCH: i32 = 8;
const ATTACK: i32 = 2;
const MOVE: i32 = 6;
const BUILD: i32 = 1;
const TRAIN: i32 = 5;
const CITY_REWARD: i32 = 11;
const DIPLOMACY: usize = 14;
const RESEARCH_AT: usize = 32;

/// The unit block's channels: (kind, option, name), -1 for no option. Channels past the end
/// and 10 (Boost: no unit in the rule data has it) are kept for later.
const UNIT: [(i32, i32, &str); 17] = [
    (3, -1, "Recover"),
    (4, -1, "HealOthers"),
    (7, -1, "Capture"),
    (10, -1, "Disband"),
    (13, -1, "Promote"),
    (14, -1, "ExamineRuins"),
    (36, -1, "Disembark"),
    (17, -1, "FreezeArea"),
    (18, 0, "BreakIce"),
    (24, -1, "Explode"),
    (25, -1, "Boost"),
    (38, -1, "Swarm"),
    (39, 1, "ClearTileEffect flood"),
    (39, 4, "ClearTileEffect algae"),
    // the three ships a transport ship becomes
    (16, 41, "Upgrade 41"),
    (16, 42, "Upgrade 42"),
    (16, 44, "Upgrade 44"),
];
/// The unit block's channel kept for Boost, which no rule data in use offers.
pub const BOOST_CHANNEL: usize = 10;

/// The tile block's channels before the reward, build and train ranges.
const DESTROY: i32 = 9;
const DECOMPOSE: i32 = 26;
const REWARDS_AT: usize = 2;
const BUILD_AT: usize = 10;
const TRAIN_AT: usize = 74;

/// The table's size for a board of `tiles` tiles.
pub fn slots(tiles: usize) -> usize {
    GLOBAL + (UNIT_CHANNELS + ATTACK_WINDOW + TILE_CHANNELS) * tiles + tiles * tiles
}

/// Where each block starts, and the end: global, unit, attack, tile, move, end.
pub fn blocks(tiles: usize) -> [usize; 6] {
    let unit = GLOBAL;
    let attack = unit + UNIT_CHANNELS * tiles;
    let tile = attack + ATTACK_WINDOW * tiles;
    let moves = tile + TILE_CHANNELS * tiles;
    [0, unit, attack, tile, moves, moves + tiles * tiles]
}

/// A block of the table and the channel within it, for counts: the global block's slot, a
/// unit or tile channel, the attack window's offset, or the move block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    Global(usize),
    Unit(usize),
    Attack(usize),
    Tile(usize),
    Move,
}

/// The block and channel of `slot` on a board of `tiles` tiles.
pub fn channel(slot: usize, tiles: usize) -> Channel {
    let b = blocks(tiles);
    if slot < b[1] {
        Channel::Global(slot)
    } else if slot < b[2] {
        Channel::Unit((slot - b[1]) % UNIT_CHANNELS)
    } else if slot < b[3] {
        Channel::Attack((slot - b[2]) % ATTACK_WINDOW)
    } else if slot < b[4] {
        Channel::Tile((slot - b[3]) % TILE_CHANNELS)
    } else {
        Channel::Move
    }
}

/// A readable name for a channel, for reports: the kind and option, the attack offset.
pub fn describe(c: Channel) -> String {
    let diplomacy = [
        "PeaceTreaty",
        "PeaceReply decline",
        "PeaceReply accept",
        "BreakPeace",
        "Embassy",
    ];
    match c {
        Channel::Global(0) => "EndTurn".into(),
        Channel::Global(1) => "EndMatch".into(),
        Channel::Global(2) => "Resign".into(),
        Channel::Global(s) if (DIPLOMACY..DIPLOMACY + 15).contains(&s) => format!(
            "{} seat +{}",
            diplomacy[(s - DIPLOMACY) % 5],
            (s - DIPLOMACY) / 5 + 1
        ),
        Channel::Global(s) if s >= RESEARCH_AT => format!("Research {}", s - RESEARCH_AT),
        Channel::Global(s) => format!("global reserved {s}"),
        Channel::Unit(k) => UNIT
            .get(k)
            .map_or(format!("unit reserved {k}"), |u| u.2.into()),
        Channel::Attack(k) => format!("Attack {:+},{:+}", k as i32 % 7 - 3, k as i32 / 7 - 3),
        Channel::Tile(0) => "Destroy".into(),
        Channel::Tile(1) => "Decompose".into(),
        Channel::Tile(c) if c < BUILD_AT => format!("CityReward {}", c + 1 - REWARDS_AT),
        Channel::Tile(c) if c < TRAIN_AT => format!("Build {}", c - BUILD_AT),
        Channel::Tile(c) => format!("Train {}", c - TRAIN_AT),
        Channel::Move => "Move".into(),
    }
}

/// The board a table is laid out for: its side, the player to move and the number of seats
/// (player ids 1 to `seats`).
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub side: i32,
    pub mover: i32,
    pub seats: i32,
}

impl Frame {
    /// Whether the table is laid out for this frame: a side the engine plays (8, 11 to 30),
    /// two to four seats, and a mover among them.
    pub fn valid(&self) -> Result<(), String> {
        let side = self.side == 8 || (11..=30).contains(&self.side);
        if side && (2..=4).contains(&self.seats) && (1..=self.seats).contains(&self.mover) {
            Ok(())
        } else {
            Err(format!("no table for {self:?}"))
        }
    }
    fn tiles(&self) -> usize {
        (self.side * self.side) as usize
    }
    fn tile(&self, t: i32, what: &str) -> Result<usize, String> {
        if (0..self.side * self.side).contains(&t) {
            Ok(t as usize)
        } else {
            Err(format!("{what} {t} is not a tile"))
        }
    }
}

/// The slot of the move whose identifying fields are `m[..5]`. Refuses a row that does not
/// match its kind's pattern: a field set that the kind does not use, an option outside its
/// block, an attack outside the window, an opponent that is the mover or not a seat.
pub fn encode(m: &Move, f: &Frame) -> Result<usize, String> {
    f.valid()?;
    let [kind, source, target, option, opponent, _] = *m;
    let fields = (source != -1, target != -1, option != -1, opponent != -1);
    let refuse = |why: &str| Err(format!("kind {kind} {m:?}: {why}"));
    let want = |s, t, o, p| {
        if fields == (s, t, o, p) {
            Ok(())
        } else {
            Err(format!("kind {kind} {m:?}: fields not the kind's pattern"))
        }
    };
    let b = blocks(f.tiles());
    match kind {
        END_TURN | END_MATCH | RESIGN => {
            want(false, false, false, false)?;
            Ok(match kind {
                END_TURN => 0,
                END_MATCH => 1,
                _ => 2,
            })
        }
        PEACE_TREATY | PEACE_RESPONSE | BREAK_PEACE | EMBASSY => {
            want(false, false, kind == PEACE_RESPONSE, true)?;
            if opponent == f.mover || !(1..=f.seats).contains(&opponent) {
                return refuse("the opponent is the mover or not a seat");
            }
            let rel = (opponent - f.mover).rem_euclid(f.seats) as usize;
            let j = match kind {
                PEACE_TREATY => 0,
                PEACE_RESPONSE if option == 0 => 1,
                PEACE_RESPONSE if option == 1 => 2,
                PEACE_RESPONSE => return refuse("a reply is 0 or 1"),
                BREAK_PEACE => 3,
                _ => 4,
            };
            Ok(DIPLOMACY + 5 * (rel - 1) + j)
        }
        RESEARCH => {
            want(false, false, true, false)?;
            if !(0..ID_BLOCK).contains(&option) {
                return refuse("a technology id outside 0 to 63");
            }
            Ok(RESEARCH_AT + option as usize)
        }
        ATTACK => {
            want(true, true, false, false)?;
            let (s, t) = (f.tile(source, "source")?, f.tile(target, "target")?);
            let side = f.side;
            let (dx, dy) = (target % side - source % side, target / side - source / side);
            if dx.abs() > ATTACK_REACH || dy.abs() > ATTACK_REACH || s == t {
                return refuse("the target is outside the attack window");
            }
            Ok(b[2] + ATTACK_WINDOW * s + (7 * (dy + 3) + dx + 3) as usize)
        }
        MOVE => {
            want(true, true, false, false)?;
            let (s, t) = (f.tile(source, "source")?, f.tile(target, "target")?);
            if s == t {
                return refuse("a move onto its own tile");
            }
            Ok(b[4] + f.tiles() * s + t)
        }
        DESTROY | DECOMPOSE | CITY_REWARD | BUILD | TRAIN => {
            want(false, true, !matches!(kind, DESTROY | DECOMPOSE), false)?;
            let t = f.tile(target, "target")?;
            let c = match kind {
                DESTROY => 0,
                DECOMPOSE => 1,
                CITY_REWARD if (1..=8).contains(&option) => REWARDS_AT - 1 + option as usize,
                BUILD | TRAIN if (0..ID_BLOCK).contains(&option) => {
                    (if kind == BUILD { BUILD_AT } else { TRAIN_AT }) + option as usize
                }
                _ => return refuse("an option outside its block"),
            };
            Ok(b[3] + TILE_CHANNELS * t + c)
        }
        _ => {
            let Some(c) = UNIT.iter().position(|&(k, o, _)| k == kind && o == option) else {
                return refuse("not a kind or option the table holds");
            };
            want(true, false, option != -1, false)?;
            Ok(b[1] + UNIT_CHANNELS * f.tile(source, "source")? + c)
        }
    }
}

/// The identifying fields (kind, source, target, option, opponent; -1 where unused) of the
/// move at `slot`, or None for a slot no move uses (reserved, past the seats, off the board).
pub fn decode(slot: usize, f: &Frame) -> Option<[i32; 5]> {
    f.valid().ok()?;
    let n = f.tiles();
    let b = blocks(n);
    if slot >= b[5] {
        return None;
    }
    let tile = |k: usize| k as i32;
    if slot < b[1] {
        return match slot {
            0 => Some([END_TURN, -1, -1, -1, -1]),
            1 => Some([END_MATCH, -1, -1, -1, -1]),
            2 => Some([RESIGN, -1, -1, -1, -1]),
            s if (DIPLOMACY..DIPLOMACY + 15).contains(&s) => {
                let (rel, j) = ((s - DIPLOMACY) / 5 + 1, (s - DIPLOMACY) % 5);
                if rel as i32 >= f.seats {
                    return None;
                }
                let opponent = (f.mover - 1 + rel as i32).rem_euclid(f.seats) + 1;
                let (kind, option) = match j {
                    0 => (PEACE_TREATY, -1),
                    1 => (PEACE_RESPONSE, 0),
                    2 => (PEACE_RESPONSE, 1),
                    3 => (BREAK_PEACE, -1),
                    _ => (EMBASSY, -1),
                };
                Some([kind, -1, -1, option, opponent])
            }
            s if s >= RESEARCH_AT => Some([RESEARCH, -1, -1, (s - RESEARCH_AT) as i32, -1]),
            _ => None,
        };
    }
    if slot < b[2] {
        let (t, c) = ((slot - b[1]) / UNIT_CHANNELS, (slot - b[1]) % UNIT_CHANNELS);
        let &(kind, option, _) = UNIT.get(c)?;
        return Some([kind, tile(t), -1, option, -1]);
    }
    if slot < b[3] {
        let (s, k) = ((slot - b[2]) / ATTACK_WINDOW, (slot - b[2]) % ATTACK_WINDOW);
        let (dx, dy) = (k as i32 % 7 - 3, k as i32 / 7 - 3);
        let (x, y) = (s as i32 % f.side + dx, s as i32 / f.side + dy);
        if (dx, dy) == (0, 0) || !(0..f.side).contains(&x) || !(0..f.side).contains(&y) {
            return None;
        }
        return Some([ATTACK, tile(s), y * f.side + x, -1, -1]);
    }
    if slot < b[4] {
        let (t, c) = ((slot - b[3]) / TILE_CHANNELS, (slot - b[3]) % TILE_CHANNELS);
        let (kind, option) = match c {
            0 => (DESTROY, -1),
            1 => (DECOMPOSE, -1),
            c if c < BUILD_AT => (CITY_REWARD, (c + 1 - REWARDS_AT) as i32),
            c if c < TRAIN_AT => (BUILD, (c - BUILD_AT) as i32),
            c => (TRAIN, (c - TRAIN_AT) as i32),
        };
        return Some([kind, -1, tile(t), option, -1]);
    }
    let (s, t) = ((slot - b[4]) / n, (slot - b[4]) % n);
    (s != t).then_some([MOVE, tile(s), tile(t), -1, -1])
}

/// The legality bitmask of a list of rows: slot `i` is bit `i % 32` of word `i / 32`. Refuses
/// a row that does not encode, or two rows on one slot.
pub fn mask(rows: &[Move], f: &Frame, out: &mut Vec<u32>) -> Result<(), String> {
    let slots_of = rows
        .iter()
        .map(|m| encode(m, f))
        .collect::<Result<Vec<_>, _>>()?;
    fill_mask(&slots_of, f.tiles(), out)
}

/// The bitmask of slots already encoded, on a board of `tiles` tiles. Refuses a slot set twice.
pub fn fill_mask(slots_of: &[usize], tiles: usize, out: &mut Vec<u32>) -> Result<(), String> {
    out.clear();
    out.resize(slots(tiles).div_ceil(32), 0);
    for &s in slots_of {
        let (w, bit) = (s / 32, 1u32 << (s % 32));
        if out[w] & bit != 0 {
            return Err(format!("two rows on slot {s}"));
        }
        out[w] |= bit;
    }
    Ok(())
}

/// The coverage check's properties at one decision:
/// every row encodes (D1), decodes to its own fields (D2), no two rows share a slot (D3), and
/// `mask` holds exactly the rows' slots (D4). Returns each row's slot; an error starts with the
/// property's name ("D1 " to "D4 ").
pub fn check(rows: &[Move], f: &Frame, mask: &[u32]) -> Result<Vec<usize>, String> {
    let mut slots_of = Vec::with_capacity(rows.len());
    for m in rows {
        let s = encode(m, f).map_err(|e| format!("D1 {e}"))?;
        let key = [m[0], m[1], m[2], m[3], m[4]];
        if decode(s, f) != Some(key) {
            return Err(format!(
                "D2 kind {} {m:?}: slot {s} decodes to {:?}",
                m[0],
                decode(s, f)
            ));
        }
        slots_of.push(s);
    }
    let mut sorted = slots_of.clone();
    sorted.sort_unstable();
    if let Some(w) = sorted.windows(2).find(|w| w[0] == w[1]) {
        return Err(format!("D3 two rows on slot {}", w[0]));
    }
    if mask.len() != slots(f.tiles()).div_ceil(32) {
        return Err(format!("D4 a mask of {} words", mask.len()));
    }
    let set: usize = mask.iter().map(|w| w.count_ones() as usize).sum();
    if set != rows.len() || slots_of.iter().any(|&s| mask[s / 32] >> (s % 32) & 1 == 0) {
        return Err(format!(
            "D4 the mask has {set} bits for {} rows",
            rows.len()
        ));
    }
    Ok(slots_of)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames() -> impl Iterator<Item = Frame> {
        [8, 11, 12, 13, 14, 15, 16].into_iter().flat_map(|side| {
            (2..=4)
                .flat_map(move |seats| (1..=seats).map(move |mover| Frame { side, mover, seats }))
        })
    }

    /// S1: every slot decodes to nothing or to fields that encode back to it.
    #[test]
    fn every_slot_round_trips() {
        for f in frames() {
            for slot in 0..slots(f.tiles()) {
                if let Some([k, s, t, o, p]) = decode(slot, &f) {
                    assert_eq!(
                        encode(&[k, s, t, o, p, -1], &f),
                        Ok(slot),
                        "{f:?} slot {slot}"
                    );
                }
            }
        }
    }

    /// S2: the blocks follow each other and add up to the table.
    #[test]
    fn blocks_add_up() {
        for side in [8usize, 11, 14, 16, 30] {
            let n = side * side;
            let b = blocks(n);
            assert_eq!(b[0], 0);
            assert!(b.windows(2).all(|w| w[0] < w[1]));
            assert_eq!(b[5], slots(n));
            assert_eq!(
                [
                    b[1] - b[0],
                    b[2] - b[1],
                    b[3] - b[2],
                    b[4] - b[3],
                    b[5] - b[4]
                ],
                [
                    GLOBAL,
                    UNIT_CHANNELS * n,
                    ATTACK_WINDOW * n,
                    TILE_CHANNELS * n,
                    n * n
                ]
            );
        }
        // the document's sizes
        assert_eq!(
            [64, 121, 196, 256].map(slots),
            [17_440, 39_784, 79_084, 118_624]
        );
    }

    /// Encode refuses every row that does not match its kind's pattern.
    #[test]
    fn mismatched_rows_are_refused() {
        let f = Frame {
            side: 11,
            mover: 1,
            seats: 2,
        };
        for m in [
            [15, 3, -1, -1, -1, -1],    // EndTurn with a source
            [27, -1, -1, -1, 1, -1],    // a treaty with the mover
            [27, -1, -1, -1, 3, -1],    // a treaty with no such seat
            [28, -1, -1, 2, 2, -1],     // a reply other than 0 or 1
            [8, -1, -1, 64, -1, -1],    // a technology past the block
            [2, 0, 4 * 11, -1, -1, -1], // an attack 4 rows away
            [2, 5, 5, -1, -1, -1],      // an attack on its own tile
            [6, 5, 5, -1, -1, -1],      // a move onto its own tile
            [6, 5, 121, -1, -1, -1],    // a move off the board
            [11, -1, 7, 9, -1, -1],     // a reward past 8
            [1, -1, 7, 64, -1, -1],     // an improvement past the block
            [1, 3, 7, 5, -1, -1],       // a build with a source
            [9, -1, 7, 0, -1, -1],      // Destroy with an option
            [16, 3, -1, 40, -1, -1],    // an upgrade to a unit the table has no channel for
            [39, 3, -1, 2, -1, -1],     // clearing an effect other than flood or algae
            [3, 3, 4, -1, -1, -1],      // Recover with a target
            [12, 3, -1, -1, -1, -1],    // a kind the table does not hold
        ] {
            assert!(encode(&m, &f).is_err(), "{m:?}");
        }
        let mut words = Vec::new();
        let row = [6, 0, 1, -1, -1, 0];
        assert!(mask(&[row, row], &f, &mut words).is_err());
        // no table for a frame outside the layout: a fifth seat would run into Research
        for bad in [
            Frame {
                side: 11,
                mover: 1,
                seats: 5,
            },
            Frame {
                side: 11,
                mover: 3,
                seats: 2,
            },
            Frame {
                side: 11,
                mover: 0,
                seats: 2,
            },
            Frame {
                side: 9,
                mover: 1,
                seats: 2,
            },
        ] {
            assert!(encode(&[27, -1, -1, -1, 2, -1], &bad).is_err(), "{bad:?}");
            assert_eq!(decode(14, &bad), None, "{bad:?}");
        }
    }

    /// The diplomacy slots address the other seats from the mover, so every seat's opponents
    /// land on the same slots relative to it.
    #[test]
    fn diplomacy_is_relative_to_the_mover() {
        let f = |mover| Frame {
            side: 11,
            mover,
            seats: 4,
        };
        assert_eq!(encode(&[27, -1, -1, -1, 2, -1], &f(1)), Ok(DIPLOMACY));
        assert_eq!(encode(&[27, -1, -1, -1, 1, -1], &f(4)), Ok(DIPLOMACY));
        assert_eq!(
            encode(&[30, -1, -1, -1, 3, -1], &f(4)),
            Ok(DIPLOMACY + 5 * 2 + 4)
        );
    }
}
