//! The queued actions as the flat records of `kernel/action.h`, and their conversion with the
//! Rust engine's `Action`. The field each variant uses is the one `EF_ACTIONS` lists for it;
//! `to_device` and `from_device` must agree with that list and with each other (the round
//! trip in `tests/state_roundtrip.rs` checks the second, the step lockstep the first).

use crate::state::{EfTrigger, EfXy};
use engine_fast::action::{
    Action, City, CityConnection, Combat, Diplomacy, Economy, MoveEffect, Payload, TurnEffect,
};
use engine_fast::state::{Trigger, Xy};

pub const PATH_TILES: usize = 13;
pub const RESIGN_TRIGGERS: usize = 4;

#[repr(C)]
#[derive(Clone, Copy)]
pub union EfList {
    pub path: [EfXy; PATH_TILES],
    pub triggers: [EfTrigger; RESIGN_TRIGGERS],
    pub players: [i32; 5],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfAction {
    pub kind: i32,
    pub player: i32,
    pub i: [i32; 6],
    pub w: [i64; 2],
    pub p: [EfXy; 3],
    pub len: i32,
    pub pad: i32,
    pub list: EfList,
}

/// The kinds, numbered as `EF_ACTIONS` lists them.
pub mod kind {
    pub const END_COMMAND: i32 = 0;
    pub const RUN_DEFERRED: i32 = 1;
    pub const INCREASE_SCORE: i32 = 2;
    pub const INCREASE_CURRENCY: i32 = 3;
    pub const DECREASE_SCORE: i32 = 4;
    pub const ATTACK: i32 = 5;
    pub const KILL: i32 = 6;
    pub const CAPTURE_CITY: i32 = 7;
    pub const INFILTRATE: i32 = 8;
    pub const INFILTRATION_REWARD: i32 = 9;
    pub const CONVERT: i32 = 10;
    pub const PROMOTE: i32 = 11;
    pub const BUILD: i32 = 12;
    pub const BUILD_ROAD: i32 = 13;
    pub const DESTROY_RESOURCE: i32 = 14;
    pub const CREATE_RESOURCE: i32 = 15;
    pub const LEVEL_UP_IMPROVEMENT: i32 = 16;
    pub const LEVEL_DOWN_IMPROVEMENT: i32 = 17;
    pub const CITY_REWARD: i32 = 18;
    pub const SCOUT: i32 = 19;
    pub const EXPAND_CITY: i32 = 20;
    pub const MODIFY_PRODUCTION: i32 = 21;
    pub const MODIFY_SCORE: i32 = 22;
    pub const HOUDINI: i32 = 23;
    pub const TRAIN: i32 = 24;
    pub const INCREASE_POPULATION: i32 = 25;
    pub const DECREASE_POPULATION: i32 = 26;
    pub const LEVEL_UP: i32 = 27;
    pub const MOVE: i32 = 28;
    pub const RESELECT: i32 = 29;
    pub const REVEAL: i32 = 30;
    pub const HIDE: i32 = 31;
    pub const EMBARK: i32 = 32;
    pub const DISEMBARK: i32 = 33;
    pub const DESTROY_IMPROVEMENT: i32 = 34;
    pub const CONNECT: i32 = 35;
    pub const CHANGE: i32 = 36;
    pub const DISCONNECT: i32 = 37;
    pub const HEAL_OTHERS: i32 = 38;
    pub const HEAL: i32 = 39;
    pub const RESIGN: i32 = 40;
    pub const UPGRADE: i32 = 41;
    pub const RECOVER: i32 = 42;
    pub const END_TURN: i32 = 43;
    pub const START_TURN: i32 = 44;
    pub const PASS_PLAYER: i32 = 45;
    pub const END_MATCH: i32 = 46;
    pub const RECEIVE_MESSAGE: i32 = 47;
    pub const BREAK_PEACE: i32 = 48;
    pub const PEACE_RESPONSE: i32 = 49;
    pub const ESTABLISH_EMBASSY: i32 = 50;
    pub const PEACE_TREATY: i32 = 51;
    pub const EXPLORE: i32 = 52;
    pub const MEET: i32 = 53;
    pub const UPDATE_ROUTES: i32 = 54;
    pub const UPDATE_CITY_CONNECTIONS: i32 = 55;
    pub const ENABLE_TASK: i32 = 56;
    pub const TASK_COMPLETED: i32 = 57;
    pub const DESTROY_EMBASSY: i32 = 58;
    pub const GAME_OVER: i32 = 59;
    pub const WIPE_PLAYER: i32 = 60;
    pub const WIPE_PLAYER_END: i32 = 61;
    pub const START_MATCH: i32 = 62;
    pub const RESEARCH: i32 = 63;
    pub const RUINS: i32 = 64;
    pub const DISBAND_UNIT: i32 = 65;
    pub const FREEZE_UNIT: i32 = 66;
    pub const FLOOD: i32 = 67;
    pub const POISON: i32 = 68;
    pub const EXPLODE: i32 = 69;
    pub const EAT: i32 = 70;
    pub const GROW_UP: i32 = 71;
    pub const BUBBLE: i32 = 72;
    pub const FREEZE_TILE: i32 = 73;
    pub const FREEZE_AREA: i32 = 74;
    pub const FREEZE_BORDER: i32 = 75;
    pub const MOVE_HEAL_OTHERS: i32 = 76;
    pub const BREAK_ICE_AREA: i32 = 77;
    pub const CLEAR_TILE_EFFECT: i32 = 78;
    pub const BOOST_OTHERS: i32 = 79;
    pub const BOOST: i32 = 80;
    pub const DECOMPOSE: i32 = 81;
    pub const BREAK_ICE: i32 = 82;
    pub const CHANGE_CLIMATE: i32 = 83;
    pub const COUNT: i32 = 66;
}

impl EfAction {
    fn new(kind: i32, player: i32) -> EfAction {
        EfAction {
            kind,
            player,
            i: [0; 6],
            w: [0; 2],
            p: [EfXy::default(); 3],
            len: 0,
            pad: 0,
            // the path is the union's largest member, so this zeroes every byte; the other
            // members are then written over it field by field, leaving the rest zero
            list: EfList {
                path: [EfXy::default(); PATH_TILES],
            },
        }
    }
    fn at(mut self, c: Xy) -> EfAction {
        self.p[0] = xy(c);
        self
    }
    fn at2(mut self, c: Xy) -> EfAction {
        self.p[1] = xy(c);
        self
    }
    fn int(mut self, k: usize, v: i32) -> EfAction {
        self.i[k] = v;
        self
    }
    fn wide(mut self, v: i64) -> EfAction {
        self.w[0] = v;
        self
    }
    fn path(mut self, path: &[Xy]) -> Result<EfAction, String> {
        if path.len() > PATH_TILES {
            return Err("a path longer than the device action holds".into());
        }
        let mut list = [EfXy::default(); PATH_TILES];
        for (slot, c) in list.iter_mut().zip(path) {
            *slot = xy(*c);
        }
        self.list.path = list;
        self.len = path.len() as i32;
        Ok(self)
    }

    /// The path, triggers or players this action carries; the union is all integers, so
    /// every view of it is valid.
    fn path_items(&self) -> Result<Vec<Xy>, String> {
        let n = self.count(PATH_TILES)?;
        // SAFETY: every field of the union is plain integers, so any bytes are a valid path.
        Ok(unsafe { self.list.path }[..n]
            .iter()
            .map(|c| back(*c))
            .collect())
    }
    fn count(&self, max: usize) -> Result<usize, String> {
        usize::try_from(self.len)
            .ok()
            .filter(|&n| n <= max)
            .ok_or_else(|| "action list length out of range".into())
    }
}

fn xy(c: Xy) -> EfXy {
    EfXy { x: c.x, y: c.y }
}

fn back(c: EfXy) -> Xy {
    Xy { x: c.x, y: c.y }
}

/// The action as a device record.
pub fn to_device(a: &Action) -> Result<EfAction, String> {
    use kind::*;
    let n = EfAction::new;
    Ok(match a {
        Action::Payload(Payload::EndCommand { player, command }) => {
            n(END_COMMAND, *player).wide(*command as i64)
        }
        Action::Payload(Payload::RunDeferred { player }) => n(RUN_DEFERRED, *player),
        Action::Payload(Payload::IncreaseScore {
            player,
            amount,
            delay,
            source,
        }) => n(INCREASE_SCORE, *player)
            .int(0, *amount)
            .int(1, *delay)
            .at(*source),
        Action::Payload(Payload::IncreaseCurrency {
            player,
            source,
            delay,
            amount,
        }) => n(INCREASE_CURRENCY, *player)
            .int(0, *amount)
            .int(1, *delay)
            .at(*source),
        Action::Combat(c) => match c {
            Combat::DecreaseScore { player, amount } => n(DECREASE_SCORE, *player).int(0, *amount),
            Combat::Attack {
                player,
                origin,
                target,
                damage,
                animation,
                should_move_to_target,
                delay,
            } => n(ATTACK, *player)
                .at(*origin)
                .at2(*target)
                .int(0, *damage)
                .int(1, *animation)
                .int(2, i32::from(*should_move_to_target))
                .int(3, *delay),
            Combat::Kill {
                player,
                coordinates,
            } => n(KILL, *player).at(*coordinates),
            Combat::CaptureCity {
                player,
                coordinates,
                old_owner,
            } => n(CAPTURE_CITY, *player).at(*coordinates).int(0, *old_owner),
            Combat::Infiltrate {
                player,
                origin,
                target,
            } => n(INFILTRATE, *player).at(*origin).at2(*target),
            Combat::InfiltrationReward {
                player,
                reward,
                coordinates,
                kind,
            } => n(INFILTRATION_REWARD, *player)
                .wide(*reward)
                .at(*coordinates)
                .int(0, *kind),
            Combat::Convert {
                player,
                origin,
                target,
            } => n(CONVERT, *player).at(*origin).at2(*target),
            Combat::Promote {
                player,
                coordinates,
            } => n(PROMOTE, *player).at(*coordinates),
            Combat::FreezeUnit {
                player,
                origin,
                target,
                damage,
            } => n(FREEZE_UNIT, *player)
                .at(*origin)
                .at2(*target)
                .int(0, *damage),
            Combat::Flood {
                player,
                coordinates,
            } => n(FLOOD, *player).at(*coordinates),
            Combat::Poison {
                player,
                origin,
                target,
            } => n(POISON, *player).at(*origin).at2(*target),
            Combat::Explode {
                player,
                coordinates,
                home_coordinates,
            } => n(EXPLODE, *player).at(*coordinates).at2(*home_coordinates),
            Combat::Eat {
                player,
                unit,
                coordinates,
            } => n(EAT, *player).int(0, *unit as i32).at(*coordinates),
            Combat::Upgrade {
                player,
                kind,
                coordinates,
                cost,
            } => n(GROW_UP, *player)
                .int(0, *kind)
                .at(*coordinates)
                .int(1, *cost),
            Combat::Bubble { player, unit, add } => n(BUBBLE, *player)
                .int(0, *unit as i32)
                .int(1, i32::from(*add)),
            Combat::FreezeTile {
                player,
                coordinates,
            } => n(FREEZE_TILE, *player).at(*coordinates),
            Combat::FreezeArea {
                player,
                coordinates,
                radius,
                freeze_units,
                only_owned,
            } => n(FREEZE_AREA, *player)
                .at(*coordinates)
                .int(0, *radius)
                .int(1, i32::from(*freeze_units))
                .int(2, i32::from(*only_owned)),
        },
        Action::Economy(e) => match e {
            Economy::Build {
                player,
                kind,
                target,
                pay,
            } => n(BUILD, *player)
                .int(0, *kind)
                .at(*target)
                .int(1, i32::from(*pay)),
            Economy::BuildRoad { player, target } => n(BUILD_ROAD, *player).at(*target),
            Economy::DestroyResource { player, target } => n(DESTROY_RESOURCE, *player).at(*target),
            Economy::CreateResource {
                player,
                kind,
                target,
                reason,
            } => n(CREATE_RESOURCE, *player)
                .int(0, *kind)
                .at(*target)
                .int(1, *reason),
            Economy::ImprovementLevelUp { player, target } => {
                n(LEVEL_UP_IMPROVEMENT, *player).at(*target)
            }
            Economy::ImprovementLevelDown {
                player,
                target,
                amount,
            } => n(LEVEL_DOWN_IMPROVEMENT, *player)
                .at(*target)
                .int(0, *amount),
            Economy::CityReward {
                player,
                reward,
                target,
            } => n(CITY_REWARD, *player).int(0, *reward).at(*target),
            Economy::Scout {
                player,
                unit,
                remaining,
                seed,
                from,
                path,
                behaviour,
            } => n(SCOUT, *player)
                .int(0, *unit as i32)
                .wide(*remaining)
                .int(1, *seed as i32)
                .at(*from)
                .int(2, *behaviour)
                .path(&path.iter().copied().collect::<Vec<Xy>>())?,
            Economy::ExpandCity { player, target } => n(EXPAND_CITY, *player).at(*target),
            Economy::ModifyProduction {
                player,
                amount,
                target,
            } => n(MODIFY_PRODUCTION, *player).int(0, *amount).at(*target),
            Economy::ModifyScore {
                player,
                amount,
                target,
            } => n(MODIFY_SCORE, *player).int(0, *amount).at(*target),
            Economy::Houdini { player, target } => n(HOUDINI, *player).at(*target),
            Economy::Train {
                player,
                kind,
                target,
                cost,
                direction_target,
                train_action_aborted,
            } => n(TRAIN, *player)
                .int(0, *kind)
                .at(*target)
                .int(1, *cost)
                .at2(*direction_target)
                .int(2, i32::from(*train_action_aborted)),
            Economy::FreezeArea {
                player,
                target,
                radius,
                freeze_units,
                only_owned_tiles,
            } => n(FREEZE_BORDER, *player)
                .at(*target)
                .int(0, *radius)
                .int(1, i32::from(*freeze_units))
                .int(2, i32::from(*only_owned_tiles)),
        },
        Action::City(c) => match c {
            City::IncreasePopulation {
                player,
                source,
                target,
                delay,
            } => n(INCREASE_POPULATION, *player)
                .at(*source)
                .at2(*target)
                .int(0, *delay),
            City::DecreasePopulation {
                player,
                target,
                delay,
            } => n(DECREASE_POPULATION, *player).at(*target).int(0, *delay),
            City::LevelUp { player, target } => n(LEVEL_UP, *player).at(*target),
        },
        Action::MoveEffect(m) => match m {
            MoveEffect::Move {
                player,
                unit,
                path,
                should_animate,
                reason,
            } => n(MOVE, *player)
                .int(0, *unit as i32)
                .int(1, i32::from(*should_animate))
                .int(2, *reason)
                .path(&path.iter().copied().collect::<Vec<Xy>>())?,
            MoveEffect::Reselect { player, unit } => n(RESELECT, *player).int(0, *unit as i32),
            MoveEffect::Reveal {
                player,
                coordinates,
                origin,
                show_popup,
            } => n(REVEAL, *player)
                .at(*coordinates)
                .at2(*origin)
                .int(0, i32::from(*show_popup)),
            MoveEffect::Hide {
                player,
                coordinates,
            } => n(HIDE, *player).at(*coordinates),
            MoveEffect::Embark {
                player,
                coordinates,
            } => n(EMBARK, *player).at(*coordinates),
            MoveEffect::Disembark {
                player,
                coordinates,
            } => n(DISEMBARK, *player).at(*coordinates),
            MoveEffect::DestroyImprovement {
                player,
                coordinates,
            } => n(DESTROY_IMPROVEMENT, *player).at(*coordinates),
            MoveEffect::HealOthers {
                player,
                coordinates,
            } => n(MOVE_HEAL_OTHERS, *player).at(*coordinates),
        },
        Action::CityConnection(c) => match c {
            CityConnection::Connect {
                player,
                coordinates,
            } => n(CONNECT, *player).at(*coordinates),
            CityConnection::Change {
                player,
                coordinates,
            } => n(CHANGE, *player).at(*coordinates),
            CityConnection::Disconnect {
                player,
                coordinates,
            } => n(DISCONNECT, *player).at(*coordinates),
        },
        Action::TurnEffect(e) => match e {
            TurnEffect::HealOthers {
                player,
                coordinates,
            } => n(HEAL_OTHERS, *player).at(*coordinates),
            TurnEffect::Heal {
                player,
                coordinates,
                fixed_heal,
            } => n(HEAL, *player).at(*coordinates).int(0, *fixed_heal),
            TurnEffect::Resign {
                player,
                resigned_player,
                kicker_player,
                was_kicked,
                pending_triggers,
            } => {
                if pending_triggers.len() > RESIGN_TRIGGERS {
                    return Err(
                        "more pending choices in a resignation than the device action holds".into(),
                    );
                }
                let mut r = n(RESIGN, *player)
                    .int(0, *resigned_player)
                    .int(1, *kicker_player)
                    .int(2, i32::from(*was_kicked));
                let mut list = [EfTrigger {
                    player: 0,
                    opponent: 0,
                    kind: 0,
                    coordinates: EfXy::default(),
                }; RESIGN_TRIGGERS];
                for (slot, t) in list.iter_mut().zip(pending_triggers) {
                    *slot = EfTrigger {
                        player: t.player,
                        opponent: t.opponent,
                        kind: t.kind,
                        coordinates: xy(t.coordinates),
                    };
                }
                r.list.triggers = list;
                r.len = pending_triggers.len() as i32;
                r
            }
            TurnEffect::Upgrade {
                player,
                coordinates,
                unit,
                cost,
            } => n(UPGRADE, *player)
                .at(*coordinates)
                .int(0, *unit)
                .int(1, *cost),
            TurnEffect::Recover {
                player,
                coordinates,
            } => n(RECOVER, *player).at(*coordinates),
            TurnEffect::EndTurn { player } => n(END_TURN, *player),
            TurnEffect::StartTurn { player } => n(START_TURN, *player),
            TurnEffect::PassPlayer { player } => n(PASS_PLAYER, *player),
            TurnEffect::EndMatch { player } => n(END_MATCH, *player),
            TurnEffect::ReceiveDiplomacyMessage {
                player,
                sender,
                kind,
            } => n(RECEIVE_MESSAGE, *player).int(0, *sender).wide(*kind),
            TurnEffect::BreakIceArea {
                player,
                coordinates,
                radius,
            } => n(BREAK_ICE_AREA, *player).at(*coordinates).int(0, *radius),
            TurnEffect::ClearTileEffect {
                player,
                coordinates,
                effect,
                triggered_by_unit,
            } => n(CLEAR_TILE_EFFECT, *player)
                .at(*coordinates)
                .int(0, *effect)
                .int(1, i32::from(*triggered_by_unit)),
            TurnEffect::BoostOthers {
                player,
                coordinates,
            } => n(BOOST_OTHERS, *player).at(*coordinates),
            TurnEffect::Boost {
                player,
                coordinates,
            } => n(BOOST, *player).at(*coordinates),
            TurnEffect::Decompose {
                player,
                coordinates,
            } => n(DECOMPOSE, *player).at(*coordinates),
            TurnEffect::BreakIce {
                player,
                coordinates,
            } => n(BREAK_ICE, *player).at(*coordinates),
            TurnEffect::ChangeClimate {
                player,
                coordinates,
                climate,
            } => n(CHANGE_CLIMATE, *player).at(*coordinates).int(0, *climate),
        },
        Action::Diplomacy(d) => match d {
            Diplomacy::BreakPeace { player, opponent } => n(BREAK_PEACE, *player).int(0, *opponent),
            Diplomacy::PeaceResponse {
                player,
                opponent,
                accepted,
            } => n(PEACE_RESPONSE, *player)
                .int(0, *opponent)
                .int(1, i32::from(*accepted)),
            Diplomacy::EstablishEmbassy { player, opponent } => {
                n(ESTABLISH_EMBASSY, *player).int(0, *opponent)
            }
            Diplomacy::PeaceTreaty { player, opponent } => {
                n(PEACE_TREATY, *player).int(0, *opponent)
            }
        },
        Action::Explore {
            player,
            coordinates,
        } => n(EXPLORE, *player).at(*coordinates),
        Action::Meet {
            player,
            other,
            coordinates,
            technology,
        } => n(MEET, *player)
            .int(0, *other)
            .at(*coordinates)
            .int(1, *technology),
        Action::UpdateRoutes { player } => n(UPDATE_ROUTES, *player),
        Action::UpdateCityConnections { actor, players } => {
            let ids: Vec<i32> = players.iter().copied().collect();
            if ids.len() > 5 {
                return Err(
                    "more players in a connection update than the device action holds".into(),
                );
            }
            let mut list = [0i32; 5];
            list[..ids.len()].copy_from_slice(&ids);
            let mut r = n(UPDATE_CITY_CONNECTIONS, *actor);
            r.list.players = list;
            r.len = ids.len() as i32;
            r
        }
        Action::EnableTask { player, task } => n(ENABLE_TASK, *player).int(0, *task as i32),
        Action::TaskCompleted { player, task } => n(TASK_COMPLETED, *player).int(0, *task as i32),
        Action::DestroyEmbassy { player, opponent } => {
            n(DESTROY_EMBASSY, *player).int(0, *opponent)
        }
        Action::GameOver { player, winner } => n(GAME_OVER, *player).int(0, *winner),
        Action::WipePlayer { player, target } => n(WIPE_PLAYER, *player).int(0, *target),
        Action::WipePlayerEnd { player, target } => n(WIPE_PLAYER_END, *player).int(0, *target),
        Action::StartMatch { player } => n(START_MATCH, *player),
        Action::Research {
            player,
            technology,
            cost,
        } => n(RESEARCH, *player).wide(*technology).int(0, *cost),
        Action::Ruins {
            player,
            reward,
            coordinates,
        } => n(RUINS, *player).wide(*reward).at(*coordinates),
        Action::DisbandUnit {
            player,
            coordinates,
            home_coordinates,
            get_paid,
        } => n(DISBAND_UNIT, *player)
            .at(*coordinates)
            .at2(*home_coordinates)
            .int(0, i32::from(*get_paid)),
    })
}

/// The device record as an action.
pub fn from_device(a: &EfAction) -> Result<Action, String> {
    use kind::*;
    let (player, i, w, p0, p1) = (a.player, a.i, a.w[0], back(a.p[0]), back(a.p[1]));
    let flag = |k: usize| i[k] != 0;
    Ok(match a.kind {
        END_COMMAND => Action::Payload(Payload::EndCommand {
            player,
            command: w as u64,
        }),
        RUN_DEFERRED => Action::Payload(Payload::RunDeferred { player }),
        INCREASE_SCORE => Action::Payload(Payload::IncreaseScore {
            player,
            amount: i[0],
            delay: i[1],
            source: p0,
        }),
        INCREASE_CURRENCY => Action::Payload(Payload::IncreaseCurrency {
            player,
            source: p0,
            delay: i[1],
            amount: i[0],
        }),
        DECREASE_SCORE => Action::Combat(Combat::DecreaseScore {
            player,
            amount: i[0],
        }),
        ATTACK => Action::Combat(Combat::Attack {
            player,
            origin: p0,
            target: p1,
            damage: i[0],
            animation: i[1],
            should_move_to_target: flag(2),
            delay: i[3],
        }),
        KILL => Action::Combat(Combat::Kill {
            player,
            coordinates: p0,
        }),
        CAPTURE_CITY => Action::Combat(Combat::CaptureCity {
            player,
            coordinates: p0,
            old_owner: i[0],
        }),
        INFILTRATE => Action::Combat(Combat::Infiltrate {
            player,
            origin: p0,
            target: p1,
        }),
        INFILTRATION_REWARD => Action::Combat(Combat::InfiltrationReward {
            player,
            reward: w,
            coordinates: p0,
            kind: i[0],
        }),
        CONVERT => Action::Combat(Combat::Convert {
            player,
            origin: p0,
            target: p1,
        }),
        PROMOTE => Action::Combat(Combat::Promote {
            player,
            coordinates: p0,
        }),
        BUILD => Action::Economy(Economy::Build {
            player,
            kind: i[0],
            target: p0,
            pay: flag(1),
        }),
        BUILD_ROAD => Action::Economy(Economy::BuildRoad { player, target: p0 }),
        DESTROY_RESOURCE => Action::Economy(Economy::DestroyResource { player, target: p0 }),
        CREATE_RESOURCE => Action::Economy(Economy::CreateResource {
            player,
            kind: i[0],
            target: p0,
            reason: i[1],
        }),
        LEVEL_UP_IMPROVEMENT => Action::Economy(Economy::ImprovementLevelUp { player, target: p0 }),
        LEVEL_DOWN_IMPROVEMENT => Action::Economy(Economy::ImprovementLevelDown {
            player,
            target: p0,
            amount: i[0],
        }),
        CITY_REWARD => Action::Economy(Economy::CityReward {
            player,
            reward: i[0],
            target: p0,
        }),
        SCOUT => Action::Economy(Economy::Scout {
            player,
            unit: i[0] as u32,
            remaining: w,
            seed: i[1] as u32,
            from: p0,
            path: a.path_items()?.into_iter().collect(),
            behaviour: i[2],
        }),
        EXPAND_CITY => Action::Economy(Economy::ExpandCity { player, target: p0 }),
        MODIFY_PRODUCTION => Action::Economy(Economy::ModifyProduction {
            player,
            amount: i[0],
            target: p0,
        }),
        MODIFY_SCORE => Action::Economy(Economy::ModifyScore {
            player,
            amount: i[0],
            target: p0,
        }),
        HOUDINI => Action::Economy(Economy::Houdini { player, target: p0 }),
        TRAIN => Action::Economy(Economy::Train {
            player,
            kind: i[0],
            target: p0,
            cost: i[1],
            direction_target: p1,
            train_action_aborted: flag(2),
        }),
        INCREASE_POPULATION => Action::City(City::IncreasePopulation {
            player,
            source: p0,
            target: p1,
            delay: i[0],
        }),
        DECREASE_POPULATION => Action::City(City::DecreasePopulation {
            player,
            target: p0,
            delay: i[0],
        }),
        LEVEL_UP => Action::City(City::LevelUp { player, target: p0 }),
        MOVE => {
            let path = a.path_items()?;
            if path.len() > engine_fast_path_tiles() {
                return Err("a move path longer than the engine's".into());
            }
            Action::MoveEffect(MoveEffect::Move {
                player,
                unit: i[0] as u32,
                path: path.into_iter().collect(),
                should_animate: flag(1),
                reason: i[2],
            })
        }
        RESELECT => Action::MoveEffect(MoveEffect::Reselect {
            player,
            unit: i[0] as u32,
        }),
        REVEAL => Action::MoveEffect(MoveEffect::Reveal {
            player,
            coordinates: p0,
            origin: p1,
            show_popup: flag(0),
        }),
        HIDE => Action::MoveEffect(MoveEffect::Hide {
            player,
            coordinates: p0,
        }),
        EMBARK => Action::MoveEffect(MoveEffect::Embark {
            player,
            coordinates: p0,
        }),
        DISEMBARK => Action::MoveEffect(MoveEffect::Disembark {
            player,
            coordinates: p0,
        }),
        DESTROY_IMPROVEMENT => Action::MoveEffect(MoveEffect::DestroyImprovement {
            player,
            coordinates: p0,
        }),
        CONNECT => Action::CityConnection(CityConnection::Connect {
            player,
            coordinates: p0,
        }),
        CHANGE => Action::CityConnection(CityConnection::Change {
            player,
            coordinates: p0,
        }),
        DISCONNECT => Action::CityConnection(CityConnection::Disconnect {
            player,
            coordinates: p0,
        }),
        HEAL_OTHERS => Action::TurnEffect(TurnEffect::HealOthers {
            player,
            coordinates: p0,
        }),
        HEAL => Action::TurnEffect(TurnEffect::Heal {
            player,
            coordinates: p0,
            fixed_heal: i[0],
        }),
        RESIGN => {
            let n = a.count(RESIGN_TRIGGERS)?;
            // SAFETY: the union is plain integers, so any bytes are valid triggers.
            let list = unsafe { a.list.triggers };
            let pending_triggers = list[..n]
                .iter()
                .map(|t| Trigger {
                    player: t.player,
                    opponent: t.opponent,
                    kind: t.kind,
                    coordinates: back(t.coordinates),
                })
                .collect();
            Action::TurnEffect(TurnEffect::Resign {
                player,
                resigned_player: i[0],
                kicker_player: i[1],
                was_kicked: flag(2),
                pending_triggers,
            })
        }
        UPGRADE => Action::TurnEffect(TurnEffect::Upgrade {
            player,
            coordinates: p0,
            unit: i[0],
            cost: i[1],
        }),
        RECOVER => Action::TurnEffect(TurnEffect::Recover {
            player,
            coordinates: p0,
        }),
        END_TURN => Action::TurnEffect(TurnEffect::EndTurn { player }),
        START_TURN => Action::TurnEffect(TurnEffect::StartTurn { player }),
        PASS_PLAYER => Action::TurnEffect(TurnEffect::PassPlayer { player }),
        END_MATCH => Action::TurnEffect(TurnEffect::EndMatch { player }),
        RECEIVE_MESSAGE => Action::TurnEffect(TurnEffect::ReceiveDiplomacyMessage {
            player,
            sender: i[0],
            kind: w,
        }),
        BREAK_PEACE => Action::Diplomacy(Diplomacy::BreakPeace {
            player,
            opponent: i[0],
        }),
        PEACE_RESPONSE => Action::Diplomacy(Diplomacy::PeaceResponse {
            player,
            opponent: i[0],
            accepted: flag(1),
        }),
        ESTABLISH_EMBASSY => Action::Diplomacy(Diplomacy::EstablishEmbassy {
            player,
            opponent: i[0],
        }),
        PEACE_TREATY => Action::Diplomacy(Diplomacy::PeaceTreaty {
            player,
            opponent: i[0],
        }),
        EXPLORE => Action::Explore {
            player,
            coordinates: p0,
        },
        MEET => Action::Meet {
            player,
            other: i[0],
            coordinates: p0,
            technology: i[1],
        },
        UPDATE_ROUTES => Action::UpdateRoutes { player },
        UPDATE_CITY_CONNECTIONS => {
            let n = a.count(5)?;
            // SAFETY: the union is plain integers.
            let ids = unsafe { a.list.players };
            Action::UpdateCityConnections {
                actor: player,
                players: ids[..n].iter().copied().collect(),
            }
        }
        ENABLE_TASK => Action::EnableTask {
            player,
            task: i[0] as u32,
        },
        TASK_COMPLETED => Action::TaskCompleted {
            player,
            task: i[0] as u32,
        },
        DESTROY_EMBASSY => Action::DestroyEmbassy {
            player,
            opponent: i[0],
        },
        GAME_OVER => Action::GameOver {
            player,
            winner: i[0],
        },
        WIPE_PLAYER => Action::WipePlayer {
            player,
            target: i[0],
        },
        WIPE_PLAYER_END => Action::WipePlayerEnd {
            player,
            target: i[0],
        },
        START_MATCH => Action::StartMatch { player },
        RESEARCH => Action::Research {
            player,
            technology: w,
            cost: i[0],
        },
        RUINS => Action::Ruins {
            player,
            reward: w,
            coordinates: p0,
        },
        DISBAND_UNIT => Action::DisbandUnit {
            player,
            coordinates: p0,
            home_coordinates: p1,
            get_paid: flag(0),
        },
        FREEZE_UNIT => Action::Combat(Combat::FreezeUnit {
            player,
            origin: p0,
            target: p1,
            damage: i[0],
        }),
        FLOOD => Action::Combat(Combat::Flood {
            player,
            coordinates: p0,
        }),
        POISON => Action::Combat(Combat::Poison {
            player,
            origin: p0,
            target: p1,
        }),
        EXPLODE => Action::Combat(Combat::Explode {
            player,
            coordinates: p0,
            home_coordinates: p1,
        }),
        EAT => Action::Combat(Combat::Eat {
            player,
            unit: i[0] as u32,
            coordinates: p0,
        }),
        GROW_UP => Action::Combat(Combat::Upgrade {
            player,
            kind: i[0],
            coordinates: p0,
            cost: i[1],
        }),
        BUBBLE => Action::Combat(Combat::Bubble {
            player,
            unit: i[0] as u32,
            add: flag(1),
        }),
        FREEZE_TILE => Action::Combat(Combat::FreezeTile {
            player,
            coordinates: p0,
        }),
        FREEZE_AREA => Action::Combat(Combat::FreezeArea {
            player,
            coordinates: p0,
            radius: i[0],
            freeze_units: flag(1),
            only_owned: flag(2),
        }),
        FREEZE_BORDER => Action::Economy(Economy::FreezeArea {
            player,
            target: p0,
            radius: i[0],
            freeze_units: flag(1),
            only_owned_tiles: flag(2),
        }),
        MOVE_HEAL_OTHERS => Action::MoveEffect(MoveEffect::HealOthers {
            player,
            coordinates: p0,
        }),
        BREAK_ICE_AREA => Action::TurnEffect(TurnEffect::BreakIceArea {
            player,
            coordinates: p0,
            radius: i[0],
        }),
        CLEAR_TILE_EFFECT => Action::TurnEffect(TurnEffect::ClearTileEffect {
            player,
            coordinates: p0,
            effect: i[0],
            triggered_by_unit: flag(1),
        }),
        BOOST_OTHERS => Action::TurnEffect(TurnEffect::BoostOthers {
            player,
            coordinates: p0,
        }),
        BOOST => Action::TurnEffect(TurnEffect::Boost {
            player,
            coordinates: p0,
        }),
        DECOMPOSE => Action::TurnEffect(TurnEffect::Decompose {
            player,
            coordinates: p0,
        }),
        BREAK_ICE => Action::TurnEffect(TurnEffect::BreakIce {
            player,
            coordinates: p0,
        }),
        CHANGE_CLIMATE => Action::TurnEffect(TurnEffect::ChangeClimate {
            player,
            coordinates: p0,
            climate: i[0],
        }),
        k => return Err(format!("unknown device action kind {k}")),
    })
}

/// The longest move path the engine's `Path` holds (11 on the engine branch; `main` keeps a
/// list without a bound, where the device's 13 is the limit).
fn engine_fast_path_tiles() -> usize {
    PATH_TILES
}

/// The record's fields as numbers, the unused list entries left out.
pub fn dump(a: &EfAction, out: &mut Vec<i64>) {
    out.push(i64::from(a.kind));
    out.push(i64::from(a.player));
    out.extend(a.i.iter().map(|&v| i64::from(v)));
    out.extend(a.w);
    for c in a.p {
        out.push(i64::from(c.x));
        out.push(i64::from(c.y));
    }
    out.push(i64::from(a.len));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The union's largest member covers it, so building a record writes every byte.
    #[test]
    fn the_path_covers_the_union() {
        assert_eq!(
            std::mem::size_of::<EfList>(),
            std::mem::size_of::<[EfXy; PATH_TILES]>()
        );
    }
}
