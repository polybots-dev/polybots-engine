"""Tests of the installed `engine_fast` package (run: python -m unittest discover -s tests).

They check the package's own surface: shapes and types, the rules for moves by index, a
slot stopping alone, and that results do not depend on the number of workers. The batch
crate's tests check the batch against the single-game path. They read the rule data from
ENGINE_FAST_RULES (README.md).
"""

import struct
import unittest
from pathlib import Path

import numpy as np

import engine_fast

RESIGN = 35


def choose(offsets, moves, status, rng):
    """One move per playing slot: a random legal move, never Resign."""
    choice = np.full(len(status), engine_fast.SKIP, dtype=np.int32)
    for slot in np.flatnonzero(status == 1):
        legal = moves[offsets[slot]:offsets[slot + 1]]
        kept = np.flatnonzero(legal[:, 0] != RESIGN)
        choice[slot] = kept[rng.integers(len(kept))]
    return choice


def play(workers, calls, tribes=(7, 7), size=11, games=6):
    """Every array the batch hands out over `calls` steps of seeded random play."""
    batch = engine_fast.Batch(games=games, size=size, workers=workers)
    batch.reset(np.arange(games), np.arange(101, 101 + games), list(tribes), action_limit=calls + 1)
    rng = np.random.default_rng(7)
    seen = []
    for _ in range(calls):
        status = batch.status()
        offsets, moves = batch.moves()
        seen.append((status, offsets, moves, *batch.views(), batch.winner()))
        applied = batch.step(choose(offsets, moves, status, rng))
        seen.append((applied,))
    return seen


class BatchTest(unittest.TestCase):
    def test_shapes_types_and_names(self):
        games, size = 5, 11
        batch = engine_fast.Batch(games=games, size=size, workers=2)
        self.assertEqual((len(batch), batch.size), (games, size))
        self.assertTrue((batch.status() == 0).all())
        batch.reset(np.arange(games), np.arange(1, games + 1), [7, 7])
        status = batch.status()
        self.assertEqual((status.dtype, status.shape), (np.uint8, (games,)))
        self.assertTrue((status == 1).all())
        offsets, moves = batch.moves()
        self.assertEqual((offsets.dtype, offsets.shape), (np.int64, (games + 1,)))
        self.assertEqual(moves.dtype, np.int32)
        self.assertEqual(moves.shape, (offsets[-1], len(engine_fast.MOVE_FIELDS)))
        self.assertTrue((np.diff(offsets) > 0).all())
        for slot in range(games):
            legal = moves[offsets[slot]:offsets[slot + 1], :5]
            # sorted, and the first five fields identify a move
            self.assertTrue((np.lexsort(legal.T[::-1]) == np.arange(len(legal))).all())
            self.assertEqual(len(np.unique(legal, axis=0)), len(legal))
        tiles, scalars, players, tech = batch.views()
        self.assertEqual((tiles.dtype, tiles.shape), (np.int16, (games, len(engine_fast.TILE_PLANES), size * size)))
        self.assertEqual((scalars.dtype, scalars.shape), (np.int32, (games, len(engine_fast.SCALAR_FIELDS))))
        self.assertEqual((players.dtype, players.shape), (np.int64, (games, 4, len(engine_fast.PLAYER_FIELDS))))
        self.assertEqual(tech.dtype, np.int16)
        explored = tiles[:, engine_fast.TILE_PLANES.index("explored")]
        self.assertTrue(((explored == 0) | (explored == 1)).all())
        self.assertTrue((explored.sum(axis=1) > 0).all())
        # the viewer is the first player row
        seat = engine_fast.PLAYER_FIELDS.index("seat")
        self.assertTrue((players[:, 0, seat] == scalars[:, engine_fast.SCALAR_FIELDS.index("viewer")]).all())
        self.assertEqual(engine_fast.STATUS[1], "playing")
        self.assertEqual(engine_fast.RULES, 124)

    def test_results_do_not_depend_on_the_number_of_workers(self):
        for tribes, size in (((7, 7), 11), ((7, 7, 7, 7), 14)):
            one = play(1, 120, tribes, size)
            many = play(4, 120, tribes, size)
            self.assertEqual(len(one), len(many))
            for a, b in zip(one, many):
                for x, y in zip(a, b):
                    np.testing.assert_array_equal(x, y)

    def test_moves_by_index_skip_and_rejection(self):
        batch = engine_fast.Batch(games=3)
        batch.reset(np.array([0, 2]), np.array([5, 5]), [7, 7])
        self.assertEqual(batch.status().tolist(), [1, 0, 1])
        offsets, moves = batch.moves()
        before = [x.copy() for x in batch.views()]
        past = offsets[1] - offsets[0]
        # slot 0: an index past its list; slot 1: empty; slot 2: skipped
        applied = batch.step(np.array([past, 0, engine_fast.SKIP], dtype=np.int32))
        self.assertEqual(applied.tolist(), [2, 2, 0])
        for x, y in zip(before, batch.views()):
            np.testing.assert_array_equal(x, y)
        np.testing.assert_array_equal(batch.moves()[1], moves)
        # the two slots hold the same game: the same move gives the same next view
        applied = batch.step(np.array([0, engine_fast.SKIP, 0], dtype=np.int32))
        self.assertEqual(applied.tolist(), [1, 0, 1])
        for array in batch.views():
            np.testing.assert_array_equal(array[0], array[2])
            self.assertTrue((array[1] == -1).all())
        with self.assertRaises(ValueError):
            batch.step(np.array([0], dtype=np.int32))
        with self.assertRaises(ValueError):
            batch.reset(np.array([3]), np.array([1]), [7, 7])
        with self.assertRaises(ValueError):
            batch.reason(3)

    def test_a_slot_stops_alone(self):
        batch = engine_fast.Batch(games=2)
        batch.reset(np.array([0]), np.array([1]), [7, 7], action_limit=3)
        # one seat is outside the engine's scope: refused, with the reason
        batch.reset(np.array([1]), np.array([1]), [7])
        self.assertEqual(batch.status().tolist(), [1, 4])
        self.assertIn("scope", batch.reason(1))
        self.assertEqual(batch.reason(0), "")
        rng = np.random.default_rng(1)
        for _ in range(3):
            self.assertEqual(batch.status()[0], 1)
            offsets, moves = batch.moves()
            self.assertEqual(offsets[2] - offsets[1], 0)
            batch.step(choose(offsets, moves, batch.status(), rng))
        # the decision limit
        self.assertEqual(batch.status().tolist(), [3, 4])
        self.assertEqual(batch.winner().tolist(), [0, 0])
        self.assertEqual(batch.moves()[1].shape[0], 0)
        # a stopped slot can be started again
        batch.reset(np.array([1]), np.array([2]), [7, 7, 7])
        self.assertEqual(batch.status().tolist(), [3, 1])

    def test_arguments_of_any_integer_type_and_the_board_side(self):
        batch = engine_fast.Batch(games=4)
        batch.reset([0, 1, 2, 3], np.arange(4, dtype=np.int32), [7, 7])
        # a list, an int64 array and a strided view all work
        self.assertEqual(batch.step([0, -1, -1, -1]).tolist(), [1, 0, 0, 0])
        self.assertEqual(batch.step(np.array([-1, 0, -1, -1])).tolist(), [0, 1, 0, 0])
        wide = np.zeros(8, dtype=np.int32)
        wide[::2] = [-1, -1, 0, 0]
        self.assertEqual(batch.step(wide[::2]).tolist(), [0, 0, 1, 1])
        for size in (5, 7, 9, 10, 31, 50000):
            with self.assertRaises(ValueError):
                engine_fast.Batch(games=1, size=size)
        with self.assertRaises(ValueError):
            batch.reset([0, 0], [1, 2], [7, 7])
        self.assertEqual(engine_fast.KINDS["Resign"], RESIGN)
        self.assertEqual(engine_fast.APPLIED[2], "rejected")

    def test_nano_games_start_and_play(self):
        """Nano (side 8): two to four seats start, views have 64 tiles, play does not depend on workers."""
        for tribes in ((7, 7), (7, 7, 7), (7, 4, 8, 10)):
            batch = engine_fast.Batch(games=3, size=8, workers=2)
            self.assertEqual(batch.size, 8)
            batch.reset(np.arange(3), np.arange(1, 4), list(tribes))
            self.assertEqual(batch.status().tolist(), [1, 1, 1])
            tiles = batch.views()[0]
            self.assertEqual(tiles.shape[2], 64)
            self.assertTrue((tiles[:, engine_fast.TILE_PLANES.index("terrain")] != -1).any())
        one, many = play(1, 120, size=8), play(3, 120, size=8)
        for a, b in zip(one, many):
            for x, y in zip(a, b):
                np.testing.assert_array_equal(x, y)

    def test_an_ended_slot_refused_at_reset_names_no_winner(self):
        batch = engine_fast.Batch(games=1)
        batch.reset([0], [3], [7, 7])
        offsets, moves = batch.moves()
        batch.step([int(np.flatnonzero(moves[:, 0] == RESIGN)[0])])
        self.assertNotEqual(batch.winner()[0], 0)
        batch.reset([0], [3], [7])
        self.assertEqual((batch.status()[0], batch.winner()[0]), (4, 0))

    def test_a_resigned_game_ends_with_its_winner(self):
        batch = engine_fast.Batch(games=1)
        batch.reset(np.array([0]), np.array([3]), [7, 7])
        viewer = batch.views()[1][0, engine_fast.SCALAR_FIELDS.index("viewer")]
        offsets, moves = batch.moves()
        resign = np.flatnonzero(moves[:, 0] == RESIGN)
        self.assertEqual(len(resign), 1)
        batch.step(np.array([resign[0]], dtype=np.int32))
        self.assertEqual(batch.status().tolist(), [2])
        self.assertNotIn(batch.winner()[0], (0, viewer))




class OutputsTest(unittest.TestCase):
    """The `income` and `max_health` outputs (the cpu backend only). Their values are
    checked against the per-game queries in the batch crate's `tests/outputs.rs`."""

    def test_shapes_types_and_their_relation_to_the_view(self):
        games, size = 4, 11
        batch = engine_fast.Batch(games=games + 1, size=size, outputs=("income", "max_health"))
        batch.reset(np.arange(games), np.arange(1, games + 1), [7, 7, 7])
        rng = np.random.default_rng(3)
        shown = 0
        for _ in range(150):
            status = batch.status()
            income, max_health = batch.income(), batch.max_health()
            self.assertEqual((income.dtype, income.shape), (np.int32, (games + 1,)))
            self.assertEqual((max_health.dtype, max_health.shape), (np.int16, (games + 1, size * size)))
            # slots not playing (the last one is never started) have none
            self.assertTrue((income[status != 1] == -1).all())
            self.assertTrue((max_health[status != 1] == -1).all())
            self.assertTrue((income[status == 1] >= 0).all())
            tiles = batch.views()[0]
            owner = tiles[:, engine_fast.TILE_PLANES.index("unit_owner")]
            panel = max_health >= 0
            # a unit's maximum health shows only where the view shows the unit (a carrier's is
            # its passenger's, so it is not compared with the carrier's health)
            self.assertTrue((owner[panel] > 0).all())
            shown += int(panel.sum())
            offsets, moves = batch.moves()
            batch.step(choose(offsets, moves, status, rng))
        self.assertGreater(shown, 0)

    def test_an_output_not_asked_for_is_refused(self):
        batch = engine_fast.Batch(games=1, size=11, outputs=("income",))
        batch.reset(np.arange(1), np.arange(1, 2), [7, 7])
        self.assertEqual(batch.income().shape, (1,))
        with self.assertRaisesRegex(ValueError, "max_health"):
            batch.max_health()
        with self.assertRaisesRegex(ValueError, "unknown output"):
            engine_fast.Batch(games=1, outputs=("health",))
        for output in ("income", "max_health"):
            with self.assertRaisesRegex(ValueError, "cpu backend only"):
                engine_fast.Batch(games=1, backend="gpu", outputs=(output,))



class OtherView(unittest.TestCase):
    """The other seat's view (`outputs=("other_view",)`), a privileged critic's input: laid out
    as `views`, it is the view of the seat the turn passes to next (in a 2-seat game the other
    player), -1 in slots not playing, and every other array is the same with and without it.
    Its values are checked against the view function in the batch crate's
    `tests/other_view.rs`."""

    def test_layout_seat_and_every_other_array_unchanged(self):
        games, size = 5, 8
        made = [engine_fast.Batch(games=games + 1, size=size, workers=2, outputs=o) for o in (("other_view",), ())]
        for batch in made:
            batch.reset(np.arange(games), np.arange(21, 21 + games), [7, 7])
        with_it, without = made
        viewer = engine_fast.SCALAR_FIELDS.index("viewer")
        rng = np.random.default_rng(5)
        for _ in range(200):
            status = with_it.status()
            offsets, moves = with_it.moves()
            arrays = [status, offsets, moves, *with_it.views(), with_it.winner()]
            others = [without.status(), *without.moves(), *without.views(), without.winner()]
            for a, b in zip(arrays, others):
                np.testing.assert_array_equal(a, b)
            other = with_it.other_views()
            for a, b in zip(other, arrays[3:7]):
                self.assertEqual((a.dtype, a.shape), (b.dtype, b.shape))
            playing = status == 1
            self.assertTrue(all((a[~playing] == -1).all() for a in other))
            # two seats, ids 1 and 2: the other seat is the player not to move
            mover = arrays[4][:, viewer]
            self.assertTrue((other[1][playing, viewer] == 3 - mover[playing]).all())
            choice = choose(offsets, moves, status, rng)
            np.testing.assert_array_equal(with_it.step(choice), without.step(choice))
        with self.assertRaisesRegex(ValueError, "other_view"):
            without.other_views()


class TableTest(unittest.TestCase):
    """The fixed action table from Python: the mask is the legal list's rows encoded, decode
    inverts encode, and stepping by slot plays as stepping by index. The table itself is
    checked in the batch crate (`src/table.rs`, `tests/table.rs`)."""

    def test_masks_rows_and_frames_agree_with_the_list(self):
        games, size = 3, 11
        batch = engine_fast.Batch(games=games + 1, size=size)
        batch.reset(np.arange(games), np.arange(1, games + 1), [7, 3, 16])
        words = -(-engine_fast.table_slots(size) // 32)
        self.assertEqual(engine_fast.table_blocks(size)[-1], engine_fast.table_slots(size))
        rng = np.random.default_rng(5)
        for _ in range(60):
            status = batch.status()
            offsets, moves = batch.moves()
            masks, rows, frames = batch.masks(), batch.table_rows(), batch.table_frames()
            self.assertEqual((masks.dtype, masks.shape), (np.uint32, (games + 1, words)))
            self.assertEqual(len(rows), len(moves))
            self.assertTrue((masks[games] == 0).all() and (frames[games] == -1).all())
            for g in np.flatnonzero(status == 1):
                side, mover, seats = frames[g]
                legal = moves[offsets[g]:offsets[g + 1]]
                slots = engine_fast.table_encode(legal, side, mover, seats)
                self.assertTrue((slots == rows[offsets[g]:offsets[g + 1]]).all())
                bits = np.unpackbits(masks[g].view(np.uint8), bitorder="little")
                self.assertEqual(set(np.flatnonzero(bits)), set(slots.tolist()))
                back = engine_fast.table_decode(slots, side, mover, seats)
                self.assertTrue((back == legal[:, :5]).all())
            batch.step(choose(offsets, moves, status, rng))
        self.assertTrue((engine_fast.table_decode([-1, 10**9], 11, 1, 2) == -1).all())
        with self.assertRaises(ValueError):
            engine_fast.table_encode(np.array([[6, 3, 3, -1, -1]], dtype=np.int32), 11, 1, 2)
        self.assertIn("action-table v1", engine_fast.TABLE_SCHEMA)

    def test_stepping_by_slot_equals_stepping_by_index(self):
        def batch():
            b = engine_fast.Batch(games=3, size=11)
            b.reset(np.arange(3), np.arange(11, 14), [7, 2])
            return b
        by_index, by_slot = batch(), batch()
        rng = np.random.default_rng(9)
        for _ in range(80):
            status = by_index.status()
            offsets, moves = by_index.moves()
            choice = choose(offsets, moves, status, rng)
            rows = by_index.table_rows()
            slots = np.full(3, engine_fast.SKIP, dtype=np.int64)
            for g in np.flatnonzero(choice != engine_fast.SKIP):
                slots[g] = rows[offsets[g] + choice[g]]
            self.assertTrue((by_index.step(choice) == by_slot.step_table(slots)).all())
            for a, b in zip(by_index.views(), by_slot.views()):
                self.assertTrue((a == b).all())
            self.assertTrue((by_slot.moves()[1] == by_index.moves()[1]).all())


class Load(unittest.TestCase):
    """A multiplayer state in a slot: the engine's stored state (a seeded two-seat game, 11 by
    11, placeholder account ids), with the rules version the backend puts in front."""

    VERSION = struct.pack("<i", 124)
    ACCOUNTS = {1: "01010101-0101-0101-0101-010101010101", 2: "02020202020202020202020202020202"}

    def setUp(self):
        stored = Path(__file__).resolve().parents[2] / "tests" / "data" / "state.hex"
        self.state = self.VERSION + bytes.fromhex(stored.read_text())

    def test_the_slot_plays_for_the_account_to_move_only(self):
        batch = engine_fast.Batch(games=1, size=11)
        statuses = []
        for seat, account in self.ACCOUNTS.items():
            self.assertEqual(batch.load(0, self.state, account), (seat, 235))
            statuses.append(int(batch.status()[0]))
            offsets, moves = batch.moves()
            if statuses[-1] == 1:
                self.assertGreater(len(moves), 1)
                for index in range(len(moves)):
                    command = batch.command(0, index)
                    self.assertEqual(command[:4], self.VERSION)
                    self.assertEqual((command[4], command[6]), (moves[index, 0], seat))
                with self.assertRaises(ValueError):
                    batch.command(0, len(moves))
            else:
                self.assertEqual(len(moves), 0)
                with self.assertRaises(ValueError):
                    batch.command(0, 0)
        self.assertEqual(sorted(statuses), [0, 1])

    def test_what_is_refused(self):
        batch = engine_fast.Batch(games=1, size=11)
        refused = [
            (struct.pack("<i", 123) + self.state[4:], self.ACCOUNTS[1]),  # another version in front
            (self.state[:200], self.ACCOUNTS[1]),  # not a whole state
            (self.state, "09090909-0909-0909-0909-090909090909"),  # an account with no seat
            (self.state, "not an account id"),
        ]
        for state, account in refused:
            with self.assertRaises(ValueError):
                batch.load(0, state, account)
        with self.assertRaises(ValueError):  # a batch of another board size
            engine_fast.Batch(games=1, size=14).load(0, self.state, self.ACCOUNTS[1])
        self.assertEqual(int(batch.status()[0]), 0)


if __name__ == "__main__":
    unittest.main()
