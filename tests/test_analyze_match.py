import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("analyze_match", Path(__file__).parents[1] / "tools" / "analyze_match.py")
analyzer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analyzer)


class MatchAnalysisTests(unittest.TestCase):
    def test_colors_and_unfinished_pairs(self):
        report = {"games": [
            {"opening_fen": "A", "white_engine": 0, "result": "1-0"},
            {"opening_fen": "A", "white_engine": 1, "result": "0-1"},
            {"opening_fen": "B", "white_engine": 0, "result": "*"},
            {"opening_fen": "B", "white_engine": 1, "result": "1/2-1/2"},
        ]}
        result = analyzer.summarize(report)
        self.assertEqual(result["games"], {"wins": 2, "draws": 1, "losses": 0, "unfinished": 1})
        self.assertEqual(result["completed_pairs"], 1)
        self.assertEqual(result["excluded_pairs"], 1)
        self.assertEqual(result["mean_pair_score"], 1.0)
        self.assertLess(result["score_bound"][0], 1.0)

    def test_rejects_unpaired_or_invalid_records(self):
        with self.assertRaises(ValueError):
            analyzer.summarize({"games": [{"opening_fen": "A", "white_engine": 0, "result": "1-0"}]})
        with self.assertRaises(ValueError):
            analyzer.summarize({"games": [{"opening_fen": "A", "white_engine": 0, "result": "1-0"},
                                          {"opening_fen": "B", "white_engine": 1, "result": "0-1"}]})


if __name__ == "__main__":
    unittest.main()
