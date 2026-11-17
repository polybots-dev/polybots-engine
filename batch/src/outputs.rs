//! Optional outputs of the batch (the attack previews, defence bonus and
//! city income are engine outputs every model may read, and so is the displayed
//! maximum health of a unit). Each is asked
//! for on its own and costs nothing unless asked for; none is part of the view, since some
//! show what the view hides (engine-fast/src/queries.rs lists which).

use crate::{as_move, sort_key, Batch, Move, Status};
use engine_fast::menu;
use engine_fast::queries::{self, UnitStats};
use engine_fast::state::Game;
use engine_fast::tables::Tables;
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

/// The defence plane of `g` for `viewer`: what `Batch::defence` gives for a slot holding `g`
/// whose player to move is `viewer`.
pub fn defence_plane(g: &Game, t: &Tables, viewer: i32) -> Result<Vec<i16>, String> {
    let mut out = UnitStats::default();
    queries::unit_stats(g, t, viewer, &mut out)?;
    Ok(out.defence)
}

/// The maximum health plane of `g` for `viewer`, as the unit panel displays it: what
/// `Batch::max_health` gives for a slot holding `g` whose player to move is `viewer`.
pub fn max_health_plane(g: &Game, t: &Tables, viewer: i32) -> Result<Vec<i16>, String> {
    let mut out = UnitStats::default();
    queries::unit_stats(g, t, viewer, &mut out)?;
    Ok(out.max_health)
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

    /// The defence bonus of every unit the mover sees that has an info panel, one plane over
    /// the tiles (`queries::UNKNOWN` elsewhere), from `queries::unit_stats`. Can show hidden
    /// state: a foreign unit's effects and its owner's techs. None when the slot is not playing.
    pub fn defence(&self, slot: usize) -> Result<Option<Vec<i16>>, String> {
        self.playing(slot)
            .map(|(g, mover)| defence_plane(g, &self.tables, mover))
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

    /// The maximum health the unit panel displays for every unit the mover sees that has one
    /// (a carrier shows its passenger's unless it has Protect), one plane over the tiles
    /// (`queries::UNKNOWN` elsewhere). Shows a seen foreign carrier's passenger through it, as
    /// the game's screen does. None when the slot is not playing.
    pub fn max_health(&self, slot: usize) -> Result<Option<Vec<i16>>, String> {
        self.playing(slot)
            .map(|(g, mover)| max_health_plane(g, &self.tables, mover))
            .transpose()
    }
}
