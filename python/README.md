# engine_fast (Python package)

A fast rules engine for The Battle of Polytopia that plays many games per call. It hands out
what the player to move may know (the *view*) and that player's *legal moves*, and takes one
move per game. It prepares no model input: scaling, memory of earlier views and the layout a
network wants are the caller's.

## Install

With a Rust toolchain on the path, from this folder:

    pip install .

pip builds the extension with maturin. Python 3.11 or later and numpy 2.1 or later. For the
GPU backend, build with `maturin build --release --features cuda` on a machine with CUDA 12
and its nvcc (`EF_CUDA_ARCH` picks the GPU architecture, default `sm_86`); the device code
takes several minutes to compile.

The package needs the game's rule data, read from your installed game: see the main README
(`tools/extract_rules.py`, then `ENGINE_FAST_RULES`).

## Use

    import numpy as np
    import engine_fast

    batch = engine_fast.Batch(games=256, size=11, workers=8)
    batch.reset(np.arange(256), seeds=np.arange(1, 257), tribes=[7, 7])
    while True:
        status = batch.status()                  # uint8 per slot, see engine_fast.STATUS
        offsets, moves = batch.moves()           # slot i: moves[offsets[i]:offsets[i + 1]]
        tiles, scalars, players, tech = batch.views()
        choice = np.full(len(batch), engine_fast.SKIP, dtype=np.int32)
        playing = status == 1
        if not playing.any():
            break
        choice[playing] = 0                      # your policy: an index into the slot's moves
        batch.step(choice)

- `Batch(games, size=11, workers=1, backend="cpu", outputs=(), rules=None)`: `games` slots
  that share one board side (11 to 30, or 8 for nano). With one worker the games are played
  on the calling thread. `rules` is the rule data's path (default: `ENGINE_FAST_RULES`).
- `reset(slots, seeds, tribes, map=2, mode=4, action_limit=1000)`: starts a game in each
  slot; a slot can be reset at any time. `tribes` has one tribe id (2 to 17) per seat, `map`
  is 1 to 6 and `action_limit` 1 to 10 000; the whole call uses one configuration. A
  configuration the engine does not take refuses those slots (status 4) rather than raising.
- `step(moves)`: per slot an index into its legal moves, or `SKIP`. Returns per slot 0 (no
  move), 1 (taken) or 2 (rejected: nothing changed). After 1, read the slot's status: it is
  refused or failed if the engine could not carry the move out. A move is always given by
  index, so nothing outside the legal list can be applied.
- `status()`: 0 empty, 1 playing, 2 ended, 3 truncated (the decision limit), 4 refused,
  5 failed. `winner()` gives the winner's player id of an ended game; `reason(slot)` says why
  a slot was refused or failed.
- `moves()`: `(offsets, moves)`. Columns are `MOVE_FIELDS`: kind, source tile, target tile,
  option, opponent, price; -1 where a field does not apply. `KINDS` names the kinds. A slot's
  moves are sorted by the first five, which identify the move.
- `views()`: `(tiles, scalars, players, tech)` for the player to move.
  `tiles[slot, plane, y * size + x]` with planes named in `TILE_PLANES`; `scalars` columns
  are `SCALAR_FIELDS`; `players` rows are the viewer and then the players it has met, columns
  `PLAYER_FIELDS`; `tech` lists the viewer's technologies. `seat` is the player's id, the
  same numbers as `winner()` and a move's `opponent`. Rows of a slot that is not playing and
  unused entries are -1; within a playing slot's planes -1 means "not known to this player".

Optional outputs, asked for when the batch is made (`outputs=("previews", "defence")`); a
batch made without one computes nothing for it and refuses its call. Both show what the
game's own screen shows, which can be more than the view:
- `previews()`: int16 `[rows, 4]`, one row per row of `moves()`: present, damage shown on the
  target, damage shown on the attacker (-1: the lethal icon, or no retaliation), flags (1
  kills the target, 2 the attacker dies, 4 retaliation shows). `PREVIEW_FIELDS` names them.
- `defence()`: int16 `[games, size * size]`, the defence bonus in tenths (10: none) of each
  unit with an info panel for the player to move, -1 elsewhere.

The GPU backend (`backend="gpu"`) has the same interface, and also a loop that stays on the
GPU: `device_arrays()` hands out the batch's outputs as CUDA arrays read in place
(`torch.as_tensor(a, device="cuda")`, names in `DEVICE_ARRAYS`), and `step_device(actions)`
takes a policy's int32 CUDA array of one row index per slot. A device step copies 8 bytes
to the host; `traffic()` counts the batch's copies. Restarting finished games (`reset`)
starts them on the CPU and uploads them.

Arguments may be any integer arrays or lists. A batch is used from one thread at a time;
`reset` and `step` release the interpreter lock while they run. Every array is a copy owned
by the caller. A result never depends on the number of workers or on what else is in the
batch. Names of planes and fields are data, so code should look them up rather than
hard-code positions.
