/* device.cu - the kernel sources as CUDA device code, behind a small C interface. */
#include <cuda_runtime.h>
#include <stdlib.h>
#include "floats.h"
#include "state.h"
#include "view.h"
#include "step.h"
#include "status.h"
#include "outputs.h"

/* The batches' blocking streams order against the legacy default stream (ef_cuda_step_launch);
 * a per-thread default stream would drop that ordering. */
#ifdef CUDA_API_PER_THREAD_DEFAULT_STREAM
#error "the device batch needs the legacy default stream"
#endif

/* A launch's capacities: every block in it has room for `tiles` tiles and `units` units, and
 * every view room for `tiles` tiles. */
typedef struct {
    int32_t tiles, units;
} EfCaps;
static int ef_caps_ok(EfCaps c) { return c.tiles >= 1 && c.tiles <= EF_MAX_TILES && c.units >= 1 && c.units <= EF_MAX_UNITS; }
/* game i, and view i, of a buffer of them laid end to end */
#define EF_AT(b, c, i) ((EfGame *)((char *)(b) + (size_t)ef_block_size((c).tiles, (c).units) * (size_t)(i)))
#define EF_CAT(b, c, i) ((const EfGame *)((const char *)(b) + (size_t)ef_block_size((c).tiles, (c).units) * (size_t)(i)))
#define EF_VIEW_AT(v, c, i) ((EfView *)((char *)(v) + (size_t)ef_view_size((c).tiles) * (size_t)(i)))

/* the step, compiled once and called by both step kernels (inlining it into each doubled the
 * build time) */
__device__ __noinline__ EfResult ef_device_advance(EfGame *g, const EfTables *t, const EfMove *m, int32_t *elapsed) {
    return ef_advance(g, t, m, elapsed);
}

/* the legal list, compiled once like the step */
__device__ __noinline__ EfResult ef_device_menu(const EfGame *g, EfCaps caps, const EfTables *t, EfMenuWork *w, EfMenu *m) {
    EfResult r = ef_game_check(g, caps.tiles, caps.units);
    m->len = 0;
    if (r.code == EF_OK) r = ef_menu(g, t, w, m);
    return r;
}

__global__ void k_menus(const EfGame *games, EfCaps caps, int count, const EfTables *tables, EfMenuWork *work, EfMenu *menus, EfResult *results) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    results[g] = ef_device_menu(EF_CAT(games, caps, g), caps, tables, &work[g], &menus[g]);
}

/* the batch's slots: index -2 refreshes a slot after its upload (its check result first), -1
 * leaves it, k >= 0 takes row k of its list; a playing slot's view for its player to move is
 * in views[g] and counts[g] is its number of rows */
__device__ __noinline__ EfResult ef_device_slot(EfGame *g, const EfTables *t, EfMenuWork *w, EfMenu *m, EfView *v,
                                                int32_t index, int32_t *status, int32_t *winner) {
    return index == -2 ? ef_refresh(g, t, w, m, v, status, winner) : ef_slot_step(g, t, w, m, v, index, status, winner);
}

/* The arrays a batch exports in device memory besides its views' planes and its lists' rows
 *: per slot the viewer, turn and stars; four player rows
 * of seat, tribe, relation and score; the viewer's technologies. -1 where unused, and in every
 * row of a slot that is not playing, as the Python package's views. */
#define EF_OUT_SCALARS 3
#define EF_OUT_PLAYERS 4
#define EF_OUT_PLAYER_FIELDS 4
/* the optional outputs a batch is made with: each is
 * computed and allocated only when asked for */
#define EF_WANT_PREVIEWS 1
#define EF_WANT_DEFENCE 2
typedef struct {
    int32_t *scalars;
    int64_t *players;
    int16_t *tech;
    int16_t (*previews)[4];   /* EF_MAX_ROWS per slot, or NULL */
    int16_t *defence;         /* cells per slot, or NULL */
    int32_t techs, cells;
} EfOut;

/* After slot g ran: no list row past its length holds an earlier decision's move (the rows are
 * exported in place: after a turn change they were another player's), a slot not playing has an
 * empty list and planes of -1, and the exported arrays are written from its view. */
__device__ void ef_slot_epilogue(int g, EfMenu *m, int32_t before, EfView *v, int32_t status, EfOut o) {
    int playing = status == EF_PLAYING;
    int32_t len = playing ? m->len : 0;
    int32_t top = before > m->len ? before : m->len;
    if (top > EF_MAX_ROWS) top = EF_MAX_ROWS;
    for (int32_t k = len; k < top; k++)
        for (int f = 0; f < 6; f++) m->rows[k].f[f] = 0;
    m->len = len;
    if (o.previews) {
        int16_t (*pv)[4] = o.previews + (size_t)g * EF_MAX_ROWS;
        for (int32_t k = len; k < top; k++)
            for (int f = 0; f < 4; f++) pv[k][f] = 0;
    }
    if (o.defence && !playing)
        for (int k = 0; k < o.cells; k++) o.defence[(size_t)g * (size_t)o.cells + (size_t)k] = -1;
    int32_t *sc = o.scalars + (size_t)g * EF_OUT_SCALARS;
    int64_t *pl = o.players + (size_t)g * EF_OUT_PLAYERS * EF_OUT_PLAYER_FIELDS;
    int16_t *te = o.tech + (size_t)g * (size_t)o.techs;
    for (int k = 0; k < EF_OUT_PLAYERS * EF_OUT_PLAYER_FIELDS; k++) pl[k] = -1;
    for (int k = 0; k < o.techs; k++) te[k] = -1;
    if (!playing) {
        int16_t *tiles = ef_view_tiles(v);
        for (int k = 0; k < EF_VIEW_PLANES * o.cells; k++) tiles[k] = -1;
        for (int k = 0; k < EF_OUT_SCALARS; k++) sc[k] = -1;
        return;
    }
    sc[0] = v->viewer;
    sc[1] = v->turn;
    sc[2] = v->stars;
    int np = v->players_len < EF_OUT_PLAYERS ? v->players_len : EF_OUT_PLAYERS;
    for (int k = 0; k < np; k++) {
        pl[k * EF_OUT_PLAYER_FIELDS + 0] = v->players[k].seat;
        pl[k * EF_OUT_PLAYER_FIELDS + 1] = v->players[k].tribe;
        pl[k * EF_OUT_PLAYER_FIELDS + 2] = v->players[k].relation;
        pl[k * EF_OUT_PLAYER_FIELDS + 3] = v->players[k].score;
    }
    int nt = v->tech_len < o.techs ? v->tech_len : o.techs;
    for (int k = 0; k < nt; k++) te[k] = v->tech[k];
}

/* Run slot g by `index` (-2 refresh after an upload, k take row k), only while its block has
 * passed its check (on upload) and every run since; a slot that stops keeps its reason until a
 * new upload, and its status says Refused or Failed. */
__device__ EfResult ef_run_slot(int g, EfGame *games, EfCaps caps, const EfTables *tables, EfResult *checks,
                                EfMenuWork *work, EfMenu *menus, EfView *views, int32_t index, int32_t *status,
                                int32_t *winners, int32_t *counts, EfOut o) {
    EfMenu *m = &menus[g];
    int32_t before = m->len;
    EfResult r = checks[g];
    EfGame *game = EF_AT(games, caps, g);
    if (r.code == EF_OK)
        r = ef_device_slot(game, tables, &work[g], m, EF_VIEW_AT(views, caps, g), index, &status[g], &winners[g]);
    /* the optional outputs of a slot left playing, for its player to move */
    if (r.code == EF_OK && status[g] == EF_PLAYING && o.previews)
        r = ef_menu_previews(game, tables, m, o.previews + (size_t)g * EF_MAX_ROWS);
    if (r.code == EF_OK && status[g] == EF_PLAYING && o.defence)
        r = ef_defence_plane(game, tables, game->players[game->current_player_index].id,
                             o.defence + (size_t)g * (size_t)o.cells);
    if (r.code != EF_OK) {
        checks[g] = r;
        status[g] = ef_error_is_failure(r.code) ? EF_FAILED : EF_REFUSED;
        winners[g] = 0;
    }
    ef_slot_epilogue(g, m, before, EF_VIEW_AT(views, caps, g), status[g], o);
    counts[g] = m->len;
    return r;
}

__global__ void k_slots(EfGame *games, EfCaps caps, int count, const EfTables *tables, EfResult *checks, EfMenuWork *work,
                        EfMenu *menus, EfView *views, const int32_t *index, int32_t *status, int32_t *winners,
                        EfResult *results, int32_t *counts, EfOut o) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count || index[g] == -1) return;
    if (index[g] <= -3) {
        /* a reset that failed on the host (GpuBatch::reset): the slot stops, refused (-3) or
         * failed (-4), and exports nothing of the game it held */
        EfMenu *m = &menus[g];
        int32_t before = m->len;
        status[g] = index[g] == -4 ? EF_FAILED : EF_REFUSED;
        winners[g] = 0;
        checks[g] = ef_err(EF_ERR_INVARIANT, __LINE__);
        results[g] = checks[g];
        ef_slot_epilogue(g, m, before, EF_VIEW_AT(views, caps, g), status[g], o);
        counts[g] = 0;
        return;
    }
    results[g] = ef_run_slot(g, games, caps, tables, checks, work, menus, views, index[g], status, winners, counts, o);
}

/* One decision per slot from actions in device memory (a policy's output): -1 skips (applied
 * 0); a slot not playing, or an index not in its list, is rejected with nothing changed
 * (applied 2), as the CPU batch rejects it; otherwise the slot takes that row (applied 1).
 * word[0] counts the slots not playing afterwards and word[1] those that stopped in this call:
 * the only bytes a step returns to the host. */
__global__ void k_device_step(EfGame *games, EfCaps caps, int count, const EfTables *tables, EfResult *checks,
                              EfMenuWork *work, EfMenu *menus, EfView *views, const int32_t *actions, int32_t *applied,
                              int32_t *status, int32_t *winners, EfResult *results, int32_t *counts, EfOut o,
                              uint32_t *word) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    int32_t a = actions[g];
    if (a == -1) {
        applied[g] = 0;
    } else if (status[g] != EF_PLAYING || a < 0 || a >= counts[g]) {
        applied[g] = 2;
    } else {
        applied[g] = 1;
        EfResult r = ef_run_slot(g, games, caps, tables, checks, work, menus, views, a, status, winners, counts, o);
        results[g] = r;
        if (r.code != EF_OK) atomicAdd(&word[1], 1u);
    }
    if (status[g] != EF_PLAYING) atomicAdd(&word[0], 1u);
}

/* A uniform random legal row per playing slot, never Resign (kind 35), -1 elsewhere: the
 * benchmark's policy, on the device (batch_bench's on the host) */
__global__ void k_random_actions(const EfMenu *menus, const int32_t *status, const int32_t *counts, int count,
                                 uint64_t seed, int32_t *actions) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    if (status[g] != EF_PLAYING) { actions[g] = -1; return; }
    int32_t n = counts[g], kept = 0;
    for (int32_t k = 0; k < n; k++) kept += menus[g].rows[k].f[0] != 35;
    if (kept == 0) { actions[g] = 0; return; }
    uint64_t x = seed ^ (0x9E3779B97F4A7C15ull * (uint64_t)(g + 1));
    x ^= x >> 30; x *= 0xBF58476D1CE4E5B9ull; x ^= x >> 27; x *= 0x94D049BB133111EBull; x ^= x >> 31;
    int32_t pick = (int32_t)(x % (uint64_t)kept);
    for (int32_t k = 0; k < n; k++) {
        if (menus[g].rows[k].f[0] == 35) continue;
        if (pick-- == 0) { actions[g] = k; return; }
    }
}

/* the rows of every slot, packed: slot g's rows from offsets[g] on */
__global__ void k_gather_rows(const EfMenu *menus, int count, const int64_t *offsets, const int32_t *counts, int64_t total,
                              EfRow *rows) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    /* the caller sized `rows` from its own counts: a slot that would not fit is left out */
    if (offsets[g] < 0 || offsets[g] > total - counts[g]) return;
    for (int k = 0; k < counts[g]; k++) rows[offsets[g] + k] = menus[g].rows[k];
}

__global__ void k_float_cases(int32_t op, int64_t count, const uint32_t *args, uint64_t *out) {
    int64_t i = (int64_t)blockIdx.x * blockDim.x + threadIdx.x;
    if (i < count) out[i] = ef_float_case(op, args[4 * i], args[4 * i + 1], args[4 * i + 2], args[4 * i + 3]);
}

__global__ void k_game_digests(const EfGame *games, EfCaps caps, int count, uint64_t *digests, int64_t *probes) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    const EfGame *b = EF_CAT(games, caps, g);
    digests[g] = ef_game_digest(b, ef_block_size(caps.tiles, caps.units));
    /* the probe follows the header's capacities, so only where they are the launch's */
    if (ef_game_check(b, caps.tiles, caps.units).code == EF_OK) ef_game_probe(b, probes + 4 * (size_t)g);
}

__global__ void k_views(const EfGame *games, EfCaps caps, int count, const EfTables *tables, const int32_t *viewers, EfView *views,
                        EfResult *results) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    EfResult r = ef_game_check(EF_CAT(games, caps, g), caps.tiles, caps.units);
    results[g] = r.code == EF_OK ? ef_view_write(EF_CAT(games, caps, g), tables, viewers[g], EF_VIEW_AT(views, caps, g)) : r;
}

/* the persistent batch: one decision in each slot whose block passed its check on upload and
 * has not stopped since; a slot that stops keeps its reason and is not run again */
__global__ void k_checked_advance(EfGame *games, EfCaps caps, int count, const EfTables *tables, const EfMove *moves,
                                  EfResult *checks, EfResult *results, int32_t *elapsed) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    if (checks[g].code != EF_OK) { results[g] = checks[g]; return; }
    EfResult r = ef_device_advance(EF_AT(games, caps, g), tables, &moves[g], &elapsed[g]);
    if (r.code != EF_OK) checks[g] = r;
    results[g] = r;
}

/* the persistent batch: views of the slots whose last check passed */
__global__ void k_checked_views(const EfGame *games, EfCaps caps, int count, const EfTables *tables, const int32_t *viewers,
                                const EfResult *checks, EfView *views, EfResult *results) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    results[g] = checks[g].code == EF_OK ? ef_view_write(EF_CAT(games, caps, g), tables, viewers[g], EF_VIEW_AT(views, caps, g)) : checks[g];
}

/* the uploaded blocks into their slots: CUDA block b copies staged block b to slot slots[b],
 * eight bytes per thread step (a block's size is a multiple of 8, ef_block_size) */
__global__ void k_place(EfGame *games, size_t stride, const uint64_t *staged, const int32_t *slots, int count) {
    int b = blockIdx.x;
    if (b >= count) return;
    uint64_t *to = (uint64_t *)((char *)games + (size_t)slots[b] * stride);
    const uint64_t *from = staged + (size_t)b * (stride / 8);
    for (size_t k = threadIdx.x; k < stride / 8; k += blockDim.x) to[k] = from[k];
}

/* ef_game_check of the blocks just placed in `slots` */
__global__ void k_check_slots(const EfGame *games, EfCaps caps, const int32_t *slots, int count, EfResult *checks) {
    int k = blockIdx.x * blockDim.x + threadIdx.x;
    if (k >= count) return;
    int g = slots[k];
    checks[g] = ef_game_check(EF_CAT(games, caps, g), caps.tiles, caps.units);
}

__global__ void k_check(const EfGame *games, EfCaps caps, int first, int count, EfResult *checks) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    checks[first + g] = ef_game_check(EF_CAT(games, caps, first + g), caps.tiles, caps.units);
}

__global__ void k_advance(EfGame *games, EfCaps caps, int count, const EfTables *tables, const EfMove *moves, EfResult *results,
                          int32_t *elapsed) {
    int g = blockIdx.x * blockDim.x + threadIdx.x;
    if (g >= count) return;
    EfResult r = ef_game_check(EF_AT(games, caps, g), caps.tiles, caps.units);
    if (r.code == EF_OK) r = ef_device_advance(EF_AT(games, caps, g), tables, &moves[g], &elapsed[g]);
    results[g] = r;
}

/* Copy `tables` to `d_tables`, giving it scratch for the searches of `count` threads; the
 * scratch goes to *scratch, for the caller to free */
static cudaError_t ef_put_tables(EfTables *d_tables, const EfTables *tables, int32_t count, void **scratch) {
    *scratch = NULL;
    EfTables *copy = (EfTables *)malloc(sizeof(EfTables));
    if (!copy) return cudaErrorMemoryAllocation;
    memcpy(copy, tables, sizeof(EfTables));
    size_t n = count > 0 ? (size_t)count : 0;
    /* one region per thread, as large as the largest kind (movement.h EF_SCRATCH_TAKE) */
    size_t size = sizeof(EfSearch);
    if (sizeof(EfRouteWork) > size) size = sizeof(EfRouteWork);
    if (sizeof(EfConnectWork) > size) size = sizeof(EfConnectWork);
    size = (size + 15u) & ~(size_t)15u;
    cudaError_t e = cudaSuccess;
    if (n > 0) e = cudaMalloc(scratch, n * size);
    if (e == cudaSuccess && n > 0) {
        copy->scratch = (uint64_t)(uintptr_t)*scratch;
        copy->scratch_size = (uint64_t)size;
    }
    if (e == cudaSuccess) e = cudaMemcpy(d_tables, copy, sizeof(EfTables), cudaMemcpyHostToDevice);
    free(copy);
    return e;
}

extern "C" {

/* 0, or the CUDA error code of the first call that failed */
int ef_cuda_float_cases(int32_t op, int64_t count, const uint32_t *args, uint64_t *out) {
    if (count <= 0) return 0;
    uint32_t *d_args = NULL;
    uint64_t *d_out = NULL;
    size_t in_bytes = (size_t)count * 4 * sizeof(uint32_t), out_bytes = (size_t)count * sizeof(uint64_t);
    cudaError_t e = cudaMalloc(&d_args, in_bytes);
    if (e == cudaSuccess) e = cudaMalloc(&d_out, out_bytes);
    if (e == cudaSuccess) e = cudaMemcpy(d_args, args, in_bytes, cudaMemcpyHostToDevice);
    if (e == cudaSuccess) {
        k_float_cases<<<(unsigned)((count + 255) / 256), 256>>>(op, count, d_args, d_out);
        e = cudaGetLastError();
    }
    if (e == cudaSuccess) e = cudaMemcpy(out, d_out, out_bytes, cudaMemcpyDeviceToHost);
    cudaFree(d_args);
    cudaFree(d_out);
    return (int)e;
}

/* Upload `count` blocks, compute each one's digest and probe on the device, and download the
 * blocks into `back`; 0, or the CUDA error code. */
int ef_cuda_games_roundtrip(const EfGame *games, int32_t count, int32_t tile_cap, int32_t unit_cap, EfGame *back, uint64_t *digests, int64_t *probes) {
    if (count <= 0) return 0;
    EfCaps caps = {tile_cap, unit_cap};
    if (!ef_caps_ok(caps)) return (int)cudaErrorInvalidValue;
    size_t stride = (size_t)ef_block_size(tile_cap, unit_cap);
    EfGame *d_games = NULL;
    uint64_t *d_digests = NULL;
    int64_t *d_probes = NULL;
    size_t bytes = (size_t)count * stride;
    cudaError_t e = cudaMalloc(&d_games, bytes);
    if (e == cudaSuccess) e = cudaMalloc(&d_digests, (size_t)count * sizeof(uint64_t));
    if (e == cudaSuccess) e = cudaMalloc(&d_probes, (size_t)count * 4 * sizeof(int64_t));
    if (e == cudaSuccess) e = cudaMemcpy(d_games, games, bytes, cudaMemcpyHostToDevice);
    if (e == cudaSuccess) {
        k_game_digests<<<(unsigned)((count + 31) / 32), 32>>>(d_games, caps, count, d_digests, d_probes);
        e = cudaGetLastError();
    }
    if (e == cudaSuccess) e = cudaMemcpy(digests, d_digests, (size_t)count * sizeof(uint64_t), cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = cudaMemcpy(probes, d_probes, (size_t)count * 4 * sizeof(int64_t), cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = cudaMemcpy(back, d_games, bytes, cudaMemcpyDeviceToHost);
    cudaFree(d_games);
    cudaFree(d_digests);
    cudaFree(d_probes);
    return (int)e;
}

/* Upload `count` blocks and the tables, write each game's view for its viewer on the device,
 * and download the views and results (code and argument per game); 0, or the CUDA error code. */
int ef_cuda_views(const EfGame *games, int32_t count, int32_t tile_cap, int32_t unit_cap, const EfTables *tables, const int32_t *viewers, EfView *views,
                  int32_t *results) {
    if (count <= 0) return 0;
    EfCaps caps = {tile_cap, unit_cap};
    if (!ef_caps_ok(caps)) return (int)cudaErrorInvalidValue;
    size_t stride = (size_t)ef_block_size(tile_cap, unit_cap), view_size = (size_t)ef_view_size(tile_cap);
    EfGame *d_games = NULL;
    EfTables *d_tables = NULL;
    int32_t *d_viewers = NULL;
    EfView *d_views = NULL;
    EfResult *d_results = NULL;
    size_t n = (size_t)count;
    cudaError_t e = cudaMalloc(&d_games, n * stride);
    if (e == cudaSuccess) e = cudaMalloc(&d_tables, sizeof(EfTables));
    if (e == cudaSuccess) e = cudaMalloc(&d_viewers, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMalloc(&d_views, n * view_size);
    if (e == cudaSuccess) e = cudaMalloc(&d_results, n * sizeof(EfResult));
    if (e == cudaSuccess) e = cudaMemcpy(d_games, games, n * stride, cudaMemcpyHostToDevice);
    void *d_scratch = NULL;
    if (e == cudaSuccess) e = ef_put_tables(d_tables, tables, count, &d_scratch);
    if (e == cudaSuccess) e = cudaMemcpy(d_viewers, viewers, n * sizeof(int32_t), cudaMemcpyHostToDevice);
    if (e == cudaSuccess) e = cudaMemset(d_views, 0, n * view_size);
    if (e == cudaSuccess) {
        k_views<<<(unsigned)((count + 31) / 32), 32>>>(d_games, caps, count, d_tables, d_viewers, d_views, d_results);
        e = cudaGetLastError();
    }
    if (e == cudaSuccess) e = cudaMemcpy(views, d_views, n * view_size, cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = cudaMemcpy(results, d_results, n * sizeof(EfResult), cudaMemcpyDeviceToHost);
    cudaFree(d_games);
    cudaFree(d_tables);
    cudaFree(d_scratch);
    cudaFree(d_viewers);
    cudaFree(d_views);
    cudaFree(d_results);
    return (int)e;
}

/* ---- a batch that stays on the device ---- */
struct EfDevice {
    int32_t games;
    EfCaps caps;                  /* every slot's capacities */
    size_t stride, view_size;     /* the bytes of each slot's block and view */
    EfGame *d_games;
    EfTables *d_tables;
    int32_t *d_viewers;
    EfView *d_views;
    EfResult *d_results;
    EfResult *d_checks;   /* per slot: ef_game_check of the block last uploaded */
    EfMove *d_moves;
    int32_t *d_elapsed;
    /* the batch's slots (status.h) */
    EfMenuWork *d_work;
    EfMenu *d_menus;
    int32_t *d_index, *d_status, *d_winners, *d_counts;
    int64_t *d_offsets;
    EfRow *d_rows;
    void *d_scratch;              /* the searches' scratch, one entry per slot */
    int64_t rows_capacity;
    /* the device loop: exported arrays (never reallocated while the batch lives), the applied
     * codes of the last device step, its status word, and the bytes copied to and from the host
     * by every call on this batch (ef_copy) */
    EfOut out;
    int32_t *d_applied;
    uint32_t *d_word;
    uint64_t to_device, to_host;
    /* the device step's stream (blocking, so the default stream's work on this batch orders
     * before and after it), the event of its last launch, the pinned host copy of its status
     * word, and whether a launch is waiting for ef_cuda_step_wait */
    cudaStream_t stream;
    cudaEvent_t stepped;
    uint32_t *h_word;
    int32_t in_flight;
    /* the staging area of an upload (blocks, then their slots), grown as needed */
    void *d_staging;
    size_t staging_size;
};

/* Every host copy a batch makes goes through here and is counted: the device loop's copy guard
 * (tests/device_loop.rs; tests/copies.rs fails on any other cudaMemcpy below this point). */
static cudaError_t ef_copy(EfDevice *d, void *dst, const void *src, size_t bytes, cudaMemcpyKind kind) {
    if (kind == cudaMemcpyHostToDevice) d->to_device += bytes;
    if (kind == cudaMemcpyDeviceToHost) d->to_host += bytes;
    return cudaMemcpy(dst, src, bytes, kind);
}

/* ef_copy queued on `stream` */
static cudaError_t ef_copy_async(EfDevice *d, void *dst, const void *src, size_t bytes, cudaMemcpyKind kind, cudaStream_t stream) {
    if (kind == cudaMemcpyHostToDevice) d->to_device += bytes;
    if (kind == cudaMemcpyDeviceToHost) d->to_host += bytes;
    return cudaMemcpyAsync(dst, src, bytes, kind, stream);
}

void ef_cuda_destroy(EfDevice *d) {
    if (!d) return;
    if (d->in_flight) cudaEventSynchronize(d->stepped);   /* a launched step still uses the arrays */
    cudaFree(d->d_games);
    cudaFree(d->d_tables);
    cudaFree(d->d_scratch);
    cudaFree(d->d_staging);
    cudaFree(d->d_viewers);
    cudaFree(d->d_views);
    cudaFree(d->d_results);
    cudaFree(d->d_checks);
    cudaFree(d->d_moves);
    cudaFree(d->d_elapsed);
    cudaFree(d->d_work);
    cudaFree(d->d_menus);
    cudaFree(d->d_index);
    cudaFree(d->d_status);
    cudaFree(d->d_winners);
    cudaFree(d->d_counts);
    cudaFree(d->d_offsets);
    cudaFree(d->d_rows);
    cudaFree(d->out.scalars);
    cudaFree(d->out.players);
    cudaFree(d->out.tech);
    cudaFree(d->out.previews);
    cudaFree(d->out.defence);
    cudaFree(d->d_applied);
    cudaFree(d->d_word);
    if (d->stepped) cudaEventDestroy(d->stepped);
    if (d->stream) cudaStreamDestroy(d->stream);
    cudaFreeHost(d->h_word);
    free(d);
}

/* Device memory for `games` slots whose blocks hold `tile_cap` tiles and `unit_cap` units, with
 * room for `techs` technology ids in the exported arrays; NULL with the CUDA error code in
 * *error when it fails. */
EfDevice *ef_cuda_create(int32_t games, int32_t tile_cap, int32_t unit_cap, int32_t techs, int32_t outputs, int32_t *error) {
    EfCaps caps = {tile_cap, unit_cap};
    if (games <= 0 || !ef_caps_ok(caps) || techs < 0 || techs > 4096 || (outputs & ~(EF_WANT_PREVIEWS | EF_WANT_DEFENCE))) {
        *error = (int)cudaErrorInvalidValue;
        return NULL;
    }
    size_t stride = (size_t)ef_block_size(tile_cap, unit_cap);
    EfDevice *d = (EfDevice *)calloc(1, sizeof(EfDevice));
    if (!d) { *error = (int)cudaErrorMemoryAllocation; return NULL; }
    size_t n = (size_t)games;
    d->games = games;
    d->caps = caps;
    d->stride = stride;
    d->view_size = (size_t)ef_view_size(tile_cap);
    d->out.techs = techs;
    d->out.cells = tile_cap;
    cudaError_t e = cudaMalloc(&d->d_games, n * stride);
    if (e == cudaSuccess) e = cudaMalloc(&d->out.scalars, n * EF_OUT_SCALARS * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->out.players, n * EF_OUT_PLAYERS * EF_OUT_PLAYER_FIELDS * sizeof(int64_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->out.tech, n * (size_t)(techs > 0 ? techs : 1) * sizeof(int16_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_applied, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_word, 2 * sizeof(uint32_t));
    if (e == cudaSuccess) e = cudaMallocHost((void **)&d->h_word, 2 * sizeof(uint32_t));
    if (e == cudaSuccess) e = cudaStreamCreate(&d->stream);
    if (e == cudaSuccess) e = cudaEventCreateWithFlags(&d->stepped, cudaEventDisableTiming);
    /* all bits set: -1 in every exported integer of a slot not yet run */
    if (e == cudaSuccess) e = cudaMemset(d->out.scalars, 0xFF, n * EF_OUT_SCALARS * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMemset(d->out.players, 0xFF, n * EF_OUT_PLAYERS * EF_OUT_PLAYER_FIELDS * sizeof(int64_t));
    if (e == cudaSuccess) e = cudaMemset(d->out.tech, 0xFF, n * (size_t)(techs > 0 ? techs : 1) * sizeof(int16_t));
    if (e == cudaSuccess) e = cudaMemset(d->d_applied, 0, n * sizeof(int32_t));
    if (e == cudaSuccess && (outputs & EF_WANT_PREVIEWS)) {
        e = cudaMalloc((void **)&d->out.previews, n * EF_MAX_ROWS * sizeof(int16_t[4]));
        if (e == cudaSuccess) e = cudaMemset(d->out.previews, 0, n * EF_MAX_ROWS * sizeof(int16_t[4]));
    }
    if (e == cudaSuccess && (outputs & EF_WANT_DEFENCE)) {
        e = cudaMalloc((void **)&d->out.defence, n * (size_t)tile_cap * sizeof(int16_t));
        if (e == cudaSuccess) e = cudaMemset(d->out.defence, 0xFF, n * (size_t)tile_cap * sizeof(int16_t));
    }
    if (e == cudaSuccess) e = cudaMalloc(&d->d_tables, sizeof(EfTables));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_viewers, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_views, n * d->view_size);
    if (e == cudaSuccess) e = cudaMalloc(&d->d_results, n * sizeof(EfResult));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_checks, n * sizeof(EfResult));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_moves, n * sizeof(EfMove));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_elapsed, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_work, n * sizeof(EfMenuWork));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_menus, n * sizeof(EfMenu));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_index, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_status, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_winners, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_counts, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMalloc(&d->d_offsets, n * sizeof(int64_t));
    if (e == cudaSuccess) e = cudaMemset(d->d_status, 0, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMemset(d->d_counts, 0, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMemset(d->d_moves, 0, n * sizeof(EfMove));
    if (e == cudaSuccess) e = cudaMemset(d->d_games, 0, n * stride);
    if (e == cudaSuccess) e = cudaMemset(d->d_views, 0xFF, n * d->view_size);
    if (e == cudaSuccess) e = cudaMemset(d->d_menus, 0, n * sizeof(EfMenu));
    /* an empty slot holds a zero block, which fails the check, so it has a result too */
    if (e == cudaSuccess) {
        k_check<<<(unsigned)((games + 31) / 32), 32>>>(d->d_games, d->caps, 0, games, d->d_checks);
        e = cudaGetLastError();
    }
    *error = (int)e;
    if (e != cudaSuccess) { ef_cuda_destroy(d); return NULL; }
    return d;
}

int ef_cuda_set_tables(EfDevice *d, const EfTables *tables) {
    d->to_device += sizeof(EfTables);   /* ef_put_tables' copy, counted here */
    /* the old scratch stays until the tables point at the new one */
    void *scratch = NULL;
    cudaError_t e = ef_put_tables(d->d_tables, tables, d->games, &scratch);
    if (e != cudaSuccess) { cudaFree(scratch); return (int)e; }
    cudaFree(d->d_scratch);
    d->d_scratch = scratch;
    return 0;
}

/* Copy `count` blocks (contiguous in `games`) into slots `slots` (distinct, host memory), and
 * check them there: one copy of the blocks and one of the slot list, whatever the count, and
 * one launch each to place and to check them. */
int ef_cuda_upload(EfDevice *d, int32_t count, const int32_t *slots, const EfGame *games) {
    if (count < 0 || count > d->games) return (int)cudaErrorInvalidValue;
    for (int32_t k = 0; k < count; k++)
        if (slots[k] < 0 || slots[k] >= d->games) return (int)cudaErrorInvalidValue;
    if (count == 0) return 0;
    /* the exported arrays change below: other users of the device (a policy on another stream) finish first */
    cudaError_t e = cudaDeviceSynchronize();
    if (e != cudaSuccess) return (int)e;
    size_t blocks = (size_t)count * d->stride, need = blocks + (size_t)count * sizeof(int32_t);
    if (need > d->staging_size) {
        cudaFree(d->d_staging);
        d->d_staging = NULL;
        d->staging_size = 0;
        e = cudaMalloc(&d->d_staging, need);
        if (e != cudaSuccess) return (int)e;
        d->staging_size = need;
    }
    int32_t *d_slots = (int32_t *)((char *)d->d_staging + blocks);
    e = ef_copy(d, d->d_staging, games, blocks, cudaMemcpyHostToDevice);
    if (e == cudaSuccess) e = ef_copy(d, d_slots, slots, (size_t)count * sizeof(int32_t), cudaMemcpyHostToDevice);
    if (e != cudaSuccess) return (int)e;
    k_place<<<(unsigned)count, 256>>>(d->d_games, d->stride, (const uint64_t *)d->d_staging, d_slots, count);
    k_check_slots<<<(unsigned)((count + 31) / 32), 32>>>(d->d_games, d->caps, d_slots, count, d->d_checks);
    return (int)cudaGetLastError();
}

/* Write every slot's view for its viewer, with `threads` threads per block, and wait for it;
 * the views and results stay on the device. */
int ef_cuda_run_views(EfDevice *d, const int32_t *viewers, int32_t threads) {
    if (threads <= 0 || threads > 1024) return (int)cudaErrorInvalidValue;
    if (d->games == 0) return 0;
    cudaError_t e = ef_copy(d, d->d_viewers, viewers, (size_t)d->games * sizeof(int32_t), cudaMemcpyHostToDevice);
    if (e != cudaSuccess) return (int)e;
    k_checked_views<<<(unsigned)((d->games + threads - 1) / threads), (unsigned)threads>>>(
        d->d_games, d->caps, d->games, d->d_tables, d->d_viewers, d->d_checks, d->d_views, d->d_results);
    e = cudaGetLastError();
    if (e == cudaSuccess) e = cudaDeviceSynchronize();
    return (int)e;
}

/* Copy the views and the results (code and argument per slot) of the last run to the host. */
int ef_cuda_read_views(EfDevice *d, EfView *views, int32_t *results) {
    cudaError_t e = ef_copy(d, views, d->d_views, (size_t)d->games * d->view_size, cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = ef_copy(d, results, d->d_results, (size_t)d->games * sizeof(EfResult), cudaMemcpyDeviceToHost);
    return (int)e;
}

/* free and total device memory in bytes */
int ef_cuda_memory(uint64_t *free_bytes, uint64_t *total_bytes) {
    size_t f = 0, t = 0;
    cudaError_t e = cudaMemGetInfo(&f, &t);
    *free_bytes = f;
    *total_bytes = t;
    return (int)e;
}

/* Run one decision in each of `count` blocks on the device: upload the blocks, the tables and
 * one move per block, run, and download the blocks, results and elapsed flags. */
int ef_cuda_advance(EfGame *games, int32_t count, int32_t tile_cap, int32_t unit_cap, const EfTables *tables, const EfMove *moves, EfResult *results,
                    int32_t *elapsed) {
    if (count <= 0) return 0;
    EfCaps caps = {tile_cap, unit_cap};
    if (!ef_caps_ok(caps)) return (int)cudaErrorInvalidValue;
    size_t stride = (size_t)ef_block_size(tile_cap, unit_cap);
    EfGame *d_games = NULL;
    EfTables *d_tables = NULL;
    EfMove *d_moves = NULL;
    EfResult *d_results = NULL;
    int32_t *d_elapsed = NULL;
    size_t n = (size_t)count;
    cudaError_t e = cudaMalloc(&d_games, n * stride);
    if (e == cudaSuccess) e = cudaMalloc(&d_tables, sizeof(EfTables));
    if (e == cudaSuccess) e = cudaMalloc(&d_moves, n * sizeof(EfMove));
    if (e == cudaSuccess) e = cudaMalloc(&d_results, n * sizeof(EfResult));
    if (e == cudaSuccess) e = cudaMalloc(&d_elapsed, n * sizeof(int32_t));
    if (e == cudaSuccess) e = cudaMemcpy(d_games, games, n * stride, cudaMemcpyHostToDevice);
    void *d_scratch = NULL;
    if (e == cudaSuccess) e = ef_put_tables(d_tables, tables, count, &d_scratch);
    if (e == cudaSuccess) e = cudaMemcpy(d_moves, moves, n * sizeof(EfMove), cudaMemcpyHostToDevice);
    if (e == cudaSuccess) e = cudaMemset(d_elapsed, 0, n * sizeof(int32_t));
    if (e == cudaSuccess) {
        k_advance<<<(unsigned)((count + 31) / 32), 32>>>(d_games, caps, count, d_tables, d_moves, d_results, d_elapsed);
        e = cudaGetLastError();
    }
    if (e == cudaSuccess) e = cudaMemcpy(games, d_games, n * stride, cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = cudaMemcpy(results, d_results, n * sizeof(EfResult), cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = cudaMemcpy(elapsed, d_elapsed, n * sizeof(int32_t), cudaMemcpyDeviceToHost);
    cudaFree(d_games);
    cudaFree(d_tables);
    cudaFree(d_scratch);
    cudaFree(d_moves);
    cudaFree(d_results);
    cudaFree(d_elapsed);
    return (int)e;
}

/* the legal list of each of `count` blocks: menus[k] and results[k] per block */
int ef_cuda_menus(const EfGame *games, int32_t count, int32_t tile_cap, int32_t unit_cap, const EfTables *tables, EfMenu *menus, EfResult *results) {
    if (count <= 0) return 0;
    EfCaps caps = {tile_cap, unit_cap};
    if (!ef_caps_ok(caps)) return (int)cudaErrorInvalidValue;
    size_t stride = (size_t)ef_block_size(tile_cap, unit_cap);
    EfGame *d_games = NULL;
    EfTables *d_tables = NULL;
    EfMenuWork *d_work = NULL;
    EfMenu *d_menus = NULL;
    EfResult *d_results = NULL;
    size_t n = (size_t)count;
    cudaError_t e = cudaMalloc(&d_games, n * stride);
    if (e == cudaSuccess) e = cudaMalloc(&d_tables, sizeof(EfTables));
    if (e == cudaSuccess) e = cudaMalloc(&d_work, n * sizeof(EfMenuWork));
    if (e == cudaSuccess) e = cudaMalloc(&d_menus, n * sizeof(EfMenu));
    if (e == cudaSuccess) e = cudaMalloc(&d_results, n * sizeof(EfResult));
    if (e == cudaSuccess) e = cudaMemcpy(d_games, games, n * stride, cudaMemcpyHostToDevice);
    void *d_scratch = NULL;
    if (e == cudaSuccess) e = ef_put_tables(d_tables, tables, count, &d_scratch);
    if (e == cudaSuccess) {
        k_menus<<<(unsigned)((count + 31) / 32), 32>>>(d_games, caps, count, d_tables, d_work, d_menus, d_results);
        e = cudaGetLastError();
    }
    if (e == cudaSuccess) e = cudaMemcpy(menus, d_menus, n * sizeof(EfMenu), cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = cudaMemcpy(results, d_results, n * sizeof(EfResult), cudaMemcpyDeviceToHost);
    cudaFree(d_games);
    cudaFree(d_tables);
    cudaFree(d_scratch);
    cudaFree(d_work);
    cudaFree(d_menus);
    cudaFree(d_results);
    return (int)e;
}

/* the persistent batch: one decision in every slot still playing, the moves set by
 * ef_cuda_set_moves; the blocks stay on the device */
int ef_cuda_run_advance(EfDevice *d, int32_t threads) {
    if (threads <= 0 || threads > 1024) return (int)cudaErrorInvalidValue;
    if (d->games == 0) return 0;
    k_checked_advance<<<(unsigned)((d->games + threads - 1) / threads), (unsigned)threads>>>(
        d->d_games, d->caps, d->games, d->d_tables, d->d_moves, d->d_checks, d->d_results, d->d_elapsed);
    cudaError_t e = cudaGetLastError();
    if (e == cudaSuccess) e = cudaDeviceSynchronize();
    return (int)e;
}

int ef_cuda_set_moves(EfDevice *d, const EfMove *moves) {
    return (int)ef_copy(d, d->d_moves, moves, (size_t)d->games * sizeof(EfMove), cudaMemcpyHostToDevice);
}

/* the results of the last run, one per slot */
int ef_cuda_read_results(EfDevice *d, EfResult *results) {
    return (int)ef_copy(d, results, d->d_results, (size_t)d->games * sizeof(EfResult), cudaMemcpyDeviceToHost);
}

/* one slot's block back to the host */
int ef_cuda_download(EfDevice *d, int32_t slot, EfGame *game) {
    if (slot < 0 || slot >= d->games) return (int)cudaErrorInvalidValue;
    return (int)ef_copy(d, game, (char *)d->d_games + (size_t)slot * d->stride, d->stride, cudaMemcpyDeviceToHost);
}

/* Run every slot by `index` (see k_slots) and wait. */
int ef_cuda_run_slots(EfDevice *d, const int32_t *index, int32_t threads) {
    if (threads <= 0 || threads > 1024) return (int)cudaErrorInvalidValue;
    cudaError_t w = cudaDeviceSynchronize();   /* as ef_cuda_upload */
    if (w != cudaSuccess) return (int)w;
    cudaError_t e = ef_copy(d, d->d_index, index, (size_t)d->games * sizeof(int32_t), cudaMemcpyHostToDevice);
    if (e != cudaSuccess) return (int)e;
    k_slots<<<(unsigned)((d->games + threads - 1) / threads), (unsigned)threads>>>(
        d->d_games, d->caps, d->games, d->d_tables, d->d_checks, d->d_work, d->d_menus, d->d_views, d->d_index, d->d_status,
        d->d_winners, d->d_results, d->d_counts, d->out);
    e = cudaGetLastError();
    if (e == cudaSuccess) e = cudaDeviceSynchronize();
    return (int)e;
}

/* per slot: status, winner, result of the last run, row count */
int ef_cuda_read_slots(EfDevice *d, int32_t *status, int32_t *winners, EfResult *results, int32_t *counts) {
    size_t n = (size_t)d->games;
    cudaError_t e = ef_copy(d, status, d->d_status, n * sizeof(int32_t), cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = ef_copy(d, winners, d->d_winners, n * sizeof(int32_t), cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = ef_copy(d, results, d->d_results, n * sizeof(EfResult), cudaMemcpyDeviceToHost);
    if (e == cudaSuccess) e = ef_copy(d, counts, d->d_counts, n * sizeof(int32_t), cudaMemcpyDeviceToHost);
    return (int)e;
}

/* every playing slot's rows, packed by `offsets` (one per slot, from the counts), `total` rows */
int ef_cuda_read_rows(EfDevice *d, const int64_t *offsets, int64_t total, EfRow *rows, int32_t threads) {
    if (threads <= 0 || threads > 1024 || total < 0) return (int)cudaErrorInvalidValue;
    cudaError_t e = cudaSuccess;
    if (total > d->rows_capacity) {
        cudaFree(d->d_rows);
        d->d_rows = NULL;
        d->rows_capacity = 0;
        e = cudaMalloc(&d->d_rows, (size_t)(total > 0 ? total : 1) * sizeof(EfRow));
        if (e != cudaSuccess) return (int)e;
        d->rows_capacity = total;
    }
    e = ef_copy(d, d->d_offsets, offsets, (size_t)d->games * sizeof(int64_t), cudaMemcpyHostToDevice);
    if (e != cudaSuccess) return (int)e;
    k_gather_rows<<<(unsigned)((d->games + threads - 1) / threads), (unsigned)threads>>>(d->d_menus, d->games, d->d_offsets,
                                                                                       d->d_counts, total, d->d_rows);
    e = cudaGetLastError();
    if (e == cudaSuccess && total > 0) e = ef_copy(d, rows, d->d_rows, (size_t)total * sizeof(EfRow), cudaMemcpyDeviceToHost);
    return (int)e;
}

/* the name of device 0 into `name`; 0, or the CUDA error code */
int ef_cuda_device_name(char *name, int32_t capacity) {
    cudaDeviceProp p;
    cudaError_t e = cudaGetDeviceProperties(&p, 0);
    if (e == cudaSuccess && capacity > 0) {
        strncpy(name, p.name, (size_t)capacity - 1);
        name[capacity - 1] = 0;
    }
    return (int)e;
}

/* ---- the device loop: outputs and actions stay in device memory ---- */

/* The device addresses and layouts of a batch's exported arrays. Rows and tile planes are read
 * in place: slot g's rows from rows + g * menu_stride (len counts[g], six int32 each), its planes
 * from tiles + g * view_stride (EF_VIEW_PLANES planes of `cells` int16 each). */
typedef struct {
    uint64_t status, winners, counts, applied, rows, tiles, scalars, players, tech;
    uint64_t previews, defence;   /* 0 unless the batch was made with them */
    uint64_t menu_stride, view_stride;
    int32_t max_rows, planes, cells, techs;
} EfExports;

int ef_cuda_exports(EfDevice *d, EfExports *x) {
    x->status = (uint64_t)(uintptr_t)d->d_status;
    x->winners = (uint64_t)(uintptr_t)d->d_winners;
    x->counts = (uint64_t)(uintptr_t)d->d_counts;
    x->applied = (uint64_t)(uintptr_t)d->d_applied;
    x->rows = (uint64_t)(uintptr_t)((char *)d->d_menus + offsetof(EfMenu, rows));
    x->tiles = (uint64_t)(uintptr_t)((char *)d->d_views + sizeof(EfView));
    x->scalars = (uint64_t)(uintptr_t)d->out.scalars;
    x->players = (uint64_t)(uintptr_t)d->out.players;
    x->tech = (uint64_t)(uintptr_t)d->out.tech;
    x->previews = (uint64_t)(uintptr_t)d->out.previews;
    x->defence = (uint64_t)(uintptr_t)d->out.defence;
    x->menu_stride = (uint64_t)sizeof(EfMenu);
    x->view_stride = (uint64_t)d->view_size;
    x->max_rows = EF_MAX_ROWS;
    x->planes = EF_VIEW_PLANES;
    x->cells = d->out.cells;
    x->techs = d->out.techs;
    return 0;
}

/* Whether `ptr` is memory on device 0 that the kernels may read (a policy's output). */
int ef_cuda_on_device(uint64_t ptr) {
    cudaPointerAttributes a;
    if (cudaPointerGetAttributes(&a, (const void *)(uintptr_t)ptr) != cudaSuccess) { cudaGetLastError(); return 0; }
    return (a.type == cudaMemoryTypeDevice || a.type == cudaMemoryTypeManaged) && a.device == 0;
}

/* One decision per slot from `actions`, int32 per slot in device memory (k_device_step), queued
 * on the batch's stream with the copy of its status word, without waiting: ef_cuda_step_wait
 * returns the word. The batch's earlier work on the default stream runs first (a blocking
 * stream); `actions` must be complete by then, written on the default stream or this batch's
 * stream (ef_cuda_random_actions) or waited for by the caller. One launch at a time. Only
 * work on non-blocking streams runs beside it: the default stream's waits for it. */
int ef_cuda_step_launch(EfDevice *d, uint64_t actions, int32_t threads) {
    if (threads <= 0 || threads > 1024 || d->in_flight) return (int)cudaErrorInvalidValue;
    cudaError_t e = cudaMemsetAsync(d->d_word, 0, 2 * sizeof(uint32_t), d->stream);
    if (e != cudaSuccess) return (int)e;
    k_device_step<<<(unsigned)((d->games + threads - 1) / threads), (unsigned)threads, 0, d->stream>>>(
        d->d_games, d->caps, d->games, d->d_tables, d->d_checks, d->d_work, d->d_menus, d->d_views,
        (const int32_t *)(uintptr_t)actions, d->d_applied, d->d_status, d->d_winners, d->d_results, d->d_counts, d->out,
        d->d_word);
    e = cudaGetLastError();
    if (e == cudaSuccess) e = ef_copy_async(d, d->h_word, d->d_word, 2 * sizeof(uint32_t), cudaMemcpyDeviceToHost, d->stream);
    if (e == cudaSuccess) e = cudaEventRecord(d->stepped, d->stream);
    if (e == cudaSuccess) d->in_flight = 1;
    return (int)e;
}

/* Wait for the last ef_cuda_step_launch; word[0] the slots not playing, word[1] those that
 * stopped in that call */
int ef_cuda_step_wait(EfDevice *d, uint32_t *word) {
    if (!d->in_flight) return (int)cudaErrorInvalidValue;
    cudaError_t e = cudaEventSynchronize(d->stepped);
    d->in_flight = 0;
    if (e == cudaSuccess) { word[0] = d->h_word[0]; word[1] = d->h_word[1]; }
    return (int)e;
}

/* ef_cuda_step_launch and its wait, after the work of every other user of the device (a policy
 * on another stream) */
int ef_cuda_step_device(EfDevice *d, uint64_t actions, int32_t threads, uint32_t *word) {
    cudaError_t e = cudaDeviceSynchronize();
    if (e != cudaSuccess) return (int)e;
    int r = ef_cuda_step_launch(d, actions, threads);
    return r != 0 ? r : ef_cuda_step_wait(d, word);
}

/* The benchmark's policy into `actions` (device memory, int32 per slot): k_random_actions, on
 * the batch's stream, so a step launched after it reads it */
int ef_cuda_random_actions(EfDevice *d, uint64_t seed, uint64_t actions, int32_t threads) {
    if (threads <= 0 || threads > 1024) return (int)cudaErrorInvalidValue;
    k_random_actions<<<(unsigned)((d->games + threads - 1) / threads), (unsigned)threads, 0, d->stream>>>(
        d->d_menus, d->d_status, d->d_counts, d->games, seed, (int32_t *)(uintptr_t)actions);
    return (int)cudaGetLastError();
}

/* ---- benchmark only: a stand-in for a policy's forward pass (loop_bench's overlap mode) ---- */

/* c = a b for `rows` rows of EF_LOAD_K floats against a EF_LOAD_K x EF_LOAD_K matrix, in
 * 16 x 16 tiles */
#define EF_LOAD_K 256
__global__ void k_load(const float *a, const float *b, float *c, int rows) {
    __shared__ float ta[16][16], tb[16][16];
    int r = blockIdx.y * 16 + threadIdx.y, col = blockIdx.x * 16 + threadIdx.x;
    float sum = 0.0f;
    for (int k0 = 0; k0 < EF_LOAD_K; k0 += 16) {
        ta[threadIdx.y][threadIdx.x] = r < rows ? a[(size_t)r * EF_LOAD_K + k0 + threadIdx.x] : 0.0f;
        tb[threadIdx.y][threadIdx.x] = b[(size_t)(k0 + threadIdx.y) * EF_LOAD_K + col];
        __syncthreads();
        for (int k = 0; k < 16; k++) sum += ta[threadIdx.y][k] * tb[k][threadIdx.x];
        __syncthreads();
    }
    if (r < rows) c[(size_t)r * EF_LOAD_K + col] = sum;
}

static cudaStream_t ef_load_stream;
static float *ef_load_a, *ef_load_b, *ef_load_c;
static int32_t ef_load_rows;

/* Queue `repeats` products of `rows` rows on the load stream (non-blocking, so it runs beside
 * the batches' streams); ef_cuda_load_wait waits for them */
int ef_cuda_load(int32_t rows, int32_t repeats) {
    if (rows <= 0 || repeats < 0) return (int)cudaErrorInvalidValue;
    cudaError_t e = cudaSuccess;
    if (!ef_load_stream) e = cudaStreamCreateWithFlags(&ef_load_stream, cudaStreamNonBlocking);
    if (e == cudaSuccess && rows > ef_load_rows) {
        cudaFree(ef_load_a); cudaFree(ef_load_b); cudaFree(ef_load_c);
        e = cudaMalloc((void **)&ef_load_a, (size_t)rows * EF_LOAD_K * sizeof(float));
        if (e == cudaSuccess) e = cudaMalloc((void **)&ef_load_b, (size_t)EF_LOAD_K * EF_LOAD_K * sizeof(float));
        if (e == cudaSuccess) e = cudaMalloc((void **)&ef_load_c, (size_t)rows * EF_LOAD_K * sizeof(float));
        if (e == cudaSuccess) e = cudaMemsetAsync(ef_load_a, 0, (size_t)rows * EF_LOAD_K * sizeof(float), ef_load_stream);
        if (e == cudaSuccess) e = cudaMemsetAsync(ef_load_b, 0, (size_t)EF_LOAD_K * EF_LOAD_K * sizeof(float), ef_load_stream);
        ef_load_rows = e == cudaSuccess ? rows : 0;
    }
    dim3 grid(EF_LOAD_K / 16, (unsigned)((rows + 15) / 16)), block(16, 16);
    for (int32_t k = 0; e == cudaSuccess && k < repeats; k++) {
        k_load<<<grid, block, 0, ef_load_stream>>>(ef_load_a, ef_load_b, ef_load_c, rows);
        e = cudaGetLastError();
    }
    return (int)e;
}

int ef_cuda_load_wait(void) {
    return ef_load_stream ? (int)cudaStreamSynchronize(ef_load_stream) : 0;
}

/* `bytes` from device address `from` (one of the batch's exported arrays) to the host, counted */
int ef_cuda_read(EfDevice *d, uint64_t from, uint64_t bytes, void *to) {
    return (int)ef_copy(d, to, (const void *)(uintptr_t)from, (size_t)bytes, cudaMemcpyDeviceToHost);
}

void ef_cuda_traffic(EfDevice *d, uint64_t *to_device, uint64_t *to_host) {
    *to_device = d->to_device;
    *to_host = d->to_host;
}

/* For tests and benchmarks, outside any batch's count: device memory, and copies of it. */
int ef_cuda_alloc(uint64_t bytes, uint64_t *ptr) {
    void *p = NULL;
    cudaError_t e = cudaMalloc(&p, (size_t)bytes);
    *ptr = (uint64_t)(uintptr_t)p;
    return (int)e;
}
void ef_cuda_free(uint64_t ptr) { cudaFree((void *)(uintptr_t)ptr); }
int ef_cuda_peek(uint64_t from, uint64_t bytes, void *to) {
    return (int)cudaMemcpy(to, (const void *)(uintptr_t)from, (size_t)bytes, cudaMemcpyDeviceToHost);
}
int ef_cuda_poke(uint64_t to, uint64_t bytes, const void *from) {
    return (int)cudaMemcpy((void *)(uintptr_t)to, from, (size_t)bytes, cudaMemcpyHostToDevice);
}

}
