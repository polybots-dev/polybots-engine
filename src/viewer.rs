//! Viewer memory kept in the checkpoint beside the game: `sightings` (the last-seen enemy units) and
//! `history` (ruins seen, turn-start opponent scores), both for the player about to act. They are
//! updated whenever the game builds a new frame:
//! after every decision, unless the game just ended.

use crate::state::Game;
use crate::state::PEACE;
use engine_fast_frame::RememberedUnit;
use std::collections::BTreeMap;

const INVISIBLE: i64 = 3;
const RUIN: i32 = 2;

/// The (tile index, owner, kind, displayed health) of every unit the viewer sees, in tile order.
fn visible_units(g: &Game, viewer: i32) -> Vec<(usize, i32, i32, i32)> {
    g.tiles
        .iter()
        .enumerate()
        .filter(|(_, x)| x.explorers.contains(&viewer))
        .filter_map(|(i, x)| {
            let u = g.units.get(x.unit? as usize)?;
            let hidden = u.owner != viewer
                && crate::menu::unit_effect_list(g, u.effects).contains(&INVISIBLE)
                && crate::menu::relation(g, viewer, u.owner) != PEACE;
            (!hidden).then(|| (i, u.owner, u.kind, (u.health as u16).div_ceil(10) as i32))
        })
        .collect()
}

/// Update the acting player's sightings and history, as the game does for each new frame.
pub fn refresh(g: &mut Game) -> Result<(), String> {
    // the memory leaves the game while it is written, so the game can be read beside it
    let mut shared = std::mem::take(&mut g.memory);
    let result = remember(g, std::sync::Arc::make_mut(&mut shared));
    g.memory = shared;
    result
}

fn remember(g: &Game, memory: &mut crate::state::Memory) -> Result<(), String> {
    let viewer = g.players[g.current_player_index as usize].action.id;
    let turn = g.current_turn;
    // sightings: replace remembered enemy units the viewer sees now, forget own ones
    let seen = visible_units(g, viewer);
    let rows = memory
        .sightings
        .as_mut()
        .ok_or("no sightings")?
        .entry(viewer)
        .or_default();
    let mut remembered: BTreeMap<i32, RememberedUnit> =
        rows.drain(..).map(|r| (r.index, r)).collect();
    for &(index, owner, kind, health_label) in &seen {
        let index = index as i32;
        if owner != viewer {
            remembered.insert(
                index,
                RememberedUnit {
                    index,
                    owner,
                    kind,
                    health_label,
                    turn,
                },
            );
        } else {
            remembered.remove(&index);
        }
    }
    rows.extend(remembered.into_values());
    // history: ruins the viewer has seen, and opponent scores once per turn
    let ruins: Vec<i64> = g
        .tiles
        .iter()
        .enumerate()
        .filter(|(_, x)| {
            x.explorers.contains(&viewer) && x.improvement.as_ref().is_some_and(|m| m.kind == RUIN)
        })
        .map(|(i, _)| i as i64)
        .collect();
    let known = &g.player(viewer).ok_or("missing viewer")?.known_players;
    let opponents: Vec<(i32, i64)> = g
        .players
        .iter()
        .filter(|p| p.action.id != viewer && p.action.id != 255 && known.contains(&p.action.id))
        .map(|p| (p.action.id, p.score))
        .collect();
    let h = memory
        .history
        .as_mut()
        .ok_or("no history")?
        .entry(viewer)
        .or_default();
    for i in ruins {
        if !h.ruins_seen.contains(&i) {
            h.ruins_seen.push(i);
        }
    }
    h.ruins_seen.sort_unstable();
    if h.snapshot_turn != Some(turn) {
        h.snapshot_turn = Some(turn);
        for (id, score) in opponents {
            let next = match h.scores.get(&id) {
                Some(v) => (v.0, score, Some(v.1)),
                None => (score, score, None),
            };
            h.scores.insert(id, next);
        }
    }
    Ok(())
}
