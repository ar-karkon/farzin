import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

BENCH = Path(__file__).parents[1] / "tools" / "bench.py"
ENGINE = '''#!/usr/bin/env python3
import sys
for line in sys.stdin:
    line = line.strip()
    if line == "uci": print("uciok", flush=True)
    elif line.startswith("setoption name EvalFile"):
        print("info string failed to load network: invalid file", flush=True)
    elif line == "isready": print("readyok", flush=True)
    elif line.startswith("go "):
        print("info depth 1 score cp 0 nodes 1 nps 1000 time 1 pv e2e4", flush=True)
        print("bestmove e2e4", flush=True)
    elif line == "quit": break
'''


class BenchmarkTests(unittest.TestCase):
    def test_model_load_failure_cannot_be_reported_as_success(self):
        with tempfile.TemporaryDirectory() as directory:
            engine = Path(directory) / "engine"
            engine.write_text(ENGINE, encoding="utf-8")
            os.chmod(engine, 0o755)
            report = Path(directory) / "report.json"
            result = subprocess.run([sys.executable, str(BENCH), str(engine), "--depth", "1", "--option", "EvalFile=missing", "--json", str(report)], capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("failed to load network", result.stderr)
            self.assertFalse(report.exists())

    def test_success_records_engine_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            engine = Path(directory) / "engine"
            engine.write_text(ENGINE, encoding="utf-8")
            os.chmod(engine, 0o755)
            report = Path(directory) / "report.json"
            subprocess.run([sys.executable, str(BENCH), str(engine), "--depth", "1", "--json", str(report)], capture_output=True, text=True, timeout=10, check=True)
            result = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(len(result["engine_sha256"]), 64)
            self.assertEqual(len(result["positions"]), 3)


if __name__ == "__main__":
    unittest.main()
