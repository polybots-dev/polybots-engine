/* random.h - port of engine-fast/src/random.rs: the seeded random stream (city names)
 * and the seeded integer hash the game state draws from. */
#ifndef EF_RANDOM_H
#define EF_RANDOM_H

#include "ef.h"

/* ---- SystemRandom ---- */
typedef struct {
    int32_t values[56];
    int32_t current, lag;
} EfSystemRandom;

EF_FN void ef_random_new(EfSystemRandom *r, int32_t seed) {
    int32_t magnitude = seed == INT32_MIN ? INT32_MAX : (seed < 0 ? -seed : seed);
    int32_t previous = ef_wsub(161803398, magnitude);
    int32_t next = 1;
    for (int i = 0; i < 56; i++) r->values[i] = 0;
    r->values[55] = previous;
    for (int i = 1; i < 55; i++) {
        int index = 21 * i % 55;
        r->values[index] = next;
        next = ef_wsub(previous, next);
        if (next < 0) next = ef_wadd(next, INT32_MAX);
        previous = r->values[index];
    }
    for (int round = 0; round < 4; round++) {
        for (int i = 1; i < 56; i++) {
            r->values[i] = ef_wsub(r->values[i], r->values[1 + (i + 30) % 55]);
            if (r->values[i] < 0) r->values[i] = ef_wadd(r->values[i], INT32_MAX);
        }
    }
    r->current = 0;
    r->lag = 21;
}

EF_FN int32_t ef_random_next(EfSystemRandom *r) {
    r->current = r->current == 55 ? 1 : r->current + 1;
    r->lag = r->lag == 55 ? 1 : r->lag + 1;
    int32_t value = ef_wsub(r->values[r->current], r->values[r->lag]);
    if (value == INT32_MAX) value -= 1;
    if (value < 0) value = ef_wadd(value, INT32_MAX);
    r->values[r->current] = value;
    return value;
}

/* next_double on a value already drawn: f64::from(next) * (1.0 / f64::from(i32::MAX)) */
EF_FN double ef_sample(int32_t next) { return (double)next * (1.0 / 2147483647.0); }

EF_FN double ef_random_next_double(EfSystemRandom *r) { return ef_sample(ef_random_next(r)); }
EF_FN float ef_random_next_float(EfSystemRandom *r) { return (float)ef_random_next_double(r); }

/* (f64::from(max - min) * sample + f64::from(min)) as f32 */
EF_FN float ef_float_range(double sample, float min, float max) {
    return (float)((double)(max - min) * sample + (double)min);
}
EF_FN float ef_random_next_float_range(EfSystemRandom *r, float min, float max) {
    return ef_float_range(ef_random_next_double(r), min, max);
}

/* next_max for max >= 0 (the caller refuses a negative maximum) */
EF_FN int32_t ef_max_of(double sample, int32_t max) { return ef_f64_i32(sample * (double)max); }
EF_FN int32_t ef_random_next_max(EfSystemRandom *r, int32_t max) {
    return ef_max_of(ef_random_next_double(r), max);
}

/* the wide-range sample of next_range from its two draws */
EF_FN double ef_wide_sample(int32_t first, int32_t second) {
    int32_t s = second % 2 == 0 ? ef_wsub(0, first) : first;
    return ((double)s + 2147483646.0) / 4294967293.0;
}
/* ((sample * width as f64) as i64 + i64::from(min)) as i32 */
EF_FN int32_t ef_range_of(double sample, int32_t min, int32_t max) {
    int64_t width = (int64_t)max - (int64_t)min;
    return (int32_t)(uint32_t)((uint64_t)ef_f64_i64(sample * (double)width) + (uint64_t)(int64_t)min);
}
/* next_range for min <= max (the caller refuses a reversed range) */
EF_FN int32_t ef_random_next_range(EfSystemRandom *r, int32_t min, int32_t max) {
    int64_t width = (int64_t)max - (int64_t)min;
    double sample;
    if (width <= (int64_t)INT32_MAX) {
        sample = ef_random_next_double(r);
    } else {
        int32_t first = ef_random_next(r);
        sample = ef_wide_sample(first, ef_random_next(r));
    }
    return ef_range_of(sample, min, max);
}

/* ---- the integer hash ---- */
EF_FN uint32_t ef_avalanche(uint32_t h) {
    h = (h ^ (h >> 15)) * 0x85ebca77u;
    h = (h ^ (h >> 13)) * 0xc2b2ae3du;
    return h ^ (h >> 16);
}

/* hash_words (and hash_ints, whose words are the integers' raw bits) */
EF_FN uint32_t ef_hash_words(uint32_t seed, const uint32_t *words, int count) {
    int offset = 0;
    uint32_t hash;
    if (count >= 4) {
        uint32_t lanes[4];
        lanes[0] = seed + 0x24234428u;
        lanes[1] = seed + 0x85ebca77u;
        lanes[2] = seed;
        lanes[3] = seed - 0x9e3779b1u;
        while (offset + 4 <= count) {
            for (int lane = 0; lane < 4; lane++)
                lanes[lane] = ef_rotl(lanes[lane] + words[offset + lane] * 0x85ebca77u, 13) * 0x9e3779b1u;
            offset += 4;
        }
        hash = ef_rotl(lanes[0], 1) + ef_rotl(lanes[1], 7) + ef_rotl(lanes[2], 12) + ef_rotl(lanes[3], 18);
    } else {
        hash = seed + 0x165667b1u;
    }
    hash += (uint32_t)count * 4u;
    for (int i = offset; i < count; i++) hash = ef_rotl(hash + words[i] * 0xc2b2ae3du, 17) * 0x27d4eb2fu;
    return ef_avalanche(hash);
}

/* hash_value: the hash rounded to f32, times 2^-32 */
EF_FN float ef_hash_value(uint32_t hash) { return (float)hash * ef_bits_f32(0x2f800000u); }

/* hash_range_i32 for a non-zero wrapped width (the caller refuses zero) */
EF_FN int32_t ef_hash_range_i32(uint32_t hash, int32_t min, int32_t max) {
    int32_t width = ef_wsub(max, min);
    int64_t remainder = (int64_t)hash % (int64_t)width;
    return ef_wadd(min, (int32_t)remainder);
}

/* hash_range_f32: width * rounded hash * 2^-32 + min, all in f32 */
EF_FN float ef_hash_range_f32(uint32_t hash, float min, float max) {
    float width = max - min;
    float scaled = width * (float)hash;
    float normalized = scaled * ef_bits_f32(0x2f800000u);
    return normalized + min;
}

#endif /* EF_RANDOM_H */
