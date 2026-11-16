/* ef.h - shared definitions of the engine-fast kernel sources.
 *
 * One set of headers, built twice: by a C compiler as the host twin (twin.c) and by nvcc as
 * device code (device.cu). Every function is EF_FN, allocates nothing, never recurses and
 * works on fixed-size storage. The game is the Rust engine in engine-fast/src; each
 * header ports the Rust file named at its top and is checked bit for bit against it.
 *
 * Numeric rules for both builds: no fused multiply-add, no fast-math; arithmetic that wraps
 * in Rust is written on unsigned types; a float converted to an integer goes through the
 * helpers below, which do what Rust's `as` does.
 *
 * The EF_FN macro and the cast and wrapping helpers follow the approach of engine-cu's pc.h
 * (see ../THIRD_PARTY.md). */
#ifndef EF_H
#define EF_H

#include <stdint.h>
#include <string.h>
#include <math.h>

#ifdef __CUDACC__
#define EF_FN static inline __host__ __device__
#define EF_THREAD_LOCAL thread_local
/* a large rule function: kept out of line on the device, where inlining the whole step into one
 * kernel made the build take tens of minutes and the kernel use more registers */
#define EF_BIG static __host__ __device__ __noinline__
#else
#define EF_FN static inline
#define EF_THREAD_LOCAL _Thread_local
#define EF_BIG static inline
#endif

/* ---- Rust `as` casts ---- */
/* f64 as i32: saturating, NaN -> 0 */
EF_FN int32_t ef_f64_i32(double x) {
    if (x != x) return 0;
    if (x >= 2147483648.0) return INT32_MAX;
    if (x <= -2147483648.0) return INT32_MIN;
    return (int32_t)x;
}
/* f32 as i32: saturating, NaN -> 0 */
EF_FN int32_t ef_f32_i32(float x) {
    if (x != x) return 0;
    if (x >= 2147483648.0f) return INT32_MAX;
    if (x <= -2147483648.0f) return INT32_MIN;
    return (int32_t)x;
}
/* f64 as i64: saturating, NaN -> 0 */
EF_FN int64_t ef_f64_i64(double x) {
    if (x != x) return 0;
    if (x >= 9223372036854775808.0) return INT64_MAX;
    if (x <= -9223372036854775808.0) return INT64_MIN;
    return (int64_t)x;
}

/* ---- wrapping i32 arithmetic (Rust wrapping_add / wrapping_sub / wrapping_mul) ---- */
EF_FN int32_t ef_wadd(int32_t a, int32_t b) { return (int32_t)((uint32_t)a + (uint32_t)b); }
EF_FN int32_t ef_wsub(int32_t a, int32_t b) { return (int32_t)((uint32_t)a - (uint32_t)b); }
EF_FN int32_t ef_wmul(int32_t a, int32_t b) { return (int32_t)((uint32_t)a * (uint32_t)b); }
/* u32::rotate_left, 0 < r < 32 */
EF_FN uint32_t ef_rotl(uint32_t x, int r) { return (x << r) | (x >> (32 - r)); }

/* ---- the bits of a float, and back ---- */
EF_FN uint32_t ef_f32_bits(float x) { uint32_t u; memcpy(&u, &x, sizeof u); return u; }
EF_FN float ef_bits_f32(uint32_t u) { float x; memcpy(&x, &u, sizeof x); return x; }
EF_FN uint64_t ef_f64_bits(double x) { uint64_t u; memcpy(&u, &x, sizeof u); return u; }

#endif /* EF_H */
