/* errors.h - why a kernel function stopped. The Rust engine returns its reasons as text;
 * the kernel sources return a number from this list, and the host gives the engine's text
 * for it (ef_error_text), so a stopped game reads the same from either backend. An entry
 * whose text has "{}" carries one integer argument beside the code.
 *
 * The third column is 1 where the Rust engine panics rather than returns an error (a broken
 * invariant; the batch marks the game failed rather than refused). */
#ifndef EF_ERRORS_H
#define EF_ERRORS_H

#include <stdint.h>
#include "ef.h"

#define EF_ERRORS(X) \
    X(EF_OK, "", 0) \
    X(EF_ERR_INVARIANT, "the device state is inconsistent (check at line {} of query.h)", 1) \
    X(EF_ERR_TABLE, "", 0) /* the argument numbers a reason the host kept when it built the tables */ \
    X(EF_ERR_NOT_PORTED, "not ported to the device: {}", 0) \
    X(EF_ERR_ACTIONS_FULL, "the device action stack is full", 0) \
    X(EF_ERR_UNITS_FULL, "the device unit table is full", 1) \
    X(EF_ERR_SHORT_LIST_FULL, "short list of {} is full", 1) \
    X(EF_ERR_SHORT_LIST_REFUSED, "short list of {} is full", 0) /* the same text from try_push, an error */ \
    X(EF_ERR_VIEW_PLAYER, "view for a missing player", 0) \
    X(EF_ERR_VIEW_EFFECT, "effect {} does not fit the view's bit set", 0) \
    X(EF_ERR_VIEW_MODE, "rules mode is not a small integer", 0) \
    X(EF_ERR_VIEW_TASK_COUNTER, "task counter is not a small integer", 0) \
    X(EF_ERR_UNKNOWN_TRIBE, "unknown tribe", 0) \
    X(EF_ERR_MISSING_PLAYER, "missing player", 0) \
    X(EF_ERR_MISSING_PLAYER_ID, "missing player {}", 0) \
    X(EF_ERR_NULL_TECH_CACHE, "null unlocked tech cache", 0) \
    X(EF_ERR_RELATION_PLAYER_0, "relation toward player 0", 0) \
    X(EF_ERR_CAPITAL_TILE, "missing original capital tile", 0) \
    X(EF_ERR_TASK_NONE, "task type None", 0) \
    X(EF_ERR_TASK_TYPE, "task type {}", 0) \
    X(EF_ERR_PEACEFUL_TURNS, "peaceful turns", 0) \
    X(EF_ERR_KILLS, "kills", 0) \
    X(EF_ERR_CONVERSIONS, "conversions", 0) \
    X(EF_ERR_UNKNOWN_UNIT_KIND, "unknown unit kind", 0) \
    X(EF_ERR_PASSENGER_RANGE, "passenger node out of range", 0) \
    X(EF_ERR_UNIT_RANGE, "unit node out of range", 0) \
    X(EF_ERR_UNKNOWN_TECH, "unknown tech", 0) \
    X(EF_ERR_NATURE_PHASE, "unexpected Nature phase", 0) \
    X(EF_ERR_NATURE_SETTLE, "Nature continuation did not settle", 0) \
    X(EF_ERR_COMMANDS_NEVER_RAN, "the state has commands that never ran", 0) \
    X(EF_ERR_COMMAND_KIND, "command kind {} not implemented", 0) \
    X(EF_ERR_NO_SOURCE, "action has no source", 0) \
    X(EF_ERR_NO_TARGET, "action has no target", 0) \
    X(EF_ERR_NO_OPTION, "action has no option", 0) \
    X(EF_ERR_NO_OPPONENT, "action has no opponent", 0) \
    X(EF_ERR_BUILD_TILE, "build tile", 0) \
    X(EF_ERR_UNKNOWN_IMPROVEMENT, "unknown improvement", 0) \
    X(EF_ERR_MOVE_NO_UNIT, "move with no unit", 0) \
    X(EF_ERR_MOVE_CANNOT_MOVE, "a Move by a unit that cannot move: not a legal move", 0) \
    X(EF_ERR_MOVE_NO_DATA, "a Move by a unit without unit data: not a legal move", 0) \
    X(EF_ERR_MOVE_NO_PATH, "a Move with no path within the unit's movement: not a legal move", 0) \
    X(EF_ERR_ATTACK_NO_UNIT, "attack with no unit", 0) \
    X(EF_ERR_CAPTURE_NO_UNIT, "capture with no unit", 0) \
    X(EF_ERR_PROMOTE_NO_UNIT, "promote with no unit", 0) \
    X(EF_ERR_PEACE_TRIGGER, "peace response without its trigger", 0) \
    X(EF_ERR_HEAL_OTHERS_TILE, "heal-others tile", 0) \
    X(EF_ERR_MOVE_DESTINATION, "move destination", 0) \
    X(EF_ERR_EFFECT_LIST, "effect list", 0) \
    X(EF_ERR_UNIT_OFF_BOARD, "unit off the board", 0) \
    X(EF_ERR_CAPITAL_OFF_BOARD, "capital off the board", 0) \
    X(EF_ERR_RECOVER_NO_UNIT, "Recover with no unit", 0) \
    X(EF_ERR_DISBAND_TILE, "disband tile", 0) \
    X(EF_ERR_ACHIEVEMENTS, "achievement tracking not implemented", 0) \
    X(EF_ERR_END_COMMAND_IMPROVEMENT, "end-command improvement data", 0) \
    X(EF_ERR_WINNER_SCOPE, "winner check outside 2-4 players or in Tutorial", 0) \
    X(EF_ERR_ELIMINATED_RANKING, "ranking more than three eliminated players not implemented", 0) \
    X(EF_ERR_NO_PLAYER_ALIVE, "no player alive", 0) \
    X(EF_ERR_EMBASSY_INCOME, "embassy income", 0) \
    X(EF_ERR_WORK_TILE, "work tile", 0) \
    X(EF_ERR_PLAYER_WORK_TILE, "player work tile", 0) \
    X(EF_ERR_CAPITAL_TILE_PLAIN, "capital tile", 0) \
    X(EF_ERR_MESSAGE_SENDER, "message from a missing player", 0) \
    X(EF_ERR_DIPLOMACY_MESSAGE, "unsupported diplomacy message", 0) \
    X(EF_ERR_LEVEL_TILE, "level tile", 0) \
    X(EF_ERR_NO_ICE_BANK, "no ice bank in the catalog", 0) \
    X(EF_ERR_GROWTH_RATE_ZERO, "growth rate 0", 0) \
    X(EF_ERR_SURROUNDING_TILE, "surrounding tile", 0) \
    X(EF_ERR_TRIGGERS_FULL, "the device pending-choice list is full", 0) \
    X(EF_ERR_AREA_CENTRE, "area centre off the board", 0) \
    X(EF_ERR_AREA_RADIUS, "area radius above 5", 0) \
    X(EF_ERR_EXPLORE_ORIGIN, "exploration origin off the board", 0) \
    X(EF_ERR_MISSING_UNIT_OWNER, "missing unit owner", 0) \
    X(EF_ERR_OPEN_LIST_FULL, "the device path search list is full", 0) \
    X(EF_ERR_MOVE_REASON, "unknown move reason", 0) \
    X(EF_ERR_EMPTY_PATH, "empty move path", 0) \
    X(EF_ERR_MOVE_BUNNY, "moving a Bunny not implemented", 0) \
    X(EF_ERR_ALGAE_PATH_TILE, "algae path tile", 0) \
    X(EF_ERR_FREEZE_ACTOR, "freeze actor missing", 0) \
    X(EF_ERR_FREEZE_TILE, "freeze tile off the board", 0) \
    X(EF_ERR_FREEZE_NO_UNIT, "freeze area with no unit", 0) \
    X(EF_ERR_FREEZE_REFUSED, "freeze area refused", 0) \
    X(EF_ERR_FREEZE_TILE_TILE, "freeze tile", 0) \
    X(EF_ERR_FREEZE_UNIT_TARGET, "freeze unit target", 0) \
    X(EF_ERR_BREAK_ICE_TILE, "break ice tile", 0) \
    X(EF_ERR_CLIMATE_TILE, "climate tile", 0) \
    X(EF_ERR_CLIMATE_CHANGE_TILE, "climate change tile", 0) \
    X(EF_ERR_FLOOD_TILE, "flood tile", 0) \
    X(EF_ERR_CLEAR_TILE, "clear effect tile", 0) \
    X(EF_ERR_DECOMPOSE_TILE, "decompose tile", 0) \
    X(EF_ERR_DECOMPOSE_NONE, "decompose without an improvement", 0) \
    X(EF_ERR_SPORE_TILE, "spore tile", 0) \
    X(EF_ERR_EXPLODE_TILE, "explode tile", 0) \
    X(EF_ERR_EXPLODE_PASSENGER, "explode missing passenger", 0) \
    X(EF_ERR_UPKEEP_TILE, "upkeep tile", 0) \
    X(EF_ERR_CYCLIC_CHAIN, "cyclic follower chain", 0) \
    X(EF_ERR_SKATE_TILE, "skate attack tile", 0) \
    X(EF_ERR_LOST_CITY_CENTRE, "lost city centre", 0) \
    X(EF_ERR_CONTINENTS, "the device block holds no continents (the checkpoint's do not read as continents)", 1) \
    X(EF_ERR_CONTINENTS_FULL, "the device continent table is full", 1) \
    X(EF_ERR_RESOURCE_CONTINENT, "resource continent missing", 0) \
    X(EF_ERR_ATTRACT_RATE, "attract improvement with growth rate 0", 0) \
    X(EF_ERR_ATTRACT_AGE, "attract age division", 0) \
    X(EF_ERR_GROW_SUCCESSORS, "a Grow unit with two successors", 0) \
    X(EF_ERR_MOVE_ORIGIN, "move origin", 0) \
    X(EF_ERR_ROUTER_CHAIN, "broken router predecessor chain", 0) \
    X(EF_ERR_START_TILE, "start tile", 0) \
    X(EF_ERR_CONNECTION_TILE, "connection tile", 0) \
    X(EF_ERR_CONNECTION_IMPROVEMENT, "connection without an improvement", 0) \
    X(EF_ERR_ROUTE_OPEN_FULL, "the device route search list is full", 0) \
    X(EF_ERR_CONNECTION_QUEUE_FULL, "the device connection queue is full", 0) \
    X(EF_ERR_TRAIN_TILE, "train tile", 0) \
    X(EF_ERR_EMBARK_TILE, "embark tile", 0) \
    X(EF_ERR_EMBARK_NO_UNIT, "embark with no unit", 0) \
    X(EF_ERR_DISEMBARK_TILE, "disembark tile", 0) \
    X(EF_ERR_UPGRADE_NO_UNIT, "upgrade with no unit", 0) \
    X(EF_ERR_PROMOTION_NO_UNIT, "promotion with no unit", 0) \
    X(EF_ERR_PUSH_TILE, "push tile", 0) \
    X(EF_ERR_PUSH_NO_UNIT, "push with no unit", 0) \
    X(EF_ERR_UNIT_OVERRIDE_NULL, "unit override resolved null", 0) \
    X(EF_ERR_BRIDGE_IMPROVEMENT, "bridge without its improvement", 0) \
    X(EF_ERR_ROAD_TILE, "road tile", 0) \
    X(EF_ERR_RESOURCE_TILE, "resource tile", 0) \
    X(EF_ERR_DESTROY_TILE, "destroy tile", 0) \
    X(EF_ERR_DESTROY_NONE, "destroy without an improvement", 0) \
    X(EF_ERR_CITY_ACTION_TILE, "city action with no tile", 0) \
    X(EF_ERR_CITY_ACTION_OWNER, "city action on a city with no owner player", 0) \
    X(EF_ERR_REWARD_TRIGGER, "city reward without its trigger", 0) \
    X(EF_ERR_REWARD_RANGE, "reward out of range", 0) \
    X(EF_ERR_COUNT_OUTSIDE, "id {} is outside the 14 counted", 0) \
    X(EF_ERR_COUNT_FULL, "more than 255 of id {}", 0) \
    X(EF_ERR_CITY_REWARD_KIND, "city reward {} not implemented", 0) \
    X(EF_ERR_MODIFY_TILE, "modify tile", 0) \
    X(EF_ERR_MODIFY_NONE, "modify without an improvement", 0) \
    X(EF_ERR_EXPAND_TILE, "expand tile", 0) \
    X(EF_ERR_EXPAND_NONE, "expand without a city", 0) \
    X(EF_ERR_SCOUT_TILE, "scout tile", 0) \
    X(EF_ERR_TUTORIAL_SCOUT, "tutorial scout not implemented", 0) \
    X(EF_ERR_SCOUT_RANGE, "scout hash range", 0) \
    X(EF_ERR_ATTACK_AREA_TILE, "attack area tile", 0) \
    X(EF_ERR_BATTLE_DIVISION, "battle division overflow", 0) \
    X(EF_ERR_ATTACK_MISSING_UNIT, "attack with a missing unit", 0) \
    X(EF_ERR_INFILTRATE_TARGET, "infiltrate target", 0) \
    X(EF_ERR_ATTACK_CONSUMED, "attacking with consumed not implemented", 0) \
    X(EF_ERR_ATTACK_TARGET, "attack target", 0) \
    X(EF_ERR_ATTACK_TARGET_TILE, "attack target tile", 0) \
    X(EF_ERR_ATTACK_KILLER, "attack killer missing", 0) \
    X(EF_ERR_KILL_TILE, "kill tile", 0) \
    X(EF_ERR_KILL_NO_UNIT, "kill with no unit", 0) \
    X(EF_ERR_CONVERT_ORIGIN, "convert origin", 0) \
    X(EF_ERR_CONVERT_TARGET, "convert target unit", 0) \
    X(EF_ERR_CONVERT_SOURCE, "convert source unit", 0) \
    X(EF_ERR_CONVERT_KIND, "converted unit of unknown kind", 0) \
    X(EF_ERR_CATALOG_ENTRY, "catalog has no entry {}", 0) \
    X(EF_ERR_RUINS_TILE, "ruins tile", 0) \
    X(EF_ERR_CAPTURE_TILE, "capture tile", 0) \
    X(EF_ERR_CAPTURE_CITY_TILE, "capture city tile", 0) \
    X(EF_ERR_CAPTURE_CITY_IMPROVEMENT, "capture city improvement", 0) \
    X(EF_ERR_CITY_AREA_IMPROVEMENT, "city area improvement", 0) \
    X(EF_ERR_NAME_UNKNOWN_TRIBE, "naming a city for an unknown tribe not implemented", 0) \
    X(EF_ERR_NAME_SEEDS, "city name needs a default-Random seed", 0) \
    X(EF_ERR_EMPTY_NAME, "empty city name", 0) \
    X(EF_ERR_NAME_CASING, "city name casing needs the culture", 0) \
    X(EF_ERR_WORD_FULL, "the device city-name buffer is full", 1) \
    X(EF_ERR_ROWS_FULL, "the device legal list is full", 1) \
    X(EF_ERR_UNLOCKED_FULL, "the device list of unlocked improvements is full", 1) \
    X(EF_ERR_REBEL_SPOTS_FULL, "the device list of rebellion spots is full", 1) \
    X(EF_ERR_FIELD_RANGE, "a value outside the range of the device field it goes in (kernel line {})", 1) \
    X(EF_ERR_MISSING_MOVER, "missing mover", 0) \
    X(EF_ERR_MENU_PHASE, "phase other than Started/FinalTurn", 0) \
    X(EF_ERR_LEVEL_UP_NO_CITY, "city level-up trigger without a city", 0) \
    X(EF_ERR_REWARD_BELOW_2, "city reward below level 2", 0) \
    X(EF_ERR_UNKNOWN_TRIGGER, "unknown pending trigger", 0) \
    X(EF_ERR_ADJACENCY_NULL, "adjacency resource on a tile without one", 0) \
    X(EF_ERR_RESIGN_BEYOND_TWO, "resignation outcome beyond two players not implemented", 0) \
    X(EF_ERR_NO_LEGAL_MOVE, "no legal move", 0) \
    X(EF_ERR_STARTING_VISION, "a starting technology with capital vision not implemented", 0) \
    X(EF_ERR_LIGHTHOUSE_CORNER, "lighthouse corner", 0) \
    X(EF_ERR_INFILTRATE_ORIGIN, "infiltrate origin", 0) \
    X(EF_ERR_REBELLION_TILE, "rebellion tile", 0) \
    X(EF_ERR_PRODUCTION_TILE, "production tile", 0) \
    X(EF_ERR_PLACEMENT_KIND, "placement unit kind", 0) \
    X(EF_ERR_INFILTRATION_REWARD, "infiltration reward {} not implemented", 0) \
    X(EF_ERR_PLACEMENT_TILE, "placement tile", 0) \
    X(EF_ERR_RUIN_SHUFFLE, "ruin shuffle range", 0) \
    X(EF_ERR_RUIN_TECH, "empty ruin technology choice", 0) \
    X(EF_ERR_RUINS_REWARD, "ruins reward {} not implemented", 0)

#define EF_ERROR_CODE(code, text, failed) code,
enum { EF_ERRORS(EF_ERROR_CODE) EF_ERROR_COUNT };

#ifndef __CUDA_ARCH__
#define EF_ERROR_TEXT(code, text, failed) text,
static const char *const ef_error_texts[] = { EF_ERRORS(EF_ERROR_TEXT) };
static inline const char *ef_error_text(int code) {
    return code >= 0 && code < EF_ERROR_COUNT ? ef_error_texts[code] : "unknown error code";
}
/* whether `code` is a device capacity, which the CPU engine does not have: a game stopped there
 * is outside the comparison, not a difference */
static inline int ef_error_is_capacity(int code) {
    switch (code) {
    case EF_ERR_ACTIONS_FULL: case EF_ERR_UNITS_FULL: case EF_ERR_TRIGGERS_FULL: case EF_ERR_OPEN_LIST_FULL:
    case EF_ERR_ROUTE_OPEN_FULL: case EF_ERR_CONNECTION_QUEUE_FULL: case EF_ERR_WORD_FULL: case EF_ERR_ROWS_FULL:
    case EF_ERR_UNLOCKED_FULL: case EF_ERR_REBEL_SPOTS_FULL: case EF_ERR_FIELD_RANGE: case EF_ERR_CONTINENTS_FULL:
    case EF_ERR_CONTINENTS: return 1;
    default: return 0;
    }
}
#endif
/* whether `code` fails the game rather than refusing it (host and device code alike) */
#define EF_ERROR_FAILED_CASE(code, text, failed) case code: return failed;
EF_FN int ef_error_is_failure(int code) {
    switch (code) {
        EF_ERRORS(EF_ERROR_FAILED_CASE)
    default: return 0;
    }
}

/* A function's result: EF_OK or a code, with the code's argument (64 bits, as wide as any
 * number the engine puts in a reason). */
typedef struct { int32_t code, pad; int64_t arg; } EfResult;
#define EF_TRY(expr) do { EfResult ef_r_ = (expr); if (ef_r_.code != EF_OK) return ef_r_; } while (0)

#endif /* EF_ERRORS_H */
