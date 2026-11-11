"""Regression tests for tools/arena.py scheduling and statistics.

These do not need python-chess: the scheduler and the statistics are pure
Python and are tested with fake workers."""
import os
import sys
import threading
import time
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools"))
import arena  # noqa: E402


class RunJobsTest(unittest.TestCase):
    def test_stop_cancels_pending_without_cancelled_error(self):
        # Regression: once SPRT stopped the match, pending futures were
        # cancelled and as_completed() yielded them, so fut.result() raised
        # concurrent.futures.CancelledError out of the main loop.
        stop = threading.Event()
        seen = []

        def worker(job):
            time.sleep(0.01)
            return job

        def on_result(job, res):
            seen.append(res)
            if len(seen) == 3:
                stop.set()

        counts = arena.run_jobs(list(range(200)), worker, on_result, 2, stop)
        self.assertTrue(stop.is_set())
        self.assertGreaterEqual(counts["completed"], 3)
        self.assertGreater(counts["cancelled"], 100)
        self.assertEqual(counts["completed"], len(seen))
        self.assertEqual(counts["completed"] + counts["cancelled"] + counts["skipped"], 200)

    def test_in_flight_results_after_stop_are_kept(self):
        stop = threading.Event()
        seen = []
        gate = threading.Barrier(4)

        def worker(job):
            if job < 4:
                gate.wait(timeout=5)  # all four start together
            return job

        def on_result(job, res):
            seen.append(res)
            stop.set()

        arena.run_jobs(list(range(50)), worker, on_result, 4, stop)
        # Jobs already running when stop was set still report their games.
        self.assertEqual(sorted(x for x in seen if x < 4), [0, 1, 2, 3])

    def test_worker_exception_does_not_abort(self):
        stop = threading.Event()
        seen = []

        def worker(job):
            if job == 3:
                raise RuntimeError("engine died")
            return job

        counts = arena.run_jobs(list(range(10)), worker, lambda j, r: seen.append(r), 3, stop)
        self.assertEqual(counts["failed"], 1)
        self.assertEqual(counts["completed"], 9)
        self.assertIn("engine died", counts["errors"][0])

    def test_skipped_jobs(self):
        stop = threading.Event()
        counts = arena.run_jobs(list(range(5)), lambda j: None, lambda j, r: None, 2, stop)
        self.assertEqual(counts["skipped"], 5)


class StatisticsTest(unittest.TestCase):
    def test_pentanomial_summary_even(self):
        s = arena.pentanomial_summary([1.0] * 10 + [2.0, 0.0] * 5)
        self.assertAlmostEqual(s["score"], 0.5)
        self.assertAlmostEqual(s["elo"], 0.0, places=6)
        self.assertLess(s["elo_95"][0], 0)
        self.assertGreater(s["elo_95"][1], 0)
        self.assertEqual(s["pentanomial"], [5, 0, 10, 0, 5])
        self.assertAlmostEqual(s["nelo"], 0.0)

    def test_degenerate_sample_has_nonzero_interval(self):
        s = arena.pentanomial_summary([1.0] * 8)
        self.assertLess(s["elo_95"][0], -10)
        self.assertGreater(s["elo_95"][1], 10)
        s = arena.pentanomial_summary([2.0] * 8)
        self.assertLess(s["elo_95"][0], s["elo"])

    def test_sprt_llr_direction(self):
        good = [1.5] * 30 + [1.0] * 30
        bad = [0.5] * 30 + [1.0] * 30
        self.assertGreater(arena.sprt_llr(good, 0, 5), 0)
        self.assertLess(arena.sprt_llr(bad, 0, 5), 0)
        lo, hi = arena.sprt_bounds(0.05, 0.05)
        self.assertAlmostEqual(lo, -hi)
        self.assertAlmostEqual(hi, 2.944, places=3)

    def test_score_for(self):
        self.assertEqual(arena.score_for("a", "a", "1-0"), 1.0)
        self.assertEqual(arena.score_for("a", "b", "1-0"), 0.0)
        self.assertEqual(arena.score_for("a", "b", "0-1"), 1.0)
        self.assertEqual(arena.score_for("a", "b", "1/2-1/2"), 0.5)


class AdjudicatorTest(unittest.TestCase):
    def test_resign_needs_both_sides(self):
        adj = arena.Adjudicator(1000, 2, 5, 3, 40)
        self.assertIsNone(adj.check([1200, 1200, 1200, None], 30))
        self.assertEqual(adj.check([1200, 1100, 1200, 1500], 30), "1-0")
        self.assertEqual(adj.check([-1200, -1100, -1200, -1500], 30), "0-1")
        self.assertIsNone(adj.check([1200, 900, 1200, 1500], 30))

    def test_draw_only_after_move(self):
        adj = arena.Adjudicator(1000, 2, 5, 3, 40)
        self.assertIsNone(adj.check([0] * 6, 39))
        self.assertEqual(adj.check([0] * 6, 40), "1/2-1/2")
        self.assertIsNone(adj.check([0] * 5 + [6], 40))

    def test_presets(self):
        class A:
            adjudicate = "none"
        self.assertIsNone(arena.make_adjudicator(A))
        A.adjudicate = "conservative"
        self.assertEqual(arena.make_adjudicator(A).resign_cp, 1500)


if __name__ == "__main__":
    unittest.main()


class TimeControlTest(unittest.TestCase):
    def test_increment_tc(self):
        self.assertEqual(arena.parse_tc("10+0.1"), (10.0, 0.1))
        self.assertEqual(arena.parse_tc("60"), (60.0, 0.0))

    def test_moves_per_control_tc(self):
        # CCRL 40/15: 40 moves in 15 minutes, repeating.
        self.assertEqual(arena.parse_tc("40/900"), (900.0, 0.0, 40))
        self.assertEqual(arena.parse_tc("40/20+0.5"), (20.0, 0.5, 40))
