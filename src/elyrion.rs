//! Elyrion's rules: the sanctuary's attraction of game at turn start (the start-of-turn rules,
//! the Attract improvement ability).

use crate::action::{Action, Economy};
use crate::state::{Game, Xy};
use crate::tables::Tables;

const FOREST: i32 = 5;

/// An Attract improvement (the sanctuary) of `p` at `c`, founded on turn `founded`: every
/// `growthRate` turns of its age it puts game on the first adjacent tile (x-major from the
/// north-west corner) that `p` owns, holds no improvement and no resource, and is forest.
pub(crate) fn attract(
    g: &Game,
    t: &Tables,
    p: i32,
    c: Xy,
    kind: i32,
    founded: i32,
    out: &mut Vec<Action>,
) -> Result<(), String> {
    let rate = t
        .improvements
        .get(kind as usize)
        .and_then(|d| d.growth_rate)
        .unwrap_or(0);
    if rate == 0 {
        return Err("attract improvement with growth rate 0".into());
    }
    let age = g.current_turn.wrapping_sub(founded);
    if age.checked_rem(rate).ok_or("attract age division")? != 0 {
        return Ok(());
    }
    for x in c.x - 1..=c.x + 1 {
        for y in c.y - 1..=c.y + 1 {
            let Some(tile) = g.tile(Xy { x, y }) else {
                continue;
            };
            if tile.owner == p
                && tile.improvement.is_none()
                && tile.resource.is_none()
                && tile.terrain == FOREST
            {
                out.push(Action::Economy(Economy::CreateResource {
                    player: p,
                    kind: t.resources.id("game")? as i32,
                    target: tile.coordinates,
                    // the frozen rules123 build records the attraction reason in the queued frame
                    reason: i32::from(g.fixed.version == 123),
                }));
                return Ok(());
            }
        }
    }
    Ok(())
}
