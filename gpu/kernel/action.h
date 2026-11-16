/* action.h - the queued actions and the commands of engine-fast/src/action.rs as flat records.
 *
 * Every kind of action is one EfAction with the fields below; which field holds what is
 * listed per kind (EF_ACTIONS), and ../src/action.rs converts with the Rust `Action` in the
 * same layout. Unit references are unit ids, as in the Rust engine, not slots. */
#ifndef EF_ACTION_H
#define EF_ACTION_H

#include "ef.h"

typedef struct { int32_t x, y; } EfXy;

/* mapgen/candidate.rs Continent, the parts a lost city reads or records: the weights as f32
 * bits in Continent's order (crop, fish, fruit, game, metal, whale, spores, aquacrop, water,
 * ocean, field, mountain, forest, ice), climate, skin, the alien-climate and detached flags, and
 * its first tile (has_start: whether it has one) */
#define EF_CONTINENT_WEIGHTS 14
typedef struct {
    uint32_t weights[EF_CONTINENT_WEIGHTS];
    int32_t climate, skin, alien, detached, has_start, pad;
    EfXy start;
} EfContinent;

typedef struct {
    int32_t player, opponent;
    int64_t kind;
    EfXy coordinates;
} EfTrigger;

#define EF_PATH_TILES 13   /* the longer of the two paths: an explorer's 13; a move holds 11 */
#define EF_RESIGN_TRIGGERS 4

typedef struct {
    int32_t kind;
    int32_t player;
    int32_t i[6];
    int64_t w[2];
    EfXy p[3];
    int32_t len;          /* entries of the list below that are in use */
    int32_t pad;
    union {
        EfXy path[EF_PATH_TILES];
        EfTrigger triggers[EF_RESIGN_TRIGGERS];
        int32_t players[5];
    } list;
} EfAction;

/* The kinds, with the Rust variant and where its fields go. player is the variant's `player`
 * (UpdateCityConnections: `actor`); fields not named are 0. */
#define EF_ACTIONS(X) \
    X(EF_A_END_COMMAND,        "Payload::EndCommand: w0 command") \
    X(EF_A_RUN_DEFERRED,       "Payload::RunDeferred") \
    X(EF_A_INCREASE_SCORE,     "Payload::IncreaseScore: i0 amount, i1 delay, p0 source") \
    X(EF_A_INCREASE_CURRENCY,  "Payload::IncreaseCurrency: i0 amount, i1 delay, p0 source") \
    X(EF_A_DECREASE_SCORE,     "Combat::DecreaseScore: i0 amount") \
    X(EF_A_ATTACK,             "Combat::Attack: p0 origin, p1 target, i0 damage, i1 animation, i2 should_move_to_target, i3 delay") \
    X(EF_A_KILL,               "Combat::Kill: p0 coordinates") \
    X(EF_A_CAPTURE_CITY,       "Combat::CaptureCity: p0 coordinates, i0 old_owner") \
    X(EF_A_INFILTRATE,         "Combat::Infiltrate: p0 origin, p1 target") \
    X(EF_A_INFILTRATION_REWARD,"Combat::InfiltrationReward: w0 reward, p0 coordinates, i0 kind") \
    X(EF_A_CONVERT,            "Combat::Convert: p0 origin, p1 target") \
    X(EF_A_PROMOTE,            "Combat::Promote: p0 coordinates") \
    X(EF_A_BUILD,              "Economy::Build: i0 kind, p0 target, i1 pay") \
    X(EF_A_BUILD_ROAD,         "Economy::BuildRoad: p0 target") \
    X(EF_A_DESTROY_RESOURCE,   "Economy::DestroyResource: p0 target") \
    X(EF_A_CREATE_RESOURCE,    "Economy::CreateResource: i0 kind, p0 target, i1 reason") \
    X(EF_A_LEVEL_UP_IMPROVEMENT,   "Economy::ImprovementLevelUp: p0 target") \
    X(EF_A_LEVEL_DOWN_IMPROVEMENT, "Economy::ImprovementLevelDown: p0 target, i0 amount") \
    X(EF_A_CITY_REWARD,        "Economy::CityReward: i0 reward, p0 target") \
    X(EF_A_SCOUT,              "Economy::Scout: i0 unit, w0 remaining, i1 seed, p0 from, path, i2 behaviour") \
    X(EF_A_EXPAND_CITY,        "Economy::ExpandCity: p0 target") \
    X(EF_A_MODIFY_PRODUCTION,  "Economy::ModifyProduction: i0 amount, p0 target") \
    X(EF_A_MODIFY_SCORE,       "Economy::ModifyScore: i0 amount, p0 target") \
    X(EF_A_HOUDINI,            "Economy::Houdini: p0 target") \
    X(EF_A_TRAIN,              "Economy::Train: i0 kind, p0 target, i1 cost, p1 direction_target, i2 train_action_aborted") \
    X(EF_A_INCREASE_POPULATION,"City::IncreasePopulation: p0 source, p1 target, i0 delay") \
    X(EF_A_DECREASE_POPULATION,"City::DecreasePopulation: p0 target, i0 delay") \
    X(EF_A_LEVEL_UP,           "City::LevelUp: p0 target") \
    X(EF_A_MOVE,               "MoveEffect::Move: i0 unit, path, i1 should_animate, i2 reason") \
    X(EF_A_RESELECT,           "MoveEffect::Reselect: i0 unit") \
    X(EF_A_REVEAL,             "MoveEffect::Reveal: p0 coordinates, p1 origin, i0 show_popup") \
    X(EF_A_HIDE,               "MoveEffect::Hide: p0 coordinates") \
    X(EF_A_EMBARK,             "MoveEffect::Embark: p0 coordinates") \
    X(EF_A_DISEMBARK,          "MoveEffect::Disembark: p0 coordinates") \
    X(EF_A_DESTROY_IMPROVEMENT,"MoveEffect::DestroyImprovement: p0 coordinates") \
    X(EF_A_CONNECT,            "CityConnection::Connect: p0 coordinates") \
    X(EF_A_CHANGE,             "CityConnection::Change: p0 coordinates") \
    X(EF_A_DISCONNECT,         "CityConnection::Disconnect: p0 coordinates") \
    X(EF_A_HEAL_OTHERS,        "TurnEffect::HealOthers: p0 coordinates") \
    X(EF_A_HEAL,               "TurnEffect::Heal: p0 coordinates, i0 fixed_heal") \
    X(EF_A_RESIGN,             "TurnEffect::Resign: i0 resigned_player, i1 kicker_player, i2 was_kicked, triggers") \
    X(EF_A_UPGRADE,            "TurnEffect::Upgrade: p0 coordinates, i0 unit, i1 cost") \
    X(EF_A_RECOVER,            "TurnEffect::Recover: p0 coordinates") \
    X(EF_A_END_TURN,           "TurnEffect::EndTurn") \
    X(EF_A_START_TURN,         "TurnEffect::StartTurn") \
    X(EF_A_PASS_PLAYER,        "TurnEffect::PassPlayer") \
    X(EF_A_END_MATCH,          "TurnEffect::EndMatch") \
    X(EF_A_RECEIVE_MESSAGE,    "TurnEffect::ReceiveDiplomacyMessage: i0 sender, w0 kind") \
    X(EF_A_BREAK_PEACE,        "Diplomacy::BreakPeace: i0 opponent") \
    X(EF_A_PEACE_RESPONSE,     "Diplomacy::PeaceResponse: i0 opponent, i1 accepted") \
    X(EF_A_ESTABLISH_EMBASSY,  "Diplomacy::EstablishEmbassy: i0 opponent") \
    X(EF_A_PEACE_TREATY,       "Diplomacy::PeaceTreaty: i0 opponent") \
    X(EF_A_EXPLORE,            "Explore: p0 coordinates") \
    X(EF_A_MEET,               "Meet: i0 other, p0 coordinates, i1 technology") \
    X(EF_A_UPDATE_ROUTES,      "UpdateRoutes") \
    X(EF_A_UPDATE_CITY_CONNECTIONS, "UpdateCityConnections: players") \
    X(EF_A_ENABLE_TASK,        "EnableTask: i0 task") \
    X(EF_A_TASK_COMPLETED,     "TaskCompleted: i0 task") \
    X(EF_A_DESTROY_EMBASSY,    "DestroyEmbassy: i0 opponent") \
    X(EF_A_GAME_OVER,          "GameOver: i0 winner") \
    X(EF_A_WIPE_PLAYER,        "WipePlayer: i0 target") \
    X(EF_A_WIPE_PLAYER_END,    "WipePlayerEnd: i0 target") \
    X(EF_A_START_MATCH,        "StartMatch") \
    X(EF_A_RESEARCH,           "Research: w0 technology, i0 cost") \
    X(EF_A_RUINS,              "Ruins: w0 reward, p0 coordinates") \
    X(EF_A_DISBAND_UNIT,       "DisbandUnit: p0 coordinates, p1 home_coordinates, i0 get_paid") \
    X(EF_A_FREEZE_UNIT,        "Combat::FreezeUnit: p0 origin, p1 target, i0 damage") \
    X(EF_A_FLOOD,              "Combat::Flood: p0 coordinates") \
    X(EF_A_POISON,             "Combat::Poison: p0 origin, p1 target") \
    X(EF_A_EXPLODE,            "Combat::Explode: p0 coordinates, p1 home_coordinates") \
    X(EF_A_EAT,                "Combat::Eat: i0 unit, p0 coordinates") \
    X(EF_A_GROW_UP,            "Combat::Upgrade: i0 kind, p0 coordinates, i1 cost") \
    X(EF_A_BUBBLE,             "Combat::Bubble: i0 unit, i1 add") \
    X(EF_A_FREEZE_TILE,        "Combat::FreezeTile: p0 coordinates") \
    X(EF_A_FREEZE_AREA,        "Combat::FreezeArea: p0 coordinates, i0 radius, i1 freeze_units, i2 only_owned") \
    X(EF_A_FREEZE_BORDER,      "Economy::FreezeArea: p0 target, i0 radius, i1 freeze_units, i2 only_owned_tiles") \
    X(EF_A_MOVE_HEAL_OTHERS,   "MoveEffect::HealOthers: p0 coordinates") \
    X(EF_A_BREAK_ICE_AREA,     "TurnEffect::BreakIceArea: p0 coordinates, i0 radius") \
    X(EF_A_CLEAR_TILE_EFFECT,  "TurnEffect::ClearTileEffect: p0 coordinates, i0 effect, i1 triggered_by_unit") \
    X(EF_A_BOOST_OTHERS,       "TurnEffect::BoostOthers: p0 coordinates") \
    X(EF_A_BOOST,              "TurnEffect::Boost: p0 coordinates") \
    X(EF_A_DECOMPOSE,          "TurnEffect::Decompose: p0 coordinates") \
    X(EF_A_BREAK_ICE,          "TurnEffect::BreakIce: p0 coordinates") \
    X(EF_A_CHANGE_CLIMATE,     "TurnEffect::ChangeClimate: p0 coordinates, i0 climate")

#define EF_ACTION_CODE(code, text) code,
enum { EF_ACTIONS(EF_ACTION_CODE) EF_ACTION_KINDS };

/* A new action of `kind` for `player`, every other field zero. */
EF_FN EfAction ef_action(int32_t kind, int32_t player) {
    EfAction a;
    memset(&a, 0, sizeof a);
    a.kind = kind;
    a.player = player;
    return a;
}
EF_FN EfAction ef_action_at(int32_t kind, int32_t player, EfXy c) {
    EfAction a = ef_action(kind, player);
    a.p[0] = c;
    return a;
}

/* ---- commands (action.rs Command) ---- */
enum {
    EF_C_RESEARCH, EF_C_PEACE_TREATY, EF_C_BUILD, EF_C_DESTROY, EF_C_BREAK_PEACE, EF_C_PEACE_RESPONSE, EF_C_MOVE,
    EF_C_TRAIN, EF_C_DISEMBARK, EF_C_UPGRADE, EF_C_ESTABLISH_EMBASSY, EF_C_ATTACK, EF_C_CAPTURE, EF_C_CITY_REWARD,
    EF_C_EXAMINE_RUINS, EF_C_HEAL_OTHERS, EF_C_PROMOTE, EF_C_RESIGN, EF_C_END_TURN, EF_C_END_MATCH, EF_C_RECOVER,
    EF_C_DISBAND, EF_C_START_MATCH, EF_C_FREEZE_AREA, EF_C_BREAK_ICE, EF_C_CLEAR_TILE_EFFECT, EF_C_SWARM, EF_C_BOOST,
    EF_C_EXPLODE, EF_C_DECOMPOSE, EF_C_FLOOD, EF_COMMAND_KINDS
};

/* a command: technology in w; opponent, kind, reward, unit id, resigned player or accepted in
 * a and b (Resign: a resigned player, b kicker, c whether it was a kick); coordinates (from,
 * origin, target) in p and q */
typedef struct {
    int32_t kind, a, b, c;
    int64_t w;
    EfXy p, q;
} EfCommand;

/* The chosen legal move as the batch interface gives it to step::admit (the frame action's
 * fields); has_* says whether the field is present, accept is -1 when absent. */
typedef struct {
    int32_t kind;
    int32_t has_source, has_target, has_option, has_opponent, accept;
    EfXy source, target;
    int32_t option, opponent;
} EfMove;

#endif /* EF_ACTION_H */
