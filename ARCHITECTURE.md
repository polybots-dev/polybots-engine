# How engine-fast is built

**In one paragraph.** The engine is a Rust library that holds one game as plain data (`Game`)
and moves it forward one decision at a time. Four calls cover a game's life:
- `reset` builds a new game from a configuration;
- `menu` lists the legal moves of the player to move;
- `step` applies one of them;
- `view` writes what that player may know.

The *batch* runs many games through those calls on CPU threads. The *GPU backend* runs the
same rules, written a second time in C, on an NVIDIA GPU, and is tested to give the same
result bit for bit. The Python package wraps either batch as numpy arrays. Every rule is
exact: a rule the engine does not implement stops the game with that rule's name, never a
guess.

This file goes from the top down:
1. [The pieces](#1-the-pieces)
2. [Map of the code](#2-map-of-the-code)
3. [A game's life](#3-a-games-life)
4. [What a player is shown](#4-what-a-player-is-shown)
5. [The CPU batch](#5-the-cpu-batch)
6. [The GPU backend](#6-the-gpu-backend)
7. [How correctness is checked](#7-how-correctness-is-checked)
8. [Extending the engine](#8-extending-the-engine)
9. [Words used in the code](#9-words-used-in-the-code)

## 1. The pieces

```
 rule data (rules.json, from your installed game)
        |
        v
 Tables ----------------------------------------------+
        |                                             |
        v                                             v
 reset(config) -> Game --menu--> legal moves      GPU rules (gpu/kernel/*.h, C)
                   ^  \                            built twice: host library + CUDA
                   |   \--view/queries--> what       |
                 step   the player to move knows     |
                   |                                  |
 Batch (batch/): many Games, CPU threads    GpuBatch (gpu/): many games in GPU memory
        \                                            /
         +------ Python package (python/) ----------+
```

- **Rule data and `Tables`.** The game's rule data is a JSON document of units, technologies,
  terrain and improvements. `tables.rs` compiles it once into lookup tables by id. Nothing in
  the engine hard-codes a unit's statistics or a technology's cost.
- **`Game`** (`state.rs`) is the whole state of one game: board, units, players, the queued
  actions, the random seeds. It can be exported to and imported from a JSON checkpoint.
- **The rules** are ordinary functions over `&mut Game` and `&Tables`, one module per area.
- **The batch** owns many games and keeps each one's legal moves and view up to date.
- **The GPU backend** keeps each game as a fixed-size block of memory on the device.

## 2. Map of the code

**The engine, `src/`**

| File | What it holds |
| --- | --- |
| `lib.rs` | the crate root: the module list |
| `state.rs`, `board.rs`, `inline.rs` | the game state; tiles, units and effect lists; small in-place lists, so copying a game copies no heap data |
| `tables.rs` | the rule data compiled into tables |
| `reset.rs`, `mapgen.rs`, `mapgen/` | a new game: players, then the map generator (candidates, placement, the fairness choice) |
| `menu.rs`, `menu_cache.rs` | the legal moves; the list kept between decisions, redoing only what changed |
| `movement.rs`, `combat.rs` | the move search and the attack search behind the legal moves |
| `step.rs` | applying a decision: a command, then the actions it queues, in order |
| `action.rs` | commands and queued actions as typed values |
| `attack.rs`, `capture.rs`, `infiltrate.rs` | combat, city capture, infiltration |
| `economy.rs`, `reward.rs`, `turns.rs`, `units.rs` | buildings and city growth, city rewards, turn ends, training and transport |
| `explore.rs`, `routes.rs`, `ruins.rs`, `linked.rs` | exploration and meeting players, roads and city connections, ruins, multi-tile units |
| `aquarion.rs`, `elyrion.rs`, `polaris.rs`, `cymanti.rs` | the special tribes' own rules |
| `random.rs` | the game's seeded random streams |
| `view.rs`, `viewer.rs` | what one player may know; what a player remembers of units seen earlier |
| `wire.rs` | reads a game as the game itself serializes it (a live multiplayer state) and writes a legal move as the command to send |
| `queries.rs` | values a player reads off the screen that need a rules computation (income, a unit's panel, a city's work) |
| `contract.rs` | canonical digests of a game state, used by the tests |
| `shadow.rs` | a switch that makes every incremental computation also run in full and compare |

**The other crates**

| Path | What it holds |
| --- | --- |
| `frame/src/lib.rs` | value types shared by the crates: coordinates, actions, legal-list entries, attack previews |
| `batch/src/lib.rs` | `Batch`: slots, `reset`, `step`, statuses, legal moves and views |
| `batch/src/outputs.rs` | the batch's optional outputs (attack previews, defence, income, maximum health) |
| `batch/src/bin/batch_bench.rs` | CPU throughput benchmark |
| `gpu/kernel/` | the rules in C: one header per area, mirroring `src/` (`step.h`, `menu.h`, `view.h`, ...); `twin.c` builds them for the host, `device.cu` for CUDA |
| `gpu/src/state.rs`, `gpu/src/tables.rs` | a `Game` as a device block and back; the rule tables as a device block |
| `gpu/src/batch.rs`, `gpu/src/device.rs` | `GpuBatch`, the batch interface on the GPU; device memory and copies |
| `gpu/src/restarts.rs` | games started on CPU threads ahead of time, for restarting finished slots |
| `gpu/src/step.rs`, `menu.rs`, `view.rs`, `outputs.rs`, `action.rs`, `floats.rs`, `dump.rs` | the Rust side of each kernel area: calling the host build and converting its results, for the tests |
| `gpu/src/check.rs` | plays a recorded game (a list of commands) through the host build, comparing every decision with the Rust engine |
| `gpu/src/bin/` | GPU benchmarks: the step, the view, the batch and the whole device loop |
| `python/src/lib.rs` | the Python module `engine_fast` |
| `tools/extract_rules.py` | copies the rule data out of an installed game |

## 3. A game's life

1. **`reset::reset(config, rules, tables)`** builds a new game: the seats and tribes, then the
   game's own map generator, which draws candidate maps and keeps one fair enough. The same
   configuration and seed always give the same game.
2. **`menu::menu(game, tables)`** lists the legal moves of the player to move. Each entry
   carries the action and its details: its price, and for an attack the forecast the game
   would show.
3. **`step::advance(game, tables, action)`** applies one move. A move is a *command* (move a
   unit, build, research, end the turn). Executing it queues *actions* (a unit moves, a tile
   is explored, a city grows), which run in order until the queue is empty. Ending a turn runs
   the next player's turn start: income, healing, growth, and the special tribes' rules.
4. The game ends when a player wins. A batch also stops a game after a set number of decisions
   (*truncated*).

A rule or setting the engine does not implement returns an error naming it, and the game
stops there as *refused*, whether at the start or during play. A broken internal invariant (a
bug) stops the game as *failed*. Either way the other games in a batch play on. There are no
partial rules.

## 4. What a player is shown

- **The view** (`view.rs`) is a set of planes over the board: terrain, ownership, units,
  cities and so on, with -1 for "not known to this player". It also has the player's stars,
  technologies and the players it has met. It holds only what the game would let that player
  know: unexplored tiles are unknown, invisible enemy units are absent, and another player's
  private figures (its stars, a city's growth) are left out.
- **Rule queries** (`queries.rs`) are values the game's screen shows that need the rules to
  compute: a city's income, a unit's info panel, which units can still act. Queries that count
  units run on the game as the player is shown it (`queries::as_shown`), so a hidden unit does
  not change them.
- **Optional outputs** of the batch are some of those queries. A few show what the screen shows
  and the view leaves out, for example a seen enemy's defence bonus, which depends on that
  player's technologies. They are off unless asked for.

## 5. The CPU batch

- **`Batch`** (`batch/src/lib.rs`) holds a fixed number of slots. Each slot holds one game, its
  status, its sorted legal moves and the view of the player to move, ready to read after every
  call.
- **Threads.** Games are stepped in parallel on a thread pool. Each game is advanced on its
  own, so a result never depends on the number of threads or on the other games in the batch.
- **Incremental work.** Most of a decision's cost is recomputing the legal moves, the view and
  the road network. Each of these keeps its previous result and redoes only what the last move
  could have changed (`menu_cache.rs`, `view::write_kept`, the kept searches in `routes.rs`).
  Each gives exactly what a full recomputation gives. Turning on `shadow::set(true)` makes
  every one of them also run in full and fail the step on any difference.
- **Moves by index.** `step` takes an index into a slot's sorted legal list, so no move outside
  it can be applied. A bad index is rejected and changes nothing.

## 6. The GPU backend

- **The rules a second time, in C.** `gpu/kernel/` holds the rules as a C subset: no heap, no
  recursion, fixed-size storage, and no fused multiply-add or fast-math, so floating point
  gives the same bits as Rust. Each header names the Rust file it follows.
- **Built twice.** The same headers are built as a host library (the *twin*, always built) and
  as CUDA device code (feature `cuda`). The twin lets every rule be tested against the Rust
  engine without a GPU.
- **One game per thread.** Each game lives in a fixed-size *block* in device memory, sized for
  the board. One kernel launch steps every game, writes its status, legal moves and view, and
  the optional outputs.
- **The device loop.** A policy on the same GPU reads the arrays in place (`device_arrays()`)
  and passes its choices as a device array (`step_device`). Each step copies 8 bytes to the
  host. In Rust, `GpuBatch::step_launch` and `step_wait` split a step so the host can work
  while it runs. Every copy goes through one counter (`traffic()`).
- **Starting games.** New games are built on the CPU (the map generator is not ported) and
  uploaded. `queue` builds them ahead of time on background threads, and `restart_finished`
  puts them into finished slots with one upload, without stalling the loop.
- **Where it differs from the CPU batch.** A game that outgrows its block (units, queued
  actions, legal moves) stops with a reason saying so, where the CPU batch plays on. A slot
  that fails during a step has the kernel's reason text, not the Rust one. A command or action
  kind the kernels do not handle would stop the game as "not ported to the device"; this is a
  guard, and the GPU tests count every such stop rather than passing over it.

## 7. How correctness is checked

**The rule: exact, or stop.** Every rule follows the game; where the engine does not implement
something, the game stops with its name. A passing check therefore means equal, never close.

**Outside this repository.** The engine was built against a model of the game's rules. It was
checked by replaying 1 097 recorded online games (rules version 123, the same rule data)
decision by decision, all exact, and by playing random games in step with that model.
A separate check varies everything a player may not know and confirms that the view and the
queries do not change. These checks need the recorded games and the model, which are not
distributed.

**In this repository**

| Test | What it shows |
| --- | --- |
| `tests/reset_digest.rs`, `tests/nano_digest.rs` | new games are pinned: a hash of the whole state of new games, over seeds, sizes, seats, tribes and maps, so any change to map generation or setup shows |
| `tests/move_admission.rs` | a move outside the legal list is refused and leaves the game unchanged |
| `tests/step_allocations.rs` | the step allocates no heap memory per action in the common cases |
| `tests/mangrove.rs` | a board feature no rule makes is refused on import |
| `tests/wire.rs`, `batch/tests/load.rs` | a stored state (a seeded game with placeholder account ids) is read as pinned, what is not a whole state is refused, and a loaded slot plays for the account to move and for no other |
| `batch/tests/matches_single_games.rs` | the batch plays each game exactly as the single-game calls do, at any number of threads |
| `batch/tests/outputs.rs` | the income and maximum-health outputs equal the queries and a second statement of the rule |
| `gpu/tests/step_lockstep.rs`, `menu_lockstep.rs`, `view.rs`, `special_lockstep.rs` | the C rules (host build, and the GPU with `cuda`) equal the Rust engine's step, legal moves and view at every decision of random games, special tribes included |
| `gpu/tests/special_tribes.rs`, `naming.rs` | the special tribes' technology trees; city names drawn the same way on both sides |
| `gpu/tests/state_roundtrip.rs`, `narrow.rs` | a game survives the trip to a device block and back; every narrowed field has a stated reason why its value fits |
| `gpu/tests/float_conformance.rs` | every floating-point expression gives the same bits in Rust, the host build and the GPU |
| `gpu/tests/batch_gpu.rs`, `device_loop.rs`, `restarts.rs` | the GPU batch equals the CPU batch slot for slot, through the host calls, the device arrays and restarts (needs `cuda` and a GPU) |
| `gpu/tests/copies.rs` | no copy between host and device bypasses the byte counter |
| `python/tests/test_batch.py` | the Python package: shapes and types, moves by index, slots stopping alone, independence from the number of workers, the optional outputs |

Random play is a smoke test: it reaches rare rules slowly. The tests above compare two
implementations on the games they reach; they do not prove a rule matches the game where
neither was checked against recorded games (see the main README, "What is checked").

## 8. Extending the engine

**Ground rules**
- A behaviour change ships with a test that fails without it.
- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` pass on every crate.
- Keep results independent of thread count and batch composition: no shared mutable state
  between games.
- Never approximate. If a case is not handled, return an error that names it.

**Adding or fixing a rule**
1. Find the module for its area (section 2) and change it there.
2. If the rule changes what is legal, the legal list follows from the same functions. If it
   reads tiles around a unit or city, check the "what redoes what" list at the top of
   `menu_cache.rs`, so the kept list is redone when its inputs change.
3. Run the engine and batch tests with `shadow::set(true)` in a test of your own, which
   catches a kept result that went stale.
4. Port the change to the matching header in `gpu/kernel/` and run the GPU tests. The host
   build runs without a GPU.

**Adding a view plane:** add its name to `TILE_PLANES` and write it in `view.rs`, then in
`gpu/kernel/view.h`. `gpu/tests/view.rs` will fail until both agree. The Python package takes
`TILE_PLANES` from the engine.

**Adding a batch output:** add the computation in `batch/src/outputs.rs` (usually a call to a
query) and a method on `Batch`. Then add its name to `OUTPUTS` and a method in
`python/src/lib.rs`, with a test in `batch/tests/` against the query itself.

**Finding your way as an assistant.** Start from the call you care about: `reset::reset`,
`menu::menu`, `step::advance`, `view::write`. Each module's first comment says what it holds.
The GPU headers name the Rust file they follow.

## 9. Words used in the code

| Word | Meaning |
| --- | --- |
| decision | one move by one player; the unit of throughput and of `action_limit` |
| command, action | a move as the player gives it; the steps it queues, run in order |
| legal list, candidate, row | the legal moves of the player to move; one entry of it |
| slot | one game's place in a batch |
| seat, player id | a player in a game; `winner()` and the view's `seat` use the same numbers |
| view | what one player may know, as planes over the board plus a few scalars |
| query | a screen value computed by the rules for one player |
| refused, failed, truncated | stopped by a rule or setting outside the engine's scope; stopped by a broken invariant (a bug); stopped at the decision limit |
| block | one game's fixed-size state in GPU memory |
| twin, host build | the GPU's C rules built for the CPU, used for testing without a GPU |
| port | the C version of a Rust rule in `gpu/kernel/`; "not ported" means the C side lacks it |
| kept, incremental | a result carried over from the last decision and redone only where inputs changed |
| shadow mode | every kept result also recomputed in full and compared |
| nano | the 8 x 8 board |
