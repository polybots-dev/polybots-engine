/* floats.h - every floating-point expression of the rule path, one function each, so the
 * host twin and the device code can be compared with Rust bit for bit before any rule is
 * ported. The rule ports call these functions and write no
 * float arithmetic of their own. */
#ifndef EF_FLOATS_H
#define EF_FLOATS_H

#include "random.h"

/* menu.rs tech_price for a tier other than 0 (the caller gives 0 for tier 0, as the engine
 * does): ((cities - 1) * tier) as f32 + tier as f32 + 4.0, times the literacy multiplier when
 * the player has it, rounded up */
EF_FN int32_t ef_tech_price(int32_t cities, int32_t tier, int literacy) {
    float x = (float)ef_wmul(ef_wsub(cities, 1), tier) + (float)tier + 4.0f;
    if (literacy) x *= ef_bits_f32(0x3f2aaa3bu);
    return ef_f32_i32(ceilf(x));
}

/* movement.rs path: the distance to the destination, square root in f64 and narrowed, plus
 * the cost so far */
EF_FN float ef_move_priority(int32_t dx, int32_t dy, int32_t cost) {
    double x = (double)dx, y = (double)dy;
    return (float)sqrt(x * x + y * y) + (float)cost;
}

/* routes.rs router_path: the same with the squares summed as integers first */
EF_FN float ef_route_priority(int32_t ex, int32_t ey, int32_t cost) {
    return (float)sqrt((double)ef_wadd(ef_wmul(ex, ex), ef_wmul(ey, ey))) + (float)cost;
}

/* both searches: whether a cost is under (max_cost * 10) as f32 */
EF_FN int ef_under_limit(int32_t cost, int32_t max_cost) {
    return (float)cost < (float)ef_wmul(max_cost, 10);
}

/* explore.rs meet_action: (score as u32 as f32 / 1000.0).ceil() as i32 */
EF_FN int32_t ef_meet_tiers(uint32_t score) { return ef_f32_i32(ceilf((float)score / 1000.0f)); }

/* capture.rs city name: next_float() * 3.0 + 3.0 */
EF_FN float ef_name_length(int32_t next) { return (float)ef_sample(next) * 3.0f + 3.0f; }
/* ... and whether a word of `units` UTF-16 units is still shorter */
EF_FN int ef_name_short(int32_t units, float length) { return (float)units < length; }

/* infiltrate.rs rebellion_weight: the seeded jitter plus the terms that apply */
#define EF_REBEL_CITY 1
#define EF_REBEL_HILLY 2
#define EF_REBEL_RESOURCE 4
#define EF_REBEL_DEFENCE 8
EF_FN float ef_rebellion_jitter(uint32_t hash) { return ef_hash_range_f32(hash, 0.0f, ef_bits_f32(0x3dcccccdu)); }
EF_FN float ef_rebellion_terms(float weight, int terms) {
    if (terms & EF_REBEL_CITY) weight += 2.0f;
    if (terms & EF_REBEL_HILLY) weight += -0.1f;
    if (terms & EF_REBEL_RESOURCE) weight += 0.5f;
    if (terms & EF_REBEL_DEFENCE) weight += 1.0f;
    return weight;
}

/* combat.rs shown: an attack preview's displayed damage, `raw * 0.1` rounded half to even (the
 * default rounding mode; rintf, never roundf) */
EF_FN int32_t ef_damage_shown(int32_t raw) { return (int32_t)rintf((float)raw * 0.1f); }

/* queries.rs panel: a displayed figure in tenths, (x as f32 * 0.1).ceil() as i32 (the maximum
 * health) */
EF_FN int32_t ef_tenths_up(int32_t x) { return ef_f32_i32(ceilf((float)x * 0.1f)); }

/* ---- one entry point for the conformance check ---- */
enum {
    EF_OP_TECH_PRICE, EF_OP_MOVE_PRIORITY, EF_OP_ROUTE_PRIORITY, EF_OP_UNDER_LIMIT, EF_OP_MEET_TIERS,
    EF_OP_HASH_VALUE, EF_OP_HASH_RANGE_F32, EF_OP_HASH_RANGE_I32, EF_OP_SAMPLE, EF_OP_NEXT_FLOAT,
    EF_OP_FLOAT_RANGE, EF_OP_MAX_OF, EF_OP_RANGE_OF, EF_OP_WIDE_RANGE, EF_OP_NAME_LENGTH,
    EF_OP_NAME_SHORT, EF_OP_REBELLION, EF_OP_DAMAGE_SHOWN, EF_OP_TENTHS_UP, EF_OP_COUNT
};

/* Evaluate expression `op` on raw 32-bit arguments (floats are passed as their bits) and give
 * the result's bits. */
EF_FN uint64_t ef_float_case(int32_t op, uint32_t a, uint32_t b, uint32_t c, uint32_t d) {
    int32_t ia = (int32_t)a, ib = (int32_t)b, ic = (int32_t)c, id = (int32_t)d;
    switch (op) {
    case EF_OP_TECH_PRICE: return (uint32_t)ef_tech_price(ia, ib, ic & 1);
    case EF_OP_MOVE_PRIORITY: return ef_f32_bits(ef_move_priority(ia, ib, ic));
    case EF_OP_ROUTE_PRIORITY: return ef_f32_bits(ef_route_priority(ia, ib, ic));
    case EF_OP_UNDER_LIMIT: return (uint64_t)ef_under_limit(ia, ib);
    case EF_OP_MEET_TIERS: return (uint32_t)ef_meet_tiers(a);
    case EF_OP_HASH_VALUE: return ef_f32_bits(ef_hash_value(a));
    case EF_OP_HASH_RANGE_F32: return ef_f32_bits(ef_hash_range_f32(a, ef_bits_f32(b), ef_bits_f32(c)));
    case EF_OP_HASH_RANGE_I32: return ef_wsub(ic, ib) == 0 ? 0xffffffffffffffffull : (uint32_t)ef_hash_range_i32(a, ib, ic);
    case EF_OP_SAMPLE: return ef_f64_bits(ef_sample(ia));
    case EF_OP_NEXT_FLOAT: return ef_f32_bits((float)ef_sample(ia));
    case EF_OP_FLOAT_RANGE: return ef_f32_bits(ef_float_range(ef_sample(ia), ef_bits_f32(b), ef_bits_f32(c)));
    case EF_OP_MAX_OF: return (uint32_t)ef_max_of(ef_sample(ia), ib);
    case EF_OP_RANGE_OF: return (uint32_t)ef_range_of(ef_sample(ia), ib, ic);
    case EF_OP_WIDE_RANGE: return (uint32_t)ef_range_of(ef_wide_sample(ia, id), ib, ic);
    case EF_OP_NAME_LENGTH: return ef_f32_bits(ef_name_length(ia));
    case EF_OP_NAME_SHORT: return (uint64_t)ef_name_short(ib, ef_name_length(ia));
    /* the weight negated, as infiltrate.rs uses it for its sort key */
    case EF_OP_REBELLION: return ef_f32_bits(-ef_rebellion_terms(ef_rebellion_jitter(a), ib & 15));
    case EF_OP_DAMAGE_SHOWN: return (uint32_t)ef_damage_shown(ia);
    case EF_OP_TENTHS_UP: return (uint32_t)ef_tenths_up(ia);
    default: return 0;
    }
}

#endif /* EF_FLOATS_H */
