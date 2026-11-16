# engine-fast

A fast, exact rules engine for The Battle of Polytopia, for training game-playing bots. It
plays many games per call: it hands out what the player to move may know (the *view*) and
that player's *legal moves*, and takes one move per game. It is a simulator for training,
not a way to play the game.

Not affiliated with or endorsed by Midjiwan, the game's maker. The tree contains none of
the game's code or data: the engine reads the game's rule data from your own installed copy
of the game (below).

## Layout

| Path | What |
| --- | --- |
| `src/`, `tests/` | the engine (crate `engine-fast`): new games, legal moves, the step, the view and the rule queries |
| `frame/` | shared value types (crate `engine-fast-frame`) |
| `batch/` | many games per call on CPU threads (crate `engine-fast-batch`) |
| `gpu/` | the same batch on an NVIDIA GPU (crate `engine-fast-gpu`, feature `cuda`) |
| `python/` | the Python package `engine_fast` (built with maturin); its README documents the interface |
| `tools/extract_rules.py` | copies the rule data out of an installed game |
| `wheels/` | built Python packages (Linux x86_64 CPU and CUDA, macOS arm64) |

## The rule data

The engine needs the game's rule data (rules version 124, game build 2.18.0.16751), which is
not distributed here. Copy it out of your installed game once:

    python3 -m pip install UnityPy==1.25.4
    python3 tools/extract_rules.py "<the game's install folder>" rules.json
    export ENGINE_FAST_RULES="$PWD/rules.json"

UnityPy is needed by this tool only, not by the engine or the package. The tool reads the
game's `data.unity3d` and does not start or change the game. It refuses
rule data of any other version. The Python package reads the file `ENGINE_FAST_RULES` names
(or `Batch(..., rules=path)`); the Rust tests and benchmarks read it at build time.

## Build and test

Rust 1.82 or later. With `ENGINE_FAST_RULES` set:

    cargo test --release                                   # the engine
    cargo test --release --manifest-path batch/Cargo.toml  # the batch against single games
    cargo test --release --manifest-path gpu/Cargo.toml --features cuda   # needs CUDA 12 and a GPU
    pip install ./python && python -m unittest discover -s python/tests

## Scope

- Rules version 124, two to four seats, board sides 11 to 30 and nano (8 x 8, a board below
  the game's smallest for quick experiments: the game's own map generator runs unchanged at
  that side). Not the Tutorial.
- Every tribe, the four special tribes (Aquarion, Elyrion, Polaris, Cymanti) included.
- The engine never approximates: a game that reaches a rule it does not implement stops,
  with that rule's name as the reason. Non-default skins are refused.
- No rewards: the view gives scores, and the status and winner give the outcome.

## What is checked, and what is not

The engine was developed against a model of the game's rules and a set of recorded online
games. Of 1 097 recorded games (rules version 123, the same rule data; boards up to 18 x 18)
every one replays exactly, decision by decision, special tribes included. The GPU backend
gives the same results and equals the CPU engine bit for bit.

Not verified against recorded games:
- **rules version 124,** the version this package plays: no recorded games exist for it. It
  is checked in random play only, and random play reaches rare mechanics slowly or not at
  all;
- **board sides 19 to 30,** and **nano (8 x 8)**: random-play checks only;
- **Continents and Pangea maps** beyond the recorded games' few;
- the **rule queries** and optional outputs that show what the game's screen shows (attack
  previews, defence bonuses): compared on generated games, not on recorded ones.

The tests here are the engine's own: digests of new games, move admission, allocation counts,
the batch against single games, and the GPU against the CPU. They do not include the
recorded games, which are not distributed.

## Licence

Apache-2.0 (`LICENSE`). See `NOTICE` for third-party material.
