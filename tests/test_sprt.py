import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools"))
import sprt  # noqa: E402


def game(white, result, fen="f"):
    return {"white_engine": white, "result": result, "opening_fen": fen}


class SprtTest(unittest.TestCase):
    def test_pentanomial_counts_pairs_from_engine_a_view(self):
        report = {"games": [game(0, "1-0"), game(1, "0-1"),      # A wins both: 1.0
                            game(0, "1/2-1/2"), game(1, "1-0"),  # A draws, loses: 0.25
                            game(0, "*"), game(1, "1-0")]}       # unfinished pair
        counts, games, excluded = sprt.pentanomial([report])
        self.assertEqual(counts, [0, 1, 0, 0, 1])
        self.assertEqual(excluded, 1)
        self.assertEqual(games["unfinished"], 1)
        counts, _, excluded = sprt.pentanomial([report], unfinished_as_draw=True)
        self.assertEqual(counts, [0, 2, 0, 0, 1])
        self.assertEqual(excluded, 0)

    def test_rejects_unpaired_games(self):
        with self.assertRaises(ValueError):
            sprt.pentanomial([{"games": [game(0, "1-0", "a"), game(1, "1-0", "b")]}])

    def test_llr_sign_and_decisions(self):
        even = sprt.analyse([10, 40, 100, 40, 10], 0, 10, 0.05, 0.05)
        self.assertAlmostEqual(even["score"], 0.5)
        self.assertAlmostEqual(even["elo"], 0.0, places=6)
        self.assertLess(even["llr"], 0)
        strong = sprt.analyse([0, 10, 100, 200, 100], 0, 10, 0.05, 0.05)
        self.assertEqual(strong["decision"], "accept H1")
        weak = sprt.analyse([100, 200, 100, 10, 0], 0, 10, 0.05, 0.05)
        self.assertEqual(weak["decision"], "accept H0")
        self.assertEqual(sprt.analyse([0] * 5, 0, 10, 0.05, 0.05)["decision"], "continue")

    def test_elo_round_trip(self):
        for elo in (-200, -10, 0, 35, 400):
            self.assertAlmostEqual(sprt.elo_from_score(sprt.expected_score(elo)), elo, places=6)


if __name__ == "__main__":
    unittest.main()
