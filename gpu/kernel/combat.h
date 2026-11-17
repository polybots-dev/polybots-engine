/* combat.h - port of engine-fast/src/combat.rs and attack.rs: attack targets, the defence
 * bonus, retaliation, the battle arithmetic (integers only), and the Attack command and the
 * Attack, Kill and Convert actions. Infiltration is in infiltrate.h. */
#ifndef EF_COMBAT_H
#define EF_COMBAT_H

#include "units.h"

#define EF_ATTACK_RANGE 3
#define EF_KILLER_TASK EF_TASK_KILLER
#define EF_CITY_WALL 1
#define EF_TARGETS_MAX ((2 * EF_ATTACK_RANGE + 1) * (2 * EF_ATTACK_RANGE + 1) - 1)

typedef struct { int32_t len; EfXy at[EF_TARGETS_MAX]; } EfTargets;

EF_FN int ef_peaceful(int32_t r) { return r == EF_PEACE || r == EF_COOL_DOWN; }

EF_FN EfResult ef_targets_push(EfTargets *t, EfXy at) {
    if (t->len >= EF_TARGETS_MAX) return ef_short_list_full(EF_TARGETS_MAX);
    t->at[t->len++] = at;
    return ef_ok();
}

/* menu.rs visible_unit: the unit on an explored tile unless invisible, foreign and not at peace */
EF_FN const EfUnit *ef_visible_unit(const EfGame *g, const EfTile *tile, int32_t viewer) {
    if (!ef_tile_explored(tile, viewer) || tile->unit_n == EF_NONE) return 0;
    const EfUnit *u = &ef_cunits(g)[tile->unit_n];
    int invisible = ef_unit_effect(g, u, EF_INVISIBLE_EFFECT);
    return (u->owner_n == viewer || !invisible || ef_relation(g, viewer, u->owner_n) == EF_PEACE) ? u : 0;
}

/* combat.rs being_captured: a foreign unit stands in an owned city */
EF_FN int ef_being_captured(const EfGame *g, const EfTile *tile) {
    if (tile->unit_n == EF_NONE) return 0;
    const EfUnit *u = &ef_cunits(g)[tile->unit_n];
    return ef_xy_eq(u->coordinates, tile->coordinates) && tile->owner_n != 0 && u->owner_n != tile->owner_n;
}

/* combat.rs attack_options (the read-only form the retaliation check uses) */
EF_BIG EfResult ef_attack_options(const EfGame *g, const EfTables *t, const EfUnit *unit, int32_t range, int ignore, EfTargets *out) {
    out->len = 0;
    if (unit->leader != 0) return ef_ok();
    int capturer, convert, drench;
    EF_TRY(ef_kind_has(t, unit->kind_n, EF_AB_INFILTRATE, &capturer));
    const EfUnitData *d = ef_unit_data(t, unit->kind_n);
    EF_TRY(ef_kind_has(t, unit->kind_n, EF_AB_CONVERT, &convert));
    if (ef_wmul(d->attack, 10) == 0 && !convert && !capturer) return ef_ok();
    EF_TRY(ef_kind_has(t, unit->kind_n, EF_AB_DRENCH, &drench));
    int32_t viewer = unit->owner_n;
    EfXy c = unit->coordinates;
    if (range < 0) return ef_ok();
    int32_t height = g->tile_count / g->width;
    int32_t x0 = c.x - range < 0 ? 0 : c.x - range, x1 = c.x + range > g->width - 1 ? g->width - 1 : c.x + range;
    int32_t y0 = c.y - range < 0 ? 0 : c.y - range, y1 = c.y + range > height - 1 ? height - 1 : c.y + range;
    for (int32_t x = x0; x <= x1; x++) {
        for (int32_t y = y0; y <= y1; y++) {
            EfXy at = ef_xy(x, y);
            if (ef_xy_eq(at, c)) continue;
            const EfTile *tile = ef_tile_c(g, at);
            if (!tile) return ef_err(EF_ERR_ATTACK_AREA_TILE, 0);
            if (!ef_tile_explored(tile, viewer)) continue;
            if (capturer) {
                if (tile->has_improvement_n && tile->improvement.kind_n == EF_CITY && tile->owner_n != 0 && tile->owner_n != unit->owner_n) {
                    int32_t r = ef_relation(g, viewer, tile->owner_n);
                    if (r == EF_PEACE || ef_being_captured(g, tile) || ef_contains_i8(tile->improvement.effects_n, tile->improvement.effects_len_n, EF_ROBBED)) continue;
                    if (!ignore && ef_peaceful(r)) continue;
                    EF_TRY(ef_targets_push(out, at));
                }
                continue;
            }
            if (drench && ef_floodable(tile)) {
                EF_TRY(ef_targets_push(out, at));
                continue;
            }
            const EfUnit *target = ef_visible_unit(g, tile, viewer);
            if (target && target->owner_n != viewer && (ignore || !ef_peaceful(ef_relation(g, viewer, target->owner_n))))
                EF_TRY(ef_targets_push(out, at));
        }
    }
    return ef_ok();
}

/* combat.rs defence_bonus (x10) */
EF_FN EfResult ef_defence_bonus(const EfGame *g, const EfTables *t, const EfUnit *u, int32_t *out) {
    int32_t bonus = 10;
    const EfTile *tile = ef_tile_c(g, u->coordinates);
    if (tile) {
        int pi = u->owner_n != 0 ? ef_player_index(g, u->owner_n) : EF_NONE;
        if (pi != EF_NONE) {
            int32_t terrain = tile->terrain_n;
            if (terrain < 0 || terrain >= t->terrain_count || !t->terrain_known[terrain]) return ef_err(EF_ERR_CATALOG_ENTRY, terrain);
            const EfPlayer *p = &g->players[pi];
            for (int k = 0; k < p->available_tech_len; k++) {
                const EfTechData *d = ef_tech_data(t, p->available_tech[k]);
                if (!d) continue;
                if (d->defence_terrains & ef_terrain_bit(terrain)) {
                    if (d->defence_value[terrain] > 1) bonus = 15;
                    break;
                }
            }
        }
        if (tile->has_improvement_n && tile->improvement.kind_n == EF_CITY && tile->owner_n == u->owner_n) {
            int fortify;
            EF_TRY(ef_kind_has(t, u->kind_n, EF_AB_FORTIFY, &fortify));
            if (fortify) bonus = tile->improvement.rewards_n[EF_CITY_WALL] > 0 ? 40 : 15;
        }
    }
    if (ef_unit_effect(g, u, EF_POISONED)) bonus >>= 1;
    *out = bonus;
    return ef_ok();
}

/* combat.rs can_retaliate */
EF_FN EfResult ef_can_retaliate(const EfGame *g, const EfTables *t, const EfUnit *attacker, const EfUnit *defender, int *out) {
    int a, b, c, e;
    EF_TRY(ef_kind_has(t, defender->kind_n, EF_AB_STIFF, &a));
    *out = 0;
    if (a) return ef_ok();
    EF_TRY(ef_kind_has(t, attacker->kind_n, EF_AB_CONVERT, &b));
    if (b) return ef_ok();
    EF_TRY(ef_kind_has(t, attacker->kind_n, EF_AB_SURPRISE, &c));
    if (c) return ef_ok();
    if (ef_unit_effect(g, defender, EF_FROZEN)) return ef_ok();
    EF_TRY(ef_kind_has(t, attacker->kind_n, EF_AB_FREEZE, &e));
    if (e) return ef_ok();
    const EfUnitData *d = ef_unit_data(t, defender->kind_n);
    if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    EfTargets targets;
    EF_TRY(ef_attack_options(g, t, defender, d->range, 1, &targets));
    for (int k = 0; k < targets.len; k++)
        if (ef_xy_eq(targets.at[k], attacker->coordinates)) { *out = 1; break; }
    return ef_ok();
}

/* combat.rs round_damage (BattleHelpers.Round) */
EF_FN int32_t ef_round_damage(int32_t v) {
    int32_t base = ef_wmul(v / 10, 10);
    return v % 10 >= 10 - v % 10 ? ef_wadd(base, 10) : base;
}

EF_FN EfResult ef_battle_div(int64_t n, int64_t d, int64_t *out) {
    if (d == 0 || (n == INT64_MIN && d == -1)) return ef_err(EF_ERR_BATTLE_DIVISION, 0);
    *out = n / d;
    return ef_ok();
}
EF_FN int64_t ef_wmul64(int64_t a, int64_t b) { return (int64_t)((uint64_t)a * (uint64_t)b); }
EF_FN int64_t ef_wadd64(int64_t a, int64_t b) { return (int64_t)((uint64_t)a + (uint64_t)b); }

/* combat.rs battle: attack damage, retaliation or -1,
 * whether the defence bonus was reached, whether the retaliation query was */
EF_BIG EfResult ef_battle(const EfGame *g, const EfTables *t, const EfUnit *attacker, const EfUnit *defender,
                         int32_t *dealt_out, int32_t *back_out, int *bonus_reached, int *retaliation_reached) {
    *dealt_out = 0; *back_out = -1; *bonus_reached = 0; *retaliation_reached = 0;
    int32_t ah, dh;
    EF_TRY(ef_max_health(g, t, attacker, &ah));
    EF_TRY(ef_max_health(g, t, defender, &dh));
    if (ah < 1 || dh < 1) return ef_ok();
    const EfUnitData *a = ef_unit_data(t, attacker->kind_n);
    if (!a) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    int tentacles;
    EF_TRY(ef_kind_has(t, attacker->kind_n, EF_AB_TENTACLES, &tentacles));
    int32_t atk = tentacles ? ef_wmul(a->defence, 10) : ef_wadd(ef_wmul(a->attack, 10), ef_unit_effect(g, attacker, EF_BOOSTED) ? 50 : 0);
    int32_t bonus;
    EF_TRY(ef_defence_bonus(g, t, defender, &bonus));
    const EfUnitData *dd = ef_unit_data(t, defender->kind_n);
    if (!dd) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    int32_t def = ef_wmul(dd->defence, bonus);
    int64_t af = (int64_t)ef_wmul(ef_wmul(attacker->health_n, atk), 100) / (int64_t)ah;
    int64_t df = (int64_t)ef_wmul(ef_wmul(def, defender->health_n), 100) / (int64_t)dh;
    int64_t total = ef_wadd64(af, df);
    if (total <= 0) { *bonus_reached = 1; return ef_ok(); }
    int retaliates;
    EF_TRY(ef_can_retaliate(g, t, attacker, defender, &retaliates));
    int64_t denominator = ef_wmul64(total, 1000);
    int64_t q;
    EF_TRY(ef_battle_div(ef_wmul64(ef_wmul64((int64_t)atk, af), 450), denominator, &q));
    int32_t dealt = ef_round_damage((int32_t)(uint32_t)(uint64_t)q);
    int32_t back = -1;
    if (retaliates) {
        EF_TRY(ef_battle_div(ef_wmul64(ef_wmul64(df, (int64_t)def), 4500), ef_wmul64(denominator, (int64_t)bonus), &q));
        back = ef_round_damage((int32_t)(uint32_t)(uint64_t)q);
    }
    *dealt_out = dealt;
    *back_out = dealt >= defender->health_n ? 0 : back;
    *bonus_reached = 1;
    *retaliation_reached = 1;
    return ef_ok();
}

/* attack.rs options_at, with its state writes (relation lookups) */
EF_BIG EfResult ef_options_at(EfGame *g, const EfTables *t, int32_t viewer, EfXy center, int32_t range, int include_hidden,
                             const EfUnit *unit, int ignore, EfTargets *out) {
    int capturer, drench;
    EF_TRY(ef_kind_has(t, unit->kind_n, EF_AB_INFILTRATE, &capturer));
    EF_TRY(ef_kind_has(t, unit->kind_n, EF_AB_DRENCH, &drench));
    out->len = 0;
    if (range < 0) return ef_ok();
    int32_t h = g->tile_count / g->width;
    int32_t x0 = center.x - range < 0 ? 0 : center.x - range, x1 = center.x + range > g->width - 1 ? g->width - 1 : center.x + range;
    int32_t y0 = center.y - range < 0 ? 0 : center.y - range, y1 = center.y + range > h - 1 ? h - 1 : center.y + range;
    for (int32_t x = x0; x <= x1; x++) {
        for (int32_t y = y0; y <= y1; y++) {
            EfXy at = ef_xy(x, y);
            int32_t i = ef_tile_index(g, at);
            if (ef_xy_eq(at, center) || (!include_hidden && !ef_tile_explored(&ef_ctiles(g)[i], viewer))) continue;
            if (capturer) {
                EfTile x0t = ef_ctiles(g)[i];
                if (x0t.has_improvement_n && x0t.improvement.kind_n == EF_CITY && x0t.owner_n != 0 && x0t.owner_n != unit->owner_n) {
                    int32_t r;
                    EF_TRY(ef_relation_state_of(g, viewer, x0t.owner_n, &r));
                    int captured = x0t.unit_n != EF_NONE && ef_cunits(g)[x0t.unit_n].owner_n != x0t.owner_n;
                    int robbed = ef_contains_i8(x0t.improvement.effects_n, x0t.improvement.effects_len_n, 1);
                    if (r == EF_PEACE || captured || robbed || (!ignore && r == EF_COOL_DOWN)) continue;
                    EF_TRY(ef_targets_push(out, at));
                }
                continue;
            }
            if (drench && ef_floodable(&ef_ctiles(g)[i])) {
                EF_TRY(ef_targets_push(out, at));
                continue;
            }
            int32_t n = ef_ctiles(g)[i].unit_n;
            if (n == EF_NONE) continue;
            int32_t owner = ef_cunits(g)[n].owner_n;
            int invisible = ef_unit_effect(g, &ef_cunits(g)[n], EF_INVISIBLE_EFFECT);
            if (owner != viewer && invisible) {
                int32_t r;
                EF_TRY(ef_relation_state_of(g, viewer, owner, &r));
                if (r != EF_PEACE) continue;
            }
            if (owner != viewer) {
                int push = ignore;
                if (!ignore) {
                    int32_t r;
                    EF_TRY(ef_relation_state_of(g, viewer, owner, &r));
                    push = r != EF_PEACE && r != EF_COOL_DOWN;
                }
                if (push) EF_TRY(ef_targets_push(out, at));
            }
        }
    }
    return ef_ok();
}

/* attack.rs option_writes */
EF_FN EfResult ef_option_writes(EfGame *g, const EfTables *t, const EfUnit *unit, int32_t range, int ignore) {
    if (unit->leader != 0) return ef_ok();
    const EfUnitData *d = ef_unit_data(t, unit->kind_n);
    if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    if (ef_wmul(d->attack, 10) == 0) {
        int convert, infiltrate;
        EF_TRY(ef_kind_has(t, unit->kind_n, EF_AB_CONVERT, &convert));
        if (!convert) {
            EF_TRY(ef_kind_has(t, unit->kind_n, EF_AB_INFILTRATE, &infiltrate));
            if (!infiltrate) return ef_ok();
        }
    }
    EfTargets targets;
    return ef_options_at(g, t, unit->owner_n, unit->coordinates, range, 0, unit, ignore, &targets);
}

/* attack.rs admit_writes */
EF_FN EfResult ef_attack_admit_writes(EfGame *g, const EfTables *t, const EfUnit *unit) {
    const EfUnitData *d = ef_unit_data(t, unit->kind_n);
    if (!d) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    return ef_option_writes(g, t, unit, d->range, 0);
}

/* movement.rs can_advance */
EF_BIG EfResult ef_can_advance(const EfGame *g, const EfTables *t, const EfUnit *unit, EfXy to, int *out) {
    EfMover mover;
    EF_TRY(ef_mover_context(g, t, unit->owner_n, &mover));
    EfProfile s;
    int32_t range;
    EF_TRY(ef_profile_with(g, t, unit, &mover, &s, &range));
    *out = 0;
    if (range == 0) return ef_ok();
    s.allow_occupied = 1;
    EfPath path;
    return ef_scratch_path(t, &s, to, range, &path, out);
}

/* attack.rs battle_full, with its state writes */
EF_BIG EfResult ef_battle_full(EfGame *g, const EfTables *t, const EfUnit *a, const EfUnit *d, int32_t *dealt, int32_t *back,
                              int *advance, int *reveal) {
    *dealt = 0; *back = -1; *advance = 0; *reveal = 0;
    if (d->owner_n == a->owner_n) return ef_ok();
    int32_t r;
    EF_TRY(ef_relation_state_of(g, a->owner_n, d->owner_n, &r));
    if (r == EF_PEACE || r == EF_COOL_DOWN) return ef_ok();
    int bonus_reached, retaliation_reached;
    EF_TRY(ef_battle(g, t, a, d, dealt, back, &bonus_reached, &retaliation_reached));
    if (bonus_reached && ef_tile_c(g, d->coordinates) && d->owner_n != 0 && ef_player_index(g, d->owner_n) != EF_NONE)
        EF_TRY(ef_refresh_tech_cache(g, t, d->owner_n));
    if (retaliation_reached) {
        int stiff, convert, surprise, freeze;
        EF_TRY(ef_kind_has(t, d->kind_n, EF_AB_STIFF, &stiff));
        int skip = stiff;
        if (!skip) { EF_TRY(ef_kind_has(t, a->kind_n, EF_AB_CONVERT, &convert)); skip = convert; }
        if (!skip) { EF_TRY(ef_kind_has(t, a->kind_n, EF_AB_SURPRISE, &surprise)); skip = surprise; }
        if (!skip) skip = ef_unit_effect(g, d, EF_FROZEN);
        if (!skip) { EF_TRY(ef_kind_has(t, a->kind_n, EF_AB_FREEZE, &freeze)); skip = freeze; }
        if (!skip) {
            const EfUnitData *dd = ef_unit_data(t, d->kind_n);
            if (!dd) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
            EF_TRY(ef_option_writes(g, t, d, dd->range, 1));
        }
    }
    const EfUnitData *ad = ef_unit_data(t, a->kind_n);
    if (*dealt >= d->health_n) {
        if (!ad) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
        if (ad->range == 1) {
            EF_TRY(ef_refresh_tech_cache(g, t, a->owner_n));
            EF_TRY(ef_can_advance(g, t, a, d->coordinates, advance));
        }
    }
    *reveal = *dealt > 0 && !ef_unit_effect(g, d, EF_INVISIBLE_EFFECT);
    return ef_ok();
}

EF_FN int32_t ef_half_damage(int32_t damage) { return ef_wmul(damage / 20, 10); }

EF_FN void ef_reset_peace(EfGame *g, int i) {
    int n = ef_task_slot(&g->players[i], EF_TASK_PACIFIST);
    if (n >= 0 && !g->players[i].tasks[n].completed) {
        g->players[i].tasks[n].has_peaceful_turns = 1;
        g->players[i].tasks[n].peaceful_turns = 0;
    }
}

EF_FN EfAction ef_attack_action(int32_t p, EfXy origin, EfXy target, int32_t damage, int32_t animation, int moving, int32_t delay) {
    EfAction a = ef_action_at(EF_A_ATTACK, p, origin);
    a.p[1] = target;
    a.i[0] = damage;
    a.i[1] = animation;
    a.i[2] = moving;
    a.i[3] = delay;
    return a;
}

/* attack.rs attack_command */
EF_BIG EfResult ef_attack_command(EfGame *g, const EfTables *t, int32_t p, uint32_t id, EfXy origin, EfXy target) {
    int32_t node = ef_find_unit(g, id);
    if (node == EF_NONE) return ef_err(EF_ERR_ATTACK_MISSING_UNIT, 0);
    EfUnit u = ef_cunits(g)[node];
    const EfUnitData *ud = ef_unit_data(t, u.kind_n);
    if (!ud) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
    int32_t range = ud->range;
    EF_TRY(ef_base(g, p, 2));
#define EF_PASSENGER_HAS(ab) (u.passenger_n != EF_NONE && ef_unit_has(t, ef_cunits(g)[u.passenger_n].kind_n, (ab)))
    int infiltrator;
    EF_TRY(ef_kind_has(t, u.kind_n, EF_AB_INFILTRATE, &infiltrator));
    infiltrator = infiltrator || EF_PASSENGER_HAS(EF_AB_INFILTRATE);
    int consumed;
    EF_TRY(ef_kind_has(t, u.kind_n, EF_AB_CONSUMED, &consumed));
    if (infiltrator && !consumed) {
        /* infiltration: the cloak goes into the city instead of fighting */
        EfAction a = ef_action(EF_A_INFILTRATE, p);
        a.p[0] = u.coordinates;
        a.p[1] = target;
        EF_TRY(ef_push(g, a));
        ef_units(g)[node].moved_n = 1; /* narrow: a flag or constant */
        ef_units(g)[node].attacked_n = 1; /* narrow: a flag or constant */
        const EfTile *tt = ef_tile_c(g, target);
        if (!tt) return ef_err(EF_ERR_INFILTRATE_TARGET, 0);
        return ef_attack_diplomacy(g, p, tt->owner_n);
    }
    if (consumed || EF_PASSENGER_HAS(EF_AB_CONSUMED)) return ef_err(EF_ERR_ATTACK_CONSUMED, 0);
#undef EF_PASSENGER_HAS
    const EfTile *tt = ef_tile_c(g, target);
    if (!tt) return ef_err(EF_ERR_ATTACK_TARGET, 0);
    int has_defender = tt->unit_n != EF_NONE;
    EfUnit d;
    if (has_defender) d = ef_cunits(g)[tt->unit_n];
    int32_t dealt = 0, back = 0;
    int advance = 0, reveal = 0, killed = 1;
    if (has_defender) {
        int peace;
        EF_TRY(ef_at_peace(g, p, d.owner_n, &peace));
        if (!peace) {
            int di;
            EF_TRY(ef_player(g, d.owner_n, &di));
            EF_TRY(ef_attack_diplomacy(g, p, d.owner_n));
            EF_TRY(ef_battle_full(g, t, &u, &d, &dealt, &back, &advance, &reveal));
            killed = dealt >= d.health_n;
            int convert;
            EF_TRY(ef_kind_has(t, u.kind_n, EF_AB_CONVERT, &convert));
            if (convert && !killed && d.owner_n != EF_NATURE) {
                EfAction c = ef_action_at(EF_A_CONVERT, p, origin);
                c.p[1] = target;
                EF_TRY(ef_push(g, c));
            }
            if (killed && ef_unit_has(t, u.kind_n, EF_AB_EAT) && d.owner_n != EF_NATURE) EF_TRY(ef_eat_after_kill(g, p, node));
        }
    }
    if (back < u.health_n) {
        EfAction r = ef_action(EF_A_RESELECT, p);
        r.i[0] = (int32_t)id;
        EF_TRY(ef_push(g, r));
    }
    /* an Escape unit can still move after attacking; an Escape skater only while it stands on ice */
    int escape;
    EF_TRY(ef_kind_has(t, u.kind_n, EF_AB_ESCAPE, &escape));
    int moved = !escape;
    if (escape && ef_unit_has(t, u.kind_n, EF_AB_SKATE)) {
        const EfTile *at = ef_tile_c(g, advance ? target : u.coordinates);
        if (!at) return ef_err(EF_ERR_SKATE_TILE, 0);
        moved = at->terrain_n != EF_ICE;
    }
    ef_units(g)[node].moved_n = (int8_t)moved; /* narrow: a flag */
    if (ef_unit_effect(g, &u, EF_DOUBLE_READY)) {
        EfEffectList *e = &ef_lists(g)[ef_units(g)[node].effects_n];
        int at = ef_effect_index(e, EF_DOUBLE_READY);
        if (at >= 0) ef_effect_remove_at(e, at);
    } else {
        ef_units(g)[node].attacked_n = 1; /* narrow: a flag or constant */
    }
    int persist;
    EF_TRY(ef_kind_has(t, u.kind_n, EF_AB_PERSIST, &persist));
    if (killed && persist) {
        EfXy position = advance ? target : origin;
        int scout;
        EF_TRY(ef_kind_has(t, u.kind_n, EF_AB_SCOUT, &scout));
        EfTargets targets;
        EF_TRY(ef_options_at(g, t, p, position, range, range <= 1 + scout, &u, 0, &targets));
        for (int k = 0; k < targets.len; k++)
            if (!ef_xy_eq(targets.at[k], target)) { ef_units(g)[node].attacked_n = 0; break; } /* narrow: a flag or constant */
    }
    if (ef_unit_has(t, u.kind_n, EF_AB_FREEZE)) {
        /* a freezing attack hits and freezes in one action, with no advance or retaliation */
        EfAction f = ef_action(EF_A_FREEZE_UNIT, p);
        f.p[0] = origin;
        f.p[1] = target;
        f.i[0] = dealt;
        return ef_push(g, f);
    }
    int32_t delay;
    if (advance) {
        EfAction m = ef_action(EF_A_MOVE, p);
        m.i[0] = (int32_t)id;
        m.i[1] = 0;
        m.i[2] = 1;
        m.len = 2;
        m.list.path[0] = target;
        m.list.path[1] = origin;
        EF_TRY(ef_push(g, m));
        delay = 100;
    } else if (back > 0) {
        EF_TRY(ef_push(g, ef_attack_action(p, target, origin, back, 1, 0, 100)));
        delay = 250;
    } else {
        delay = 100;
    }
    int splash;
    EF_TRY(ef_kind_has(t, u.kind_n, EF_AB_SPLASH, &splash));
    if (splash && range > 1) {
        for (int32_t x = target.x - 1; x <= target.x + 1; x++) {
            for (int32_t y = target.y - 1; y <= target.y + 1; y++) {
                EfXy c = ef_xy(x, y);
                if (ef_xy_eq(c, target)) continue;
                const EfTile *ct = ef_tile_c(g, c);
                if (!ct || ct->unit_n == EF_NONE) continue;
                EfUnit e = ef_cunits(g)[ct->unit_n];
                int peace;
                EF_TRY(ef_at_peace(g, p, e.owner_n, &peace));
                if (peace) continue;
                if (ef_unit_has(t, u.kind_n, EF_AB_POISON)) {
                    EfAction a = ef_action_at(EF_A_POISON, p, origin);
                    a.p[1] = c;
                    EF_TRY(ef_push(g, a));
                    continue;
                }
                int32_t sd, sb;
                int sa, sr;
                EF_TRY(ef_battle_full(g, t, &u, &e, &sd, &sb, &sa, &sr));
                EF_TRY(ef_push(g, ef_attack_action(p, origin, c, ef_half_damage(sd), 2, 0, 20)));
            }
        }
    }
    EF_TRY(ef_push(g, ef_attack_action(p, origin, target, dealt, 1, advance, delay)));
    if (ef_unit_effect(g, &u, EF_INVISIBLE_EFFECT)) {
        EfAction r = ef_action_at(EF_A_REVEAL, p, u.coordinates);
        r.p[1] = ef_xy(-1, -1);
        EF_TRY(ef_push(g, r));
    }
    if (reveal) {
        if (ef_records_achievements(g)) return ef_err(EF_ERR_ACHIEVEMENTS, 0);
        int i;
        EF_TRY(ef_player(g, p, &i));
        ef_reset_peace(g, i);
    }
    return ef_ok();
}

/* attack.rs area_damage: a half-damage attack on every
 * neighbour that is neither its own nor at peace or cooling down */
EF_BIG EfResult ef_area_damage(EfGame *g, const EfTables *t, int32_t node, EfXy center) {
    EfUnit attacker = ef_cunits(g)[node];
    for (int32_t x = center.x - 1; x <= center.x + 1; x++) {
        for (int32_t y = center.y - 1; y <= center.y + 1; y++) {
            EfXy target = ef_xy(x, y);
            if (ef_xy_eq(target, center)) continue;
            const EfTile *tile = ef_tile_c(g, target);
            if (!tile || tile->unit_n == EF_NONE) continue;
            EfUnit d = ef_cunits(g)[tile->unit_n];
            int peace;
            EF_TRY(ef_at_peace(g, attacker.owner_n, d.owner_n, &peace));
            if (peace) continue;
            int32_t dealt, back;
            int advance, reveal;
            EF_TRY(ef_battle_full(g, t, &attacker, &d, &dealt, &back, &advance, &reveal));
            if (reveal) {
                int i;
                EF_TRY(ef_player(g, attacker.owner_n, &i));
                ef_reset_peace(g, i);
                EF_TRY(ef_attack_diplomacy(g, attacker.owner_n, d.owner_n));
                if (ef_records_achievements(g)) return ef_err(EF_ERR_ACHIEVEMENTS, 0);
            }
            int tentacles;
            EF_TRY(ef_kind_has(t, attacker.kind_n, EF_AB_TENTACLES, &tentacles));
            int32_t damage = tentacles ? dealt : ef_half_damage(dealt);
            EF_TRY(ef_push(g, ef_attack_action(attacker.owner_n, center, target, damage, tentacles ? 3 : 2, 0, 20)));
        }
    }
    return ef_ok();
}

EF_FN void ef_set_direction(EfGame *g, int32_t node, EfXy from, EfXy to) {
    int32_t d = ef_facing_direction(from, to);
    ef_units(g)[node].direction_n = (int8_t)d; /* narrow: ef_facing_direction gives 0 to 8 */
    ef_units(g)[node].flipped_n = d < 4; /* narrow: a flag or constant */
}
EF_FN void ef_remove_effect(EfGame *g, int32_t node, int64_t effect) {
    EfEffectList *e = &ef_lists(g)[ef_units(g)[node].effects_n];
    int at = ef_effect_index(e, effect);
    if (at >= 0) ef_effect_remove_at(e, at);
}

EF_BIG EfResult ef_hit(EfGame *g, const EfTables *t, int32_t actor, int actor_index, int32_t source, int32_t defender, EfXy origin,
                       EfXy target, int32_t damage);

/* attack.rs perform_attack (the Combat Attack action) */
EF_BIG EfResult ef_perform_attack(EfGame *g, const EfTables *t, int32_t supplied, EfXy origin, EfXy target, int32_t damage) {
    const EfTile *ot = ef_tile_c(g, origin);
    int32_t source = ot ? ot->unit_n : EF_NONE;
    int32_t ti = ef_tile_index(g, target);
    if (ti == EF_NONE) return ef_err(EF_ERR_ATTACK_TARGET_TILE, 0);
    int32_t defender = ef_ctiles(g)[ti].unit_n;
    if (source != EF_NONE) {
        ef_set_direction(g, source, origin, target);
        ef_remove_effect(g, source, EF_BOOSTED);
    }
    int32_t actor = !ef_xy_eq(origin, target) ? (source != EF_NONE ? ef_cunits(g)[source].owner_n : supplied) : supplied;
    int actor_index = ef_player_index(g, actor);
    if (defender != EF_NONE) EF_TRY(ef_hit(g, t, actor, actor_index, source, defender, origin, target, damage));
    /* Drench: the target tile floods after the hit */
    if (source != EF_NONE && ef_unit_has(t, ef_cunits(g)[source].kind_n, EF_AB_DRENCH) && ef_floodable(&ef_ctiles(g)[ti]))
        EF_TRY(ef_push(g, ef_action_at(EF_A_FLOOD, actor, target)));
    return ef_ok();
}

/* attack.rs perform_attack, the part on a defender */
EF_BIG EfResult ef_hit(EfGame *g, const EfTables *t, int32_t actor, int actor_index, int32_t source, int32_t defender, EfXy origin,
                       EfXy target, int32_t damage) {
    ef_set_direction(g, defender, target, origin);
    ef_remove_effect(g, defender, EF_BOOSTED);
    ef_remove_effect(g, defender, EF_SWIFT);
    EfUnit snapshot = ef_cunits(g)[defender];
    /* aquarion.rs bubble_lost: an attacked unit loses its Bubble, before the damage */
    if (ef_unit_effect(g, &snapshot, EF_BUBBLE)) {
        EfAction b = ef_action(EF_A_BUBBLE, ef_mover(g));
        b.i[0] = (int32_t)snapshot.id;
        b.i[1] = 0;
        EF_TRY(ef_push(g, b));
    }
    int16_t sgn = (int16_t)(uint16_t)(uint32_t)snapshot.health_n;
    int32_t m = damage < (int32_t)sgn ? damage : (int32_t)sgn;
    int16_t dealt = (int16_t)(uint16_t)(uint32_t)m;
    uint16_t health = (uint16_t)((uint16_t)sgn - (uint16_t)dealt);
    ef_units(g)[defender].health_n = health; /* narrow: a u16, as the engine's */
    if (health == 0) {
        if (snapshot.owner_n != EF_NATURE) {
            if (actor_index == EF_NONE) return ef_err(EF_ERR_ATTACK_KILLER, 0);
            int ai = actor_index;
            g->players[ai].kills += 1;
            if (!(origin.x == -1 && origin.y == -1) && source != EF_NONE) {
                EfUnit su = ef_cunits(g)[source];
                int carry, statik;
                EF_TRY(ef_kind_has(t, su.kind_n, EF_AB_CARRY, &carry));
                const EfUnitData *sd = ef_unit_data(t, su.kind_n);
                if (!carry) {
                    if (!sd) return ef_err(EF_ERR_UNKNOWN_UNIT_KIND, 0);
                    if (!sd->hidden) {
                        EF_TRY(ef_kind_has(t, su.kind_n, EF_AB_STATIC, &statik));
                        if (!statik) {
                            EF_PUT(ef_units(g)[source].xp_n, ef_units(g)[source].xp_n + 1, int16_t);
                            if (ef_records_achievements(g)) return ef_err(EF_ERR_ACHIEVEMENTS, 0);
                        }
                    }
                }
            }
            int n = ef_task_slot(&g->players[ai], EF_KILLER_TASK);
            if (n < 0) {
                EfAction e = ef_action(EF_A_ENABLE_TASK, actor);
                e.i[0] = EF_KILLER_TASK;
                EF_TRY(ef_push(g, e));
            } else {
                EfTask *task = &g->players[ai].tasks[n];
                int bumped;
                if (task->completed) {
                    bumped = 1;
                } else {
                    int64_t k = (task->has_kills ? task->kills : 0) + 1;
                    task->has_kills = 1;
                    task->kills = k;
                    bumped = k >= 10;
                }
                if (bumped) EF_TRY(ef_check_task(g, t, ai, EF_KILLER_TASK));
            }
        }
        int o;
        EF_TRY(ef_player(g, snapshot.owner_n, &o));
        g->players[o].casualties += 1;
        return ef_push(g, ef_action_at(EF_A_KILL, actor, target));
    }
    if (source != EF_NONE && ef_unit_has(t, ef_cunits(g)[source].kind_n, EF_AB_POISON)) {
        EfAction a = ef_action_at(EF_A_POISON, actor, origin);
        a.p[1] = target;
        EF_TRY(ef_push(g, a));
    }
    return ef_ok();
}

/* attack.rs kill */
EF_FN EfResult ef_kill(EfGame *g, const EfTables *t, EfXy c) {
    int32_t i = ef_tile_index(g, c);
    if (i == EF_NONE) return ef_err(EF_ERR_KILL_TILE, 0);
    int32_t n = ef_ctiles(g)[i].unit_n;
    if (n == EF_NONE) return ef_err(EF_ERR_KILL_NO_UNIT, 0);
    ef_units(g)[n].health_n = 0; /* narrow: a flag or constant */
    EfUnit u = ef_cunits(g)[n];
    /* the follower of a dead head takes its flags and grows into a centipede; the leader lets go */
    if (u.follower != 0) {
        int32_t f = ef_find_unit(g, u.follower);
        if (f != EF_NONE) {
            ef_units(g)[f].moved_n = u.moved_n; /* narrow: a copy of the same field */
            ef_units(g)[f].attacked_n = u.attacked_n; /* narrow: a copy of the same field */
            if (t->centipede_error) return ef_err(EF_ERR_TABLE, t->centipede_error - 1);
            EfAction a = ef_action_at(EF_A_GROW_UP, u.owner_n, ef_cunits(g)[f].coordinates);
            a.i[0] = t->centipede;
            a.i[1] = 0;
            EF_TRY(ef_push(g, a));
        }
    }
    if (u.leader != 0) {
        int32_t l = ef_find_unit(g, u.leader);
        if (l != EF_NONE) ef_units(g)[l].follower = 0;
    }
    if (ef_unit_effect(g, &u, EF_POISONED)) EF_TRY(ef_spores(g, t, c));
    int32_t score = ef_unit_score(g, t, &u);
    ef_tiles(g)[i].unit_n = EF_NONE; /* narrow: EF_NONE */
    EfAction a = ef_action(EF_A_DECREASE_SCORE, u.owner_n);
    a.i[0] = score;
    EF_TRY(ef_push(g, a));
    if (ef_unit_has(t, u.kind_n, EF_AB_TENTACLES)) EF_TRY(ef_update_tentacles(g, t));
    return ef_ok();
}

/* attack.rs convert */
EF_BIG EfResult ef_convert(EfGame *g, const EfTables *t, int32_t actor, EfXy origin, EfXy target) {
    const EfTile *ot = ef_tile_c(g, origin);
    if (!ot) return ef_err(EF_ERR_CONVERT_ORIGIN, 0);
    int32_t source = ot->unit_n;
    const EfTile *tt = ef_tile_c(g, target);
    if (!tt || tt->unit_n == EF_NONE) return ef_err(EF_ERR_CONVERT_TARGET, 0);
    int32_t id = tt->unit_n;
    EfUnit original = ef_cunits(g)[id];
    if (original.owner_n == actor) return ef_ok();
    if (source == EF_NONE) return ef_err(EF_ERR_CONVERT_SOURCE, 0);
    EfUnit converter = ef_cunits(g)[source];
    int p;
    EF_TRY(ef_player(g, converter.owner_n, &p));
    if (original.owner_n != EF_NATURE) {
        int n = ef_task_slot(&g->players[p], EF_TASK_CONVERT);
        if (n >= 0 && g->players[p].tasks[n].started) {
            EfTask *task = &g->players[p].tasks[n];
            if (!task->completed) {
                task->conversions = (task->has_conversions ? task->conversions : 0) + 1;
                task->has_conversions = 1;
            }
            EF_TRY(ef_check_task(g, t, p, EF_TASK_CONVERT));
        }
    }
    /* a converted chain member brings its neighbours in the chain along */
    const uint32_t linked[2] = { original.follower, original.leader };
    for (int k = 0; k < 2; k++) {
        int32_t l = linked[k] != 0 ? ef_find_unit(g, linked[k]) : EF_NONE;
        if (l == EF_NONE || ef_cunits(g)[l].owner_n == actor) continue;
        EfAction a = ef_action_at(EF_A_CONVERT, actor, original.coordinates);
        a.p[1] = ef_cunits(g)[l].coordinates;
        EF_TRY(ef_push(g, a));
    }
    int escape;
    EF_TRY(ef_kind_has(t, converter.kind_n, EF_AB_ESCAPE, &escape));
    ef_units(g)[source].moved_n = !escape; /* narrow: a flag or constant */
    ef_units(g)[source].attacked_n = 1; /* narrow: a flag or constant */
    EfUnit *u = &ef_units(g)[id];
    u->owner_n = converter.owner_n; /* narrow: a copy of the same field */
    u->moved_n = 1; /* narrow: a flag or constant */
    u->attacked_n = 1; /* narrow: a flag or constant */
    int32_t passenger = u->passenger_n;
    int independent;
    EF_TRY(ef_kind_has(t, original.kind_n, EF_AB_INDEPENDENT, &independent));
    if (!independent) {
        EfXy capital;
        EF_TRY(ef_current_capital(g, g->players[p].id, &capital));
        ef_units(g)[id].home = capital;
        if (passenger != EF_NONE) { ef_units(g)[passenger].owner_n = converter.owner_n; ef_units(g)[passenger].home = capital; } /* narrow: a copy of the same field */
    } else if (passenger != EF_NONE) {
        ef_units(g)[passenger].owner_n = converter.owner_n; /* narrow: a copy of the same field */
    }
    int32_t score = ef_unit_score(g, t, &ef_cunits(g)[id]);
    EfAction inc = ef_action_at(EF_A_INCREASE_SCORE, actor, target);
    inc.i[0] = score;
    EF_TRY(ef_push(g, inc));
    EfAction dec = ef_action(EF_A_DECREASE_SCORE, original.owner_n);
    dec.i[0] = score;
    EF_TRY(ef_push(g, dec));
    const EfUnitData *d = ef_unit_data(t, ef_cunits(g)[id].kind_n);
    if (!d) return ef_err(EF_ERR_CONVERT_KIND, 0);
    return ef_explore_from(g, converter.owner_n, target, 1 + ef_has(d->abilities, EF_AB_SCOUT));
}

#endif /* EF_COMBAT_H */
