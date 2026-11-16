//! Commands and queued actions as typed values.
//!
//! A command is what a player (or Nature) submits; executing it queues actions, and each
//! action may queue more (`step.rs`). Their serialized form is the game's: an action
//! that waits on the stack behind a pending choice is part of the compared state, and a
//! checkpoint's queued actions are read back into these types. Fields the rules never read
//! are kept because that form carries them.

use crate::inline::{Inline, Seats};
use crate::state::{Trigger, Xy};
use serde::{Deserialize, Serialize};

/// The most tiles a unit's move path holds, both ends included. A step costs at least half a
/// move and the search expands a tile only below its range, so a path has at most twice the
/// range in steps; the range is the unit's movement plus at most two from effects.
/// `Tables::compile` refuses a catalogue whose units move further than this allows.
pub const PATH_TILES: usize = 11;

/// A unit's move path, held in place.
pub type Path = Inline<Xy, PATH_TILES>;

/// The tiles an explorer has visited: where it started and one per step of its twelve.
pub type ScoutPath = Inline<Xy, 13>;

/// What a player submits. `player` is the mover in every command.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Research {
        technology: i64,
    },
    PeaceTreaty {
        opponent: i32,
    },
    Build {
        kind: i32,
        target: Xy,
    },
    Destroy {
        coordinates: Xy,
    },
    BreakPeace {
        opponent: i32,
    },
    PeaceRequestResponse {
        opponent: i32,
        accepted: bool,
    },
    Move {
        unit: u32,
        from: Xy,
        to: Xy,
    },
    Train {
        kind: i32,
        target: Xy,
    },
    Disembark {
        coordinates: Xy,
    },
    Upgrade {
        kind: i32,
        coordinates: Xy,
    },
    EstablishEmbassy {
        opponent: i32,
    },
    Attack {
        unit: u32,
        origin: Xy,
        target: Xy,
    },
    Capture {
        unit: u32,
        coordinates: Xy,
    },
    CityReward {
        reward: i32,
        target: Xy,
    },
    ExamineRuins {
        coordinates: Xy,
    },
    HealOthers {
        coordinates: Xy,
    },
    Promote {
        coordinates: Xy,
    },
    /// `kicker_player` and `was_kicked` only travel with the queued action
    Resign {
        resigned_player: i32,
        kicker_player: i32,
        was_kicked: bool,
    },
    EndTurn,
    EndMatch,
    Recover {
        coordinates: Xy,
    },
    Disband {
        coordinates: Xy,
    },
    StartMatch,
    FreezeArea {
        coordinates: Xy,
    },
    BreakIce {
        coordinates: Xy,
    },
    ClearTileEffect {
        coordinates: Xy,
        effect: i32,
    },
    Swarm {
        coordinates: Xy,
    },
    Boost {
        coordinates: Xy,
    },
    Explode {
        coordinates: Xy,
    },
    Decompose {
        coordinates: Xy,
    },
    Flood {
        coordinates: Xy,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Action {
    Payload(Payload),
    Combat(Combat),
    Economy(Economy),
    City(City),
    MoveEffect(MoveEffect),
    CityConnection(CityConnection),
    TurnEffect(TurnEffect),
    Diplomacy(Diplomacy),
    Explore {
        player: i32,
        coordinates: Xy,
    },
    Meet {
        player: i32,
        other: i32,
        coordinates: Xy,
        technology: i32,
    },
    UpdateRoutes {
        player: i32,
    },
    UpdateCityConnections {
        actor: i32,
        players: Seats<i32>,
    },
    EnableTask {
        player: i32,
        task: u32,
    },
    TaskCompleted {
        player: i32,
        task: u32,
    },
    DestroyEmbassy {
        player: i32,
        opponent: i32,
    },
    GameOver {
        player: i32,
        winner: i32,
    },
    WipePlayer {
        player: i32,
        target: i32,
    },
    WipePlayerEnd {
        player: i32,
        target: i32,
    },
    StartMatch {
        player: i32,
    },
    Research {
        player: i32,
        technology: i64,
        cost: i32,
    },
    Ruins {
        player: i32,
        reward: i64,
        coordinates: Xy,
    },
    DisbandUnit {
        player: i32,
        coordinates: Xy,
        home_coordinates: Xy,
        get_paid: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Payload {
    EndCommand {
        player: i32,
        command: u64,
    },
    RunDeferred {
        player: i32,
    },
    IncreaseScore {
        player: i32,
        amount: i32,
        delay: i32,
        source: Xy,
    },
    IncreaseCurrency {
        player: i32,
        source: Xy,
        delay: i32,
        amount: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Combat {
    DecreaseScore {
        player: i32,
        amount: i32,
    },
    Attack {
        player: i32,
        origin: Xy,
        target: Xy,
        damage: i32,
        animation: i32,
        should_move_to_target: bool,
        delay: i32,
    },
    Kill {
        player: i32,
        coordinates: Xy,
    },
    CaptureCity {
        player: i32,
        coordinates: Xy,
        old_owner: i32,
    },
    Infiltrate {
        player: i32,
        origin: Xy,
        target: Xy,
    },
    InfiltrationReward {
        player: i32,
        reward: i64,
        coordinates: Xy,
        kind: i32,
    },
    Convert {
        player: i32,
        origin: Xy,
        target: Xy,
    },
    Promote {
        player: i32,
        coordinates: Xy,
    },
    FreezeUnit {
        player: i32,
        origin: Xy,
        target: Xy,
        damage: i32,
    },
    Flood {
        player: i32,
        coordinates: Xy,
    },
    Poison {
        player: i32,
        origin: Xy,
        target: Xy,
    },
    Explode {
        player: i32,
        coordinates: Xy,
        #[serde(default)]
        home_coordinates: Xy,
    },
    Eat {
        player: i32,
        /// the eater's id
        unit: u32,
        #[serde(default)]
        coordinates: Xy,
    },
    /// a unit grows into `kind` (a dead centipede head's follower), as the Upgrade turn effect
    Upgrade {
        player: i32,
        kind: i32,
        coordinates: Xy,
        cost: i32,
    },
    Bubble {
        player: i32,
        /// the unit's id
        unit: u32,
        add: bool,
    },
    FreezeTile {
        player: i32,
        coordinates: Xy,
    },
    FreezeArea {
        player: i32,
        coordinates: Xy,
        radius: i32,
        freeze_units: bool,
        only_owned: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Economy {
    Build {
        player: i32,
        kind: i32,
        target: Xy,
        pay: bool,
    },
    BuildRoad {
        player: i32,
        target: Xy,
    },
    DestroyResource {
        player: i32,
        target: Xy,
    },
    CreateResource {
        player: i32,
        kind: i32,
        target: Xy,
        reason: i32,
    },
    ImprovementLevelUp {
        player: i32,
        target: Xy,
    },
    ImprovementLevelDown {
        player: i32,
        target: Xy,
        amount: i32,
    },
    CityReward {
        player: i32,
        reward: i32,
        target: Xy,
    },
    Scout {
        player: i32,
        unit: u32,
        remaining: i64,
        seed: u32,
        from: Xy,
        path: ScoutPath,
        behaviour: i32,
    },
    ExpandCity {
        player: i32,
        target: Xy,
    },
    /// a Polaris city's border growth freezes its owned tiles (no units)
    FreezeArea {
        player: i32,
        target: Xy,
        radius: i32,
        freeze_units: bool,
        only_owned_tiles: bool,
    },
    ModifyProduction {
        player: i32,
        amount: i32,
        target: Xy,
    },
    ModifyScore {
        player: i32,
        amount: i32,
        target: Xy,
    },
    Houdini {
        player: i32,
        target: Xy,
    },
    Train {
        player: i32,
        kind: i32,
        target: Xy,
        cost: i32,
        direction_target: Xy,
        train_action_aborted: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum City {
    IncreasePopulation {
        player: i32,
        source: Xy,
        target: Xy,
        delay: i32,
    },
    DecreasePopulation {
        player: i32,
        target: Xy,
        delay: i32,
    },
    LevelUp {
        player: i32,
        target: Xy,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum MoveEffect {
    Move {
        player: i32,
        unit: u32,
        /// destination first, origin last
        path: Path,
        should_animate: bool,
        reason: i32,
    },
    Reselect {
        player: i32,
        unit: u32,
    },
    Reveal {
        player: i32,
        coordinates: Xy,
        origin: Xy,
        show_popup: bool,
    },
    Hide {
        player: i32,
        coordinates: Xy,
    },
    Embark {
        player: i32,
        coordinates: Xy,
    },
    Disembark {
        player: i32,
        coordinates: Xy,
    },
    DestroyImprovement {
        player: i32,
        coordinates: Xy,
    },
    HealOthers {
        player: i32,
        coordinates: Xy,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CityConnection {
    Connect { player: i32, coordinates: Xy },
    Change { player: i32, coordinates: Xy },
    Disconnect { player: i32, coordinates: Xy },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum TurnEffect {
    HealOthers {
        player: i32,
        coordinates: Xy,
    },
    Heal {
        player: i32,
        coordinates: Xy,
        fixed_heal: i32,
    },
    Resign {
        player: i32,
        resigned_player: i32,
        kicker_player: i32,
        was_kicked: bool,
        pending_triggers: Vec<Trigger>,
    },
    Upgrade {
        player: i32,
        coordinates: Xy,
        /// the unit kind upgraded to
        unit: i32,
        cost: i32,
    },
    Recover {
        player: i32,
        coordinates: Xy,
    },
    BreakIceArea {
        player: i32,
        coordinates: Xy,
        radius: i32,
    },
    ClearTileEffect {
        player: i32,
        coordinates: Xy,
        effect: i32,
        triggered_by_unit: bool,
    },
    BoostOthers {
        player: i32,
        coordinates: Xy,
    },
    Boost {
        player: i32,
        coordinates: Xy,
    },
    Decompose {
        player: i32,
        coordinates: Xy,
    },
    BreakIce {
        player: i32,
        coordinates: Xy,
    },
    ChangeClimate {
        player: i32,
        coordinates: Xy,
        climate: i32,
    },
    EndTurn {
        player: i32,
    },
    StartTurn {
        player: i32,
    },
    PassPlayer {
        player: i32,
    },
    EndMatch {
        player: i32,
    },
    ReceiveDiplomacyMessage {
        player: i32,
        sender: i32,
        kind: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Diplomacy {
    BreakPeace {
        player: i32,
        opponent: i32,
    },
    PeaceResponse {
        player: i32,
        opponent: i32,
        accepted: bool,
    },
    EstablishEmbassy {
        player: i32,
        opponent: i32,
    },
    PeaceTreaty {
        player: i32,
        opponent: i32,
    },
}
