# engine-fast

**A fast, exact simulator of The Battle of Polytopia's rules, for training game-playing bots.**

- **What it does.** It runs thousands of games side by side. For each game it hands you what
  the player to move may know (the *view*) and that player's *legal moves*. It then takes one
  move per game and advances every game in one call.
- **What it is for.** Self-play, reinforcement learning, and evaluating bots and search:
  anywhere you need many games quickly, with the rules exactly right. It is not a way to play
  the game.
- **Exact or nothing.** The engine does not approximate. If a game reaches a rule the engine
  does not implement, that game stops and names the rule; the other games play on.
- **Two backends, one interface.** CPU threads, or an NVIDIA GPU that keeps every game, view and
  legal list in device memory.
- **Python or Rust.** A Python package (`engine_fast`, numpy arrays) on top of Rust crates.
- **The rules come with it.** The rules are in the engine's own format
  (`rules/rules-124.json`), and the Python package carries them, so no copy of the game is
  needed: it runs as it is, also for players who have the game only on a phone or tablet. The
  game's own rule data from a desktop copy works too (see [Rule data](#rule-data)).

## Contents

1. [Quick start](#quick-start)
2. [Performance](#performance)
3. [Rule data](#rule-data)
4. [Where things are](#where-things-are)
5. [Build and test](#build-and-test)
6. [Scope](#scope)
7. [What is checked](#what-is-checked)
8. [Disclaimer](#disclaimer) and [Licence](#licence)

Further reading:
- [`python/README.md`](python/README.md): the Python interface, call by call.
- [`ARCHITECTURE.md`](ARCHITECTURE.md): how the engine is built, how it is checked, and how to
  extend it. Start there if you are changing the code, or if you are an AI assistant handed
  this repository.

## Quick start

Install a built Python package (Python 3.11 or later). From the repository's release:

Windows x86_64:

    pip install https://github.com/polybots-dev/polybots-engine/releases/download/v0.1.0/engine_fast-0.1.0-cp311-abi3-win_amd64.whl

Linux x86_64:

    pip install https://github.com/polybots-dev/polybots-engine/releases/download/v0.1.0/engine_fast-0.1.0-cp311-abi3-manylinux_2_28_x86_64.whl

Linux arm64:

    pip install https://github.com/polybots-dev/polybots-engine/releases/download/v0.1.0/engine_fast-0.1.0-cp311-abi3-manylinux_2_28_aarch64.whl

macOS, Intel:

    pip install https://github.com/polybots-dev/polybots-engine/releases/download/v0.1.0/engine_fast-0.1.0-cp311-abi3-macosx_11_0_x86_64.whl

From a clone of the repository, in its top folder:

    pip install wheels/macos/engine_fast-0.1.0-cp311-abi3-macosx_11_0_arm64.whl   # macOS, Apple silicon
    pip install wheels/cuda/engine_fast-0.1.0-cp311-abi3-linux_x86_64.whl         # Linux x86_64, CPU and GPU

- The release's wheels (with their SHA-256 in its `SHA256SUMS`) are built and tested by
  `.github/workflows/wheels.yml` on GitHub's machines. Each was installed on a clean machine and
  ran the package tests and this README's example with no game data: Windows Server 2025 with
  Python 3.11 and 3.13, Ubuntu 22.04 and AlmaLinux 8 (Linux x86_64), Ubuntu 22.04 (Linux arm64)
  and macOS 15 (Intel).
- The Linux wheels of the release need glibc 2.28 or later (Ubuntu 20.04, Debian 10, RHEL 8
  and later; tested on Ubuntu 22.04 and AlmaLinux 8, the others by their glibc). Ubuntu
  22.04's own Python is 3.10: install `python3.11` and `python3.11-venv` with apt (its
  `python3.11` is 3.11.0rc1, on which the tests pass), make a venv with
  `python3.11 -m venv venv`, and use the venv's `pip`.
- The CUDA wheel in `wheels/cuda/` was built on Ubuntu 24.04 (glibc 2.39) and is not a
  manylinux wheel: on a Linux with an older glibc it may install but fail to import.
- The CUDA wheel is built for GPU architecture `sm_86` (compute capability 8.6: the RTX 30
  series, A10, A40) with CUDA 12, tested on an RTX 3090, and needs an NVIDIA driver for CUDA
  12. It carries no PTX, so by CUDA's rules it runs only on compute capability 8.6 to 8.9 (8.9,
  the RTX 40 series, is untested). For another GPU, an A100 (8.0) for example, build from
  source with `EF_CUDA_ARCH` (see [Build and test](#build-and-test)).
- Anywhere else, or to build from source (it needs a Rust toolchain): `pip install ./python`.

Play 64 two-player games on an 11 x 11 board with random moves:

```python
import numpy as np
import engine_fast

batch = engine_fast.Batch(games=64, size=11, workers=4)
batch.reset(np.arange(64), seeds=np.arange(1, 65), tribes=[7, 7])
rng = np.random.default_rng(0)
while (batch.status() == 1).any():                    # 1: playing
    offsets, moves = batch.moves()                    # game i: moves[offsets[i]:offsets[i + 1]]
    tiles, scalars, players, tech = batch.views()     # what each player to move may know
    count = np.maximum(np.diff(offsets), 1)
    pick = np.where(batch.status() == 1, rng.integers(0, count), engine_fast.SKIP)
    batch.step(pick.astype(np.int32))                 # one index into each game's moves
print(batch.status(), batch.winner())                 # 2: ended, 3: reached the move limit
```

Random moves include resigning, so these games end quickly. A real policy replaces the random
pick: it reads the views and chooses an index into each game's list of legal moves. A move is always an index into that list, so an illegal move
cannot be applied.

## Performance

Decisions per second; a decision is one move by one player. Random moves, two-player games,
each game capped at 1 000 decisions. Measured on one 18-core server with an NVIDIA RTX
3090. Your figures will differ with the hardware, board size, game length and the policy's
own cost.

| Backend | 11 x 11 | 16 x 16 |
| --- | ---: | ---: |
| CPU, 1 core | 55 000 | 45 000 |
| CPU, 18 cores (36 threads) | 517 000 | 303 000 |
| GPU, 16 384 games, end to end with finished games restarted | 353 000 | 271 000 |
| GPU, 65 536 games, the same | 664 000 | 487 000 |

On the 8 x 8 board, with short games of training length, the GPU reaches about 1 million
decisions per second at 65 536 games. On the GPU a step copies 8 bytes to the host: views,
legal moves and the policy's choices stay in device memory. Finished games restart from a
queue that CPU threads fill ahead of time.

## Rule data

Nothing to set up: the rules of version 124 (game build 2.18.0.16751) are in
`rules/rules-124.json`, in the engine's own format: each unit, tech, improvement, tribe,
terrain, resource and task by name, with only the values the engine reads. They were written
from the game's own rule data and give the engine the same tables. The Python package carries
this file, and the Rust tests and benchmarks read it at build time. `Tables::compile` in
`src/tables.rs` reads it, and its types document every field.

The game's own rule data works too, if you have a desktop copy of the game. You do not need
it. Copy it out once:

    python3 -m pip install UnityPy==1.25.4
    python3 tools/extract_rules.py "<the game's install folder>" rules.json
    export ENGINE_FAST_RULES="$PWD/rules.json"

- `tools/extract_rules.py` reads the game's `data.unity3d` (in the Mac and Linux installs) and
  does not start or change the game. It refuses rule data of any other version.
- UnityPy is needed by this tool only, not by the engine or the package.
- The Python package reads the file `ENGINE_FAST_RULES` names, or `Batch(..., rules=path)`, in
  place of the file it carries. It takes a file in the engine's format as it is, and the game's
  own data only when its SHA-256 is that of rules version 124. Both give the same games.
- A new game's state names the rule file it was made with and its SHA-256 (`game.rules`). Made
  from the game's own data, it carries that text instead (`game.catalog_json`).

## Where things are

| Path | What |
| --- | --- |
| `src/` | the engine (crate `engine-fast`): new games, legal moves, the step, the view, the rule queries |
| `src/mapgen/` | the game's map generator |
| `tests/` | the engine's tests |
| `frame/` | small value types shared by the crates (crate `engine-fast-frame`) |
| `batch/` | many games per call on CPU threads (crate `engine-fast-batch`) |
| `gpu/` | the same batch on an NVIDIA GPU (crate `engine-fast-gpu`, feature `cuda`) |
| `gpu/kernel/` | the rules in C, built for the host and for CUDA |
| `python/` | the Python package `engine_fast` (built with maturin) |
| `rules/` | the rule data, in the engine's own format |
| `tools/extract_rules.py` | copies the game's own rule data out of an installed game (optional) |
| `wheels/` | built Python packages: `macos/` (arm64) and `cuda/` (Linux x86_64, `sm_86`); the other platforms' are the release's |
| `.github/workflows/wheels.yml` | builds the Windows, Linux (manylinux) and Intel macOS wheels and tests each on a clean machine; run it from the Actions tab |

`ARCHITECTURE.md` has a file-by-file map.

## Build and test

Rust 1.92 or later; Python 3.11 or later with numpy 2.1 or later. The tests use
`rules/rules-124.json`; the Python tests also play on the game's own data where
`ENGINE_FAST_RULES` names it.

    cargo test --release                                   # the engine
    cargo test --release --manifest-path batch/Cargo.toml  # the batch against single games
    cargo test --release --manifest-path gpu/Cargo.toml    # the GPU rules, host build
    cargo test --release --manifest-path gpu/Cargo.toml --features cuda   # needs CUDA 12 and a GPU
    pip install ./python && python -m unittest discover -s python/tests

For the Python package with the GPU backend, run `maturin build --release --features cuda` in
`python/` on a machine with CUDA 12 and its `nvcc`. `EF_CUDA_ARCH` picks the GPU architecture
(default `sm_86`). The device code takes several minutes to compile.

## Scope

- Rules version 124, the version the Python package plays. The Rust crates also take 123, the
  version before, which uses the same rule data. Two to four players. Not the Tutorial.
- Board sides 11 to 30, and 8 x 8 ("nano"): a board below the game's smallest, for quick
  experiments. The game's own map generator runs unchanged at that size.
- Every tribe, the four special tribes (Aquarion, Elyrion, Polaris, Cymanti) included.
- New games use the default skins; a loaded live game keeps the skins its players chose.
- No rewards: the view gives scores, and the status and winner give the outcome.

## What is checked

The engine was built against a model of the game's rules and checked on recorded online games:
1 097 recorded games (rules version 123, the same rule data; boards up to 18 x 18) replay
exactly on the game's own rule data, which gives the same tables as `rules/rules-124.json`,
decision by decision, special tribes included. On those games the GPU backend gives
the same results, equal to the CPU engine bit for bit. Those checks need the recorded games and the model,
which are not distributed, so they cannot be rerun from this repository.

Not verified against recorded games:
- **rules version 124,** the version this package plays: no recorded games exist for it. It is
  checked in random play only, and random play reaches rare mechanics slowly or not at all;
- **board sides 19 to 30** and **8 x 8**: random-play checks only;
- **Continents and Pangea maps** beyond the recorded games' few;
- the **rule queries and optional outputs** (attack previews, defence bonuses, income, maximum
  health, the other seat's view) and the **action table**: compared on generated games, not
  on recorded ones.

The tests in this repository are the engine's own: digests of new games, move admission,
allocation counts, loading stored states, the batch against single games, the optional
outputs and the action table, the GPU rules against the Rust engine, and the Python
package. `ARCHITECTURE.md` lists what each one shows.

## Disclaimer

This is an unofficial community project. It is not affiliated with, endorsed by or
supported by Midjiwan AB. *The Battle of Polytopia* and related names are trademarks of
Midjiwan AB, used here only to describe what the project simulates. We respect Midjiwan's
wishes. If Midjiwan asks us to change, restrict or remove any part of this project, we will
do so promptly. Contact the maintainers by opening an issue.

## Licence

Apache-2.0 (`LICENSE`). See `NOTICE` for third-party material.
