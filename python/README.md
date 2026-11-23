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
6. [The action table](#the-action-table)
7. [The GPU backend](#the-gpu-backend)
8. [Rules of use](#rules-of-use)

## Install

Python 3.11 or later; pip installs numpy 2.1 or later with the package. The repository's
release has built packages:

Windows x86_64:

    pip install https://github.com/polybots-dev/polybots-engine/releases/download/v0.1.0/engine_fast-0.1.0-cp311-abi3-win_amd64.whl

Linux x86_64:

    pip install https://github.com/polybots-dev/polybots-engine/releases/download/v0.1.0/engine_fast-0.1.0-cp311-abi3-manylinux_2_28_x86_64.whl

Linux arm64:

    pip install https://github.com/polybots-dev/polybots-engine/releases/download/v0.1.0/engine_fast-0.1.0-cp311-abi3-manylinux_2_28_aarch64.whl

macOS, Intel:

    pip install https://github.com/polybots-dev/polybots-engine/releases/download/v0.1.0/engine_fast-0.1.0-cp311-abi3-macosx_11_0_x86_64.whl

The repository's `wheels/` folder has two more; from the repository's top folder:

    pip install wheels/macos/engine_fast-0.1.0-cp311-abi3-macosx_11_0_arm64.whl   # macOS, Apple silicon
    pip install wheels/cuda/engine_fast-0.1.0-cp311-abi3-linux_x86_64.whl         # Linux x86_64, CPU and GPU

The release's Linux wheels need glibc 2.28 or later (Ubuntu 22.04: use its `python3.11`
package, since its own Python is 3.10). The CUDA wheel is not a manylinux wheel (built with
glibc 2.39) and runs on GPUs of compute capability 8.6 to 8.9 only, with a driver for CUDA 12.
The main README has the details.

Anywhere else, build from source with a Rust toolchain on the path, from the
repository's top folder:

    pip install ./python

pip builds the extension with maturin.

The package carries the rules (version 124, in the engine's own format), so it runs with
nothing else installed: no copy of the game is needed, also for players who have the game
only on a phone or tablet. It can read the game's own rule data from a desktop copy instead:
see the main README (`tools/extract_rules.py`, then set `ENGINE_FAST_RULES`).

To build the GPU backend from source, run `maturin build --release --features cuda` in
`python/` on a machine with CUDA 12 and its `nvcc`. `EF_CUDA_ARCH` picks the GPU architecture
(default `sm_86`). The device code takes several minutes to compile.

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
- `rules` is the path of a rule file; the default is the file the `ENGINE_FAST_RULES` variable
  names (an empty variable counts as unset), else the rules the package carries. A file with a
  `format` key is read as the engine's own format; any other must be the game's own rule data
  of version 124 (its SHA-256 is checked), or the call raises.
- `batch.rules_sha256` is the SHA-256 of the rule text the batch plays on;
  `engine_fast.RULES_SHA256` is that of the carried file. `engine_fast.RULES` is the rules
  version, 124.
- Raises for a board side outside the range, an unknown output, a CPU-only output with
  `backend="gpu"`, or `"gpu"` in a build without CUDA.

**`reset(slots, seeds, tribes, map=2, mode=4, action_limit=1000)`**
- Starts a new game in each of `slots`, with the matching entry of `seeds`. A slot can be reset
  at any time.
- `tribes` has one tribe id (2 to 17) per seat, two to four seats. `map` is the map preset
  (1 to 6) and `mode` the game mode: 1 Perfection, 2 Domination, 3 Glory, 4 Might.
- A game stops as truncated after `action_limit` decisions (1 to 10 000).
- The whole call uses one configuration. A configuration the engine does not take refuses those
  slots (status 4) rather than raising. It raises if a slot is out of range or named twice,
  `slots` and `seeds` differ in length, or a seed is outside int32.

**`step(moves)`**
- Per slot, an index into its legal moves, or `SKIP` (-1) to leave the slot as it is; one entry
  per slot, or it raises.
- Returns uint8 per slot: 0 (no move given), 1 (taken) or 2 (rejected: the slot is not
  playing or the index is out of range; nothing changed). `APPLIED` names them.
- After 1, read the slot's status: it is refused or failed if the engine could not carry the
  move out.

**`status()`**: per slot 0 empty, 1 playing, 2 ended, 3 truncated (reached `action_limit`),
4 refused, 5 failed. `STATUS` names them.

**`load(slot, state, account)`** and **`command(slot, index)`** (cpu backend): a live
multiplayer game in a slot.
- `state` is the game's state as the backend returns it (`bytes`: the rules version, then the
  serialized game). `account` is your account id as text. `load` returns your player id and the
  index to send your next command with.
- The slot plays (status 1) only while you are to move, with your own view and legal moves, as
  in a game the engine started. While another player is to move it is empty (status 0).
- `command(slot, index)` is the legal move `index` as the bytes to send. It raises if the slot
  is not playing or the index is out of range.
- `load` raises for a state it does not read: another rules version or board side than the
  batch's, a state over 4 MiB, an account id that is not 32 hex digits or has no seat, a
  state saved in the middle of a command, or a field it does not read (it names it). A state
  it reads but cannot play refuses the slot (status 4; `reason` says why). Load each new state
  again; do not `step` a loaded slot and expect the backend to agree.

```python
batch = engine_fast.Batch(games=1, size=11)

def decide(game_id, state):                  # the bot function of polybots-client
    seat, index = batch.load(0, state, MY_ACCOUNT_ID)
    if batch.status()[0] != 1:
        return None                          # not my turn, or the game is over
    offsets, moves = batch.moves()
    tiles, scalars, players, tech = batch.views()
    choice = 0                               # your policy: an index into moves
    return batch.command(0, choice), index
```

**`winner()`**: int32 per slot, the winner's player id of an ended game, 0 otherwise.

**`reason(slot)`**: why a slot was refused or failed, for example the name of a rule the engine
does not implement.

**`moves()`** and **`views()`**: see [Reading the arrays](#reading-the-arrays).

**`len(batch)`** is the number of slots; **`batch.size`** the board side.

## Reading the arrays

**Legal moves: `offsets, moves = batch.moves()`**
- `offsets` is int64 `[games + 1]` and `moves` int32 `[rows, 6]`. Slot `i`'s moves are
  `moves[offsets[i]:offsets[i + 1]]`; a slot not playing has none.
- Columns are `MOVE_FIELDS`: kind, source tile, target tile, option, opponent, price. A field
  that does not apply is -1. `KINDS` maps each kind's name to its id.
- A tile is `y * size + x`.
- A slot's moves are sorted by their first five columns, which identify the move.

**The view: `tiles, scalars, players, tech = batch.views()`**, for the player to move in each
slot:
- `tiles`, int16 `[games, planes, size * size]`: `tiles[slot, plane, y * size + x]`, with the
  planes named in `TILE_PLANES` (terrain, owner, units and so on).
- `scalars`, int32 `[games, 3]`, columns named in `SCALAR_FIELDS` (the viewer, the turn, stars).
- `players`, int64 `[games, 4, 4]`: the viewer first, then the players it has met; columns
  `PLAYER_FIELDS`. `seat` is the player's id, the same numbers as `winner()` and a move's
  `opponent`.
- `tech`, int16 `[games, technologies]`: the ids of the viewer's technologies, in ascending
  order.
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
| `other_view` | `other_views()` | as `views()` | the view of the next seat in turn order after the player to move, dead players skipped (when every other player is dead, the next of them); -1 in slots not playing. For a critic that may see more than the player, never as a policy's input. Raises while a slot holds a loaded live game |

## The action table

A fixed numbering of every move a board side can hold, for a policy with one output per move
(`TABLE_SCHEMA` describes the layout; record it with a model). A move's table slot depends on
the board side, the player to move and the number of seats, not on the other moves. The batch
calls are CPU backend only.
- **`masks()`**: uint32 `[games, words]`; table slot `i` is legal when bit `i % 32` of word
  `i // 32` is set. A slot not playing is all zeros.
- **`table_rows()`**: int64, the table slot of every row of `moves()`, in the same order.
- **`table_frames()`**: int32 `[games, 3]`: board side, player to move, number of seats; -1
  for slots not playing.
- **`step_table(slots)`**: like `step`, with one table slot per game (or `SKIP`); a table slot
  no legal move holds is rejected (2) and changes nothing.
- **`table_slots(size)`** and **`table_blocks(size)`**: the number of table slots for a board
  side (39 784 at 11 x 11), and where each block starts (global, unit, attack, tile, move,
  end).
- **`table_encode(rows, size, mover, seats)`**: rows of 5 or 6 columns to int64 table slots.
  `rows` must be an int32 numpy array, as `moves()` gives; a list or another dtype raises
  `TypeError`. **`table_decode(slots, size, mover, seats)`**: back to
  int32 `[n, 5]` rows; a slot no move uses gives a row of -1. Both raise for a side, mover or
  seat count out of range, and `table_encode` for a row it cannot place.

## The GPU backend

`Batch(..., backend="gpu")` gives the same results as the CPU backend through the same calls,
except `load`, `command`, the action table's batch calls, `income` and `max_health`, which are
CPU only. It also has a loop that stays on the GPU. `device_arrays`, `step_device`, `queue`,
`queue_depth` and `restart_finished` raise on the CPU backend.
- **`device_arrays()`** hands out the batch's arrays (statuses, legal moves, views, outputs) as
  CUDA arrays read in place, for example `torch.as_tensor(a, device="cuda")`. `DEVICE_ARRAYS`
  names the arrays every batch has; the optional outputs add `previews`, `defence` and
  `other_tiles`, `other_scalars`, `other_players`, `other_tech`. Their layouts are the device's:
  `moves` is `[games, rows, 6]` and `previews` `[games, rows, 4]`, padded past `counts`, and
  the per-slot arrays are int32. They change with every step; read them, never write them.
- **`step_device(actions)`** takes a contiguous int32 CUDA array of one move index per slot (a
  policy's output). It returns `(slots not playing, slots that stopped in this call)`, the only
  data it copies to the host. `device_arrays()["applied"]` says what was done per slot.
- **`traffic()`** counts the bytes the batch copied (to the device, to the host); `(0, 0)` on
  the CPU backend.
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
- A batch is used from one thread at a time. `reset`, `step`, `step_table` and
  `restart_finished` release the interpreter lock while they run.
- Every array is a copy owned by the caller (except `device_arrays()`, which are views of device
  memory).
- Names of planes and fields are data: look them up (`TILE_PLANES.index("owner")`) rather than
  hard-coding positions.
