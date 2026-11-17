# engine_fast (Python package)

**Many games of The Battle of Polytopia per call, as numpy arrays.** A `Batch` holds a fixed
number of game slots. Each call to `step` takes one move per game and advances them all. In
between you read each game's status, its legal moves and the view of the player to move.

- Moves are given as an index into the game's list of legal moves, so nothing illegal can be
  applied.
- The engine prepares no model input: scaling, memory of earlier views and the layout a network
  wants are up to you.
- Results never depend on the number of worker threads or on which other games share the batch.

## Contents

1. [Install](#install)
2. [Quick start](#quick-start)
3. [The batch, call by call](#the-batch-call-by-call)
4. [Reading the arrays](#reading-the-arrays)
5. [Optional outputs](#optional-outputs)
6. [The GPU backend](#the-gpu-backend)
7. [Rules of use](#rules-of-use)

## Install

With a Rust toolchain on the path, from this folder:

    pip install .

pip builds the extension with maturin. Python 3.11 or later and numpy 2.1 or later.

The package needs the game's rule data, read from your installed game: see the main README
(`tools/extract_rules.py`, then set `ENGINE_FAST_RULES`).

For the GPU backend, build with `maturin build --release --features cuda` on a machine with
CUDA 12 and its `nvcc`. `EF_CUDA_ARCH` picks the GPU architecture (default `sm_86`). The
device code takes several minutes to compile.

## Quick start

```python
import numpy as np
import engine_fast

batch = engine_fast.Batch(games=256, size=11, workers=8)
batch.reset(np.arange(256), seeds=np.arange(1, 257), tribes=[7, 7])
while True:
    status = batch.status()                  # uint8 per slot, see engine_fast.STATUS
    if not (status == 1).any():              # 1: playing
        break
    offsets, moves = batch.moves()           # slot i: moves[offsets[i]:offsets[i + 1]]
    tiles, scalars, players, tech = batch.views()
    choice = np.full(len(batch), engine_fast.SKIP, dtype=np.int32)
    choice[status == 1] = 0                  # your policy: an index into the slot's moves
    batch.step(choice)
```

## The batch, call by call

**`Batch(games, size=11, workers=1, backend="cpu", outputs=(), rules=None)`**
- `games` slots that share one board side: 11 to 30, or 8 for nano.
- `workers` threads play the games; with one worker they are played on the calling thread.
- `backend` is `"cpu"` or `"gpu"` (see [The GPU backend](#the-gpu-backend)).
- `outputs` names the [optional outputs](#optional-outputs) to compute.
- `rules` is the rule data's path; the default is the `ENGINE_FAST_RULES` variable.

**`reset(slots, seeds, tribes, map=2, mode=4, action_limit=1000)`**
- Starts a new game in each of `slots`, with the matching entry of `seeds`. A slot can be reset
  at any time.
- `tribes` has one tribe id (2 to 17) per seat, two to four seats. `map` is the map preset
  (1 to 6) and `mode` the game mode.
- A game stops as truncated after `action_limit` decisions (1 to 10 000).
- The whole call uses one configuration. A configuration the engine does not take refuses those
  slots (status 4) rather than raising.

**`step(moves)`**
- Per slot, an index into its legal moves, or `SKIP` (-1) to leave the slot as it is.
- Returns per slot 0 (no move given), 1 (taken) or 2 (rejected: the slot is not playing or the
  index is out of range; nothing changed). `APPLIED` names them.
- After 1, read the slot's status: it is refused or failed if the engine could not carry the
  move out.

**`status()`**: per slot 0 empty, 1 playing, 2 ended, 3 truncated (reached `action_limit`),
4 refused, 5 failed. `STATUS` names them.

**`winner()`**: the winner's player id of an ended game, 0 otherwise.

**`reason(slot)`**: why a slot was refused or failed, for example the name of a rule the engine
does not implement.

**`moves()`** and **`views()`**: see [Reading the arrays](#reading-the-arrays).

**`len(batch)`** is the number of slots; **`batch.size`** the board side.

## Reading the arrays

**Legal moves: `offsets, moves = batch.moves()`**
- Slot `i`'s moves are `moves[offsets[i]:offsets[i + 1]]`; a slot not playing has none.
- Columns are `MOVE_FIELDS`: kind, source tile, target tile, option, opponent, price. A field
  that does not apply is -1. `KINDS` names the kinds.
- A tile is `y * size + x`.
- A slot's moves are sorted by their first five columns, which identify the move.

**The view: `tiles, scalars, players, tech = batch.views()`**, for the player to move in each
slot:
- `tiles[slot, plane, y * size + x]`, with the planes named in `TILE_PLANES` (terrain, owner,
  units and so on).
- `scalars[slot]`, columns named in `SCALAR_FIELDS` (the viewer, the turn, stars).
- `players[slot]`: the viewer first, then the players it has met; columns `PLAYER_FIELDS`.
  `seat` is the player's id, the same numbers as `winner()` and a move's `opponent`.
- `tech`: the viewer's technologies.
- -1 marks a row of a slot that is not playing, an unused entry, or, inside a playing slot's
  planes, "not known to this player".

The view holds only what that player may know: unexplored tiles and units it cannot see are
unknown.

## Optional outputs

Asked for when the batch is made, for example `outputs=("previews", "defence")`. A batch made
without one computes nothing for it and refuses its call. `OUTPUTS` lists them. These show what
the game's own screen shows, which can be more than the view: for example a seen enemy's
defence bonus, which depends on that player's technologies.

| Output | Call | Shape | What |
| --- | --- | --- | --- |
| `previews` | `previews()` | int16 `[rows, 4]`, one row per row of `moves()` | an attack's forecast: present, damage shown on the target, damage shown on the attacker (-1: the lethal icon, or no retaliation), flags (1 kills the target, 2 the attacker dies, 4 retaliation shows); `PREVIEW_FIELDS` names them |
| `defence` | `defence()` | int16 `[games, size * size]` | the defence bonus in tenths (10: none) of each unit with an info panel for the player to move, -1 elsewhere |
| `income` | `income()` | int32 `[games]` | the income the player to move is shown; -1 for slots not playing. CPU backend only |
| `max_health` | `max_health()` | int16 `[games, size * size]` | the maximum health a unit's info panel shows (a carrier shows its passenger's), -1 elsewhere. CPU backend only |

## The GPU backend

`Batch(..., backend="gpu")` has the same interface and gives the same results as the CPU
backend. It also has a loop that stays on the GPU:
- **`device_arrays()`** hands out the batch's arrays (statuses, legal moves, views, outputs) as
  CUDA arrays read in place, for example `torch.as_tensor(a, device="cuda")`. `DEVICE_ARRAYS`
  names them. They change with every step.
- **`step_device(actions)`** takes a contiguous int32 CUDA array of one move index per slot (a
  policy's output). It returns `(slots not playing, slots that stopped in this call)`, the only
  data it copies to the host. `device_arrays()["applied"]` says what was done per slot.
- **`traffic()`** counts the bytes the batch copied (to the device, to the host).
- The host calls (`status`, `moves`, `views` and so on) still work, after copying the state back.

Restarting finished games without stalling the loop:
- **`queue(seeds, tribes, map=2, mode=4, action_limit=1000)`** starts games on background CPU
  threads, after any queued before.
- **`restart_finished(wait=True)`** gives every slot that is not playing the next queued game,
  in slot order and queue order, and returns the slots restarted. A refused game's slot is among
  them, already refused. With `wait=False` only games already built are used. A slot restarted
  this way equals `reset` of it with the same configuration. Read the finished games' outcomes
  first.
- **`queue_depth()`** gives `(queued, ready)`: games queued and not used yet, and of those the
  ones already built. Each queued game holds about 68 KB of host memory at 11 x 11 until it is
  used, so keep the queue bounded.

A game that outgrows the GPU's fixed per-game storage (units, queued actions, legal moves)
stops with a reason saying so, where the CPU backend would play on.

## Rules of use

- `slots`, `seeds` and `moves` may be any integer arrays or lists; `tribes` is a list.
- A batch is used from one thread at a time. `reset` and `step` release the interpreter lock
  while they run.
- Every array is a copy owned by the caller (except `device_arrays()`, which are views of device
  memory).
- Names of planes and fields are data: look them up (`TILE_PLANES.index("owner")`) rather than
  hard-coding positions.
