//! Optional outputs of the batch. Each is asked for on its own and costs nothing unless asked for; none is part
//! of the view, since some show what the view hides (engine-fast/src/queries.rs lists which).
//! `other_view` is the privileged critic's input: it is never an
//! actor's input.

use crate::{as_move, sort_key, Applied, Batch, Move, Status, SKIP};
use engine_fast::menu;
use engine_fast::queries::{self, UnitStats};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
use engine_fast::turns;
use engine_fast::view::{self, View};
use engine_fast_frame::AttackPreview;

/// The fields of one row of `previews`: whether the row has a preview, the damage it shows on
/// the target and on the attacker (-1: the lethal icon, or no retaliation), and flags.
pub const PREVIEW_FIELDS: [&str; 4] = ["present", "damage_dealt", "damage_taken", "flags"];
/// `flags` bits
pub const KILLS_TARGET: i16 = 1;
pub const ATTACKER_DIES: i16 = 2;
pub const RETALIATION: i16 = 4;

/// One legal row's preview in the `PREVIEW_FIELDS` layout; zeros for a row without one.
pub fn preview_row(p: Option<&AttackPreview>) -> [i16; 4] {
    let Some(p) = p else { return [0; 4] };
    let shown = |x: Option<i32>| x.map_or(-1, |v| v as i16);
    let flags = (if p.kills_target { KILLS_TARGET } else { 0 })
        | (if p.attacker_dies { ATTACKER_DIES } else { 0 })
        | (if p.retaliation { RETALIATION } else { 0 });
    [1, shown(p.damage_dealt), shown(p.damage_taken), flags]
}

/// The legal rows of `g` as the batch hands them out (sorted) and each row's attack preview:
/// what `Batch::moves` and `Batch::previews` give for a slot holding `g`.
pub fn legal_previews(g: &Game, t: &Tables) -> Result<(Vec<Move>, Vec<[i16; 4]>), String> {
    let mut out: Vec<(Move, [i16; 4])> = menu::menu(g, t)?
        .iter()
        .map(|c| {
            (
                as_move(g.width, c),
                preview_row(c.details.attack_preview.as_ref()),
            )
        })
        .collect();
    out.sort_by_key(|(m, _)| sort_key(m));
    Ok(out.into_iter().unzip())
}

/// The optional outputs a batch can be made with, by name.
pub const OUTPUTS: [&str; 13] = [
    "previews",
    "defence",
    "income",
    "max_health",
    "other_view",
    "movement",
    "attack_modifier",
    "city_work",
    "city_units",
    "city_siege",
    "city_score",
    "border_edges",
    "progression",
];

/// The optional outputs that are one int16 plane over the tiles for the player to move
/// (`queries::UNKNOWN` where it has no answer), in the order the GPU batch keeps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    /// `unit_stats`' defence bonus in tenths (10: none)
    Defence,
    /// `unit_stats`' maximum health, as the unit panel displays it
    MaxHealth,
    /// `unit_stats`' movement with the unit's effects applied
    Movement,
    /// `unit_stats`' attack modifier (tenths of the displayed attack; 50 when boosted)
    AttackModifier,
    /// `queries::city_work`, masked
    CityWork,
    /// `queries::city_units`, masked
    CityUnits,
    /// `queries::city_siege`, masked and without the besieger
    CitySiege,
    /// `queries::border_edges`
    BorderEdges,
}

impl Plane {
    pub const ALL: [Plane; 8] = [
        Plane::Defence,
        Plane::MaxHealth,
        Plane::Movement,
        Plane::AttackModifier,
        Plane::CityWork,
        Plane::CityUnits,
        Plane::CitySiege,
        Plane::BorderEdges,
    ];

    /// Its name in `OUTPUTS`.
    pub fn name(self) -> &'static str {
        match self {
            Plane::Defence => "defence",
            Plane::MaxHealth => "max_health",
            Plane::Movement => "movement",
            Plane::AttackModifier => "attack_modifier",
            Plane::CityWork => "city_work",
            Plane::CityUnits => "city_units",
            Plane::CitySiege => "city_siege",
            Plane::BorderEdges => "border_edges",
        }
    }

    pub fn named(name: &str) -> Option<Plane> {
        Plane::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The plane of `g` for `viewer`: what `Batch::plane` gives for a slot holding `g` whose
    /// player to move is `viewer`.
    pub fn of(self, g: &Game, t: &Tables, viewer: i32) -> Result<Vec<i16>, String> {
        let mut out = Vec::new();
        match self {
            Plane::Defence | Plane::MaxHealth | Plane::Movement | Plane::AttackModifier => {
                let mut s = UnitStats::default();
                queries::unit_stats(g, t, viewer, &mut s)?;
                out = match self {
                    Plane::Defence => s.defence,
                    Plane::MaxHealth => s.max_health,
                    Plane::Movement => s.movement,
                    _ => s.attack_modifier,
                };
            }
            Plane::CityWork => queries::city_work(g, t, viewer, &mut out)?,
            Plane::CityUnits => queries::city_units(g, viewer, &mut out),
            Plane::CitySiege => queries::city_siege(g, viewer, &mut out),
            Plane::BorderEdges => queries::border_edges(g, viewer, &mut out),
        }
        Ok(out)
    }
}

/// The `city_score` plane of `g` for `viewer` (int32: a score need not fit int16).
pub fn city_score_plane(g: &Game, t: &Tables, viewer: i32) -> Vec<i32> {
    let mut out = Vec::new();
    queries::city_score(g, t, viewer, &mut out);
    out
}

/// `progression`'s blocks in order, each `rows` rows of `fields`: the viewer's units lost,
/// start tile and revealed resource kinds (bits by kind); its monuments in the order built;
/// its tasks as stored (`View::tasks`); the lighthouses on board corners it has explored, with
/// the seats that explored each in order (0 past the end); and per row of `players` (the
/// viewer, then the players it has met) the embassy levels both ways. A row past the list's
/// end is -1 throughout.
pub const PROGRESSION: [(&str, usize, &[&str]); 7] = [
    ("units_lost", 1, &["units_lost"]),
    ("start_tile", 1, &["start_tile"]),
    ("revealed_resources", 1, &["revealed_resources"]),
    ("monuments", MONUMENTS, &["kind"]),
    ("tasks", TASKS, &["kind", "completed", "counter"]),
    (
        "lighthouses",
        LIGHTHOUSES,
        &[
            "tile",
            "explorer_1",
            "explorer_2",
            "explorer_3",
            "explorer_4",
            "explorer_5",
        ],
    ),
    // `theirs`: that player's embassy in the viewer's capital (the viewer's relation record
    // of it); `ours`: the viewer's in that player's (its record of the viewer)
    ("embassy", VIEW_SEATS, &["theirs", "ours"]),
];
pub const MONUMENTS: usize = 8;
pub const TASKS: usize = 10;
pub const LIGHTHOUSES: usize = 4;
/// int32 entries in one `progression` row
pub const PROGRESSION_LEN: usize = 3 + MONUMENTS + TASKS * 3 + LIGHTHOUSES * 6 + VIEW_SEATS * 2;

/// The embassy level `of` keeps in its relation record of `other` (0 without one).
fn embassy(g: &Game, of: i32, other: i32) -> i32 {
    g.player(of)
        .and_then(|p| p.relation(other))
        .map_or(0, |r| r.embassy_level)
}

/// `v`, the view of `g` for its viewer, laid out as one `progression` row. Refuses a view
/// larger than the row.
pub fn progression_row(g: &Game, v: &View, out: &mut [i32]) -> Result<(), String> {
    if out.len() != PROGRESSION_LEN
        || v.monuments.len() > MONUMENTS
        || v.tasks.len() > TASKS
        || v.lighthouses.len() > LIGHTHOUSES
        || v.players.len() > VIEW_SEATS
    {
        return Err("a view larger than its progression row".into());
    }
    out.fill(-1);
    let viewer = i32::from(v.viewer);
    out[0] = v.units_lost;
    out[1] = i32::from(v.start_tile);
    out[2] = v.revealed_resources as i32;
    let (monuments, rest) = out[3..].split_at_mut(MONUMENTS);
    for (slot, &m) in monuments.iter_mut().zip(&v.monuments) {
        *slot = i32::from(m);
    }
    let (tasks, rest) = rest.split_at_mut(TASKS * 3);
    for (row, task) in tasks.chunks_mut(3).zip(&v.tasks) {
        row.copy_from_slice(&[
            i32::from(task.kind),
            i32::from(task.completed),
            task.counter,
        ]);
    }
    let (lighthouses, embassies) = rest.split_at_mut(LIGHTHOUSES * 6);
    for (row, l) in lighthouses.chunks_mut(6).zip(&v.lighthouses) {
        row[0] = i32::from(l.tile);
        for (slot, &e) in row[1..].iter_mut().zip(&l.explorers) {
            *slot = i32::from(e);
        }
    }
    for (row, p) in embassies.chunks_mut(2).zip(&v.players) {
        let seat = i32::from(p.seat);
        row.copy_from_slice(&[embassy(g, viewer, seat), embassy(g, seat, viewer)]);
    }
    Ok(())
}

/// The other seat's view of `g` (`turns::other_seat`: the next player in turn order after
/// the one to move): what `Batch::other_view` gives for a slot holding `g`. None in a game of
/// one player.
pub fn other_view(g: &Game, t: &Tables) -> Result<Option<View>, String> {
    let Some(seat) = turns::other_seat(g)? else {
        return Ok(None);
    };
    let mut out = View::default();
    view::write(g, t, seat, &mut out)?;
    Ok(Some(out))
}

/// The fields of a view's scalar row and of each player row, as the Python package's `views`
/// hands them out, and how many player rows there are: the viewer, then up to three it has met.
pub const SCALAR_FIELDS: [&str; 3] = ["viewer", "turn", "stars"];
pub const PLAYER_FIELDS: [&str; 4] = ["seat", "tribe", "relation", "score"];
pub const VIEW_SEATS: usize = 4;

/// Lay `v` out in fixed-size rows, -1 where unused: `tiles` its planes (`v.tiles` exactly),
/// `scalars` the `SCALAR_FIELDS`, `players` `VIEW_SEATS` rows of `PLAYER_FIELDS`, `tech` its
/// technologies in ascending order. Refuses a view larger than the rows.
pub fn view_rows(
    v: &View,
    tiles: &mut [i16],
    scalars: &mut [i32],
    players: &mut [i64],
    tech: &mut [i16],
) -> Result<(), String> {
    if v.tiles.len() != tiles.len()
        || scalars.len() != SCALAR_FIELDS.len()
        || players.len() != VIEW_SEATS * PLAYER_FIELDS.len()
        || v.players.len() > VIEW_SEATS
        || v.tech.len() > tech.len()
    {
        return Err("a view larger than its rows".into());
    }
    tiles.copy_from_slice(&v.tiles);
    scalars.copy_from_slice(&[i32::from(v.viewer), v.turn, v.stars]);
    players.fill(-1);
    for (row, p) in players.chunks_mut(PLAYER_FIELDS.len()).zip(&v.players) {
        row.copy_from_slice(&[
            i64::from(p.seat),
            i64::from(p.tribe),
            i64::from(p.relation),
            p.score,
        ]);
    }
    tech.fill(-1);
    tech[..v.tech.len()].copy_from_slice(&v.tech);
    Ok(())
}

impl Batch {
    /// The game of a playing slot and its player to move.
    fn playing(&self, slot: usize) -> Option<(&Game, i32)> {
        let s = &self.slots[slot];
        let (Status::Playing, Some(g)) = (s.status, s.game.as_ref()) else {
            return None;
        };
        Some((g, g.players[g.current_player_index as usize].action.id))
    }

    /// The attack preview of each of `slot`'s legal rows, in `moves(slot)` order (only legal
    /// attacks between units the mover sees have one; no pairwise table). Empty when the slot
    /// is not playing. Shows hidden state as the game's attack forecast does: a seen foreign
    /// defender's private effects and its owner's defence techs.
    pub fn previews(&self, slot: usize) -> Vec<[i16; 4]> {
        let s = &self.slots[slot];
        if s.status != Status::Playing {
            return Vec::new();
        }
        s.offered
            .iter()
            .map(|c| preview_row(c.details.attack_preview.as_ref()))
            .collect()
    }

    /// Plane `p` for the player to move (`Plane::of`). Some show hidden state as the game's
    /// screen does. None when the slot is not
    /// playing.
    pub fn plane(&self, slot: usize, p: Plane) -> Result<Option<Vec<i16>>, String> {
        self.playing(slot)
            .map(|(g, mover)| p.of(g, &self.tables, mover))
            .transpose()
    }

    /// The income the player to move is shown (`queries::income`: units hidden from it count
    /// as absent; an embassy's income reads the foreign capital, as the game's screen does).
    /// None when the slot is not playing.
    pub fn income(&self, slot: usize) -> Result<Option<i32>, String> {
        self.playing(slot)
            .map(|(g, mover)| queries::income(g, &self.tables, mover))
            .transpose()
    }

    /// The score of each city the player to move sees (`queries::city_score`; a foreign
    /// city's shows its population progress, as the building popup does). None when the slot
    /// is not playing.
    pub fn city_score(&self, slot: usize) -> Option<Vec<i32>> {
        self.playing(slot)
            .map(|(g, mover)| city_score_plane(g, &self.tables, mover))
    }

    /// The player to move's `progression` row (`progression_row` of its view). None when the
    /// slot is not playing.
    pub fn progression(&self, slot: usize) -> Result<Option<Vec<i32>>, String> {
        let (Some((g, _)), Some(v)) = (self.playing(slot), self.view(slot)) else {
            return Ok(None);
        };
        let mut out = vec![0; PROGRESSION_LEN];
        progression_row(g, v, &mut out)?;
        Ok(Some(out))
    }

    /// The view of the next seat in turn order after the player to move (`other_view`), for
    /// the privileged critic only: it shows what the mover's view hides. None when the slot is
    /// not playing or the game has one player. Refused for a game put in with `load`, which is
    /// someone's live game: there the other seat's view is the opponent's hidden information.
    pub fn other_view(&self, slot: usize) -> Result<Option<View>, String> {
        if self.slots.get(slot).is_some_and(|s| s.seat.is_some()) {
            return Err("other_view is not given for a loaded game".into());
        }
        Ok(self
            .playing(slot)
            .map(|(g, _)| other_view(g, &self.tables))
            .transpose()?
            .flatten())
    }
}

/// The action table's frame for `g`: its side, the player to move and the seats. Refuses a
/// game whose players (Nature aside) are not ids 1 to their number, which the table's
/// diplomacy slots assume.
pub fn table_frame(g: &Game) -> Result<crate::table::Frame, String> {
    let ids: Vec<i32> = g
        .player_ids()
        .iter()
        .copied()
        .filter(|&p| p != 255)
        .collect();
    let seats = ids.len() as i32;
    if !(1..=seats).all(|k| ids.contains(&k)) {
        return Err(format!("players {ids:?} are not 1 to {seats}"));
    }
    let f = crate::table::Frame {
        side: g.width,
        mover: g.players[g.current_player_index as usize].action.id,
        seats,
    };
    f.valid()?;
    Ok(f)
}

impl Batch {
    /// The action-table slot of each of `slot`'s legal rows, in `moves(slot)` order, into
    /// `rows`, and the legality mask into `mask` (`table::mask`). False, with both empty, when
    /// the slot is not playing.
    pub fn table(
        &self,
        slot: usize,
        rows: &mut Vec<u32>,
        mask: &mut Vec<u32>,
    ) -> Result<bool, String> {
        rows.clear();
        mask.clear();
        let Some((g, _)) = self.playing(slot) else {
            return Ok(false);
        };
        let f = table_frame(g)?;
        let mut slots_of = Vec::with_capacity(self.slots[slot].moves.len());
        for m in &self.slots[slot].moves {
            slots_of.push(crate::table::encode(m, &f)?);
        }
        crate::table::fill_mask(&slots_of, (f.side * f.side) as usize, mask)?;
        rows.extend(slots_of.iter().map(|&s| s as u32));
        Ok(true)
    }

    /// `step` with one action-table slot per game (or `SKIP`) in place of a row index: the
    /// slot's move is found in the game's legal list by its identifying fields, and a slot no
    /// legal row holds is rejected like an index out of range.
    pub fn step_table(&mut self, slots: &[i64], applied: &mut [Applied]) -> Result<(), String> {
        if slots.len() != self.slots.len() {
            return Err("one slot per game".into());
        }
        let rows: Vec<i32> = slots
            .iter()
            .enumerate()
            .map(|(k, &s)| {
                if s == i64::from(SKIP) {
                    return SKIP;
                }
                let Some((g, _)) = self.playing(k) else {
                    return 0;
                };
                let moves = &self.slots[k].moves;
                let found = usize::try_from(s)
                    .ok()
                    .zip(table_frame(g).ok())
                    .and_then(|(s, f)| crate::table::decode(s, &f))
                    .and_then(|key| moves.binary_search_by_key(&key, sort_key).ok());
                found.map_or(moves.len() as i32, |i| i as i32)
            })
            .collect();
        self.step(&rows, applied)
    }
}
