import importlib.util
import os
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("uci_preflight", Path(__file__).parents[1] / "tools" / "uci_preflight.py")
preflight = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preflight)


class PreflightTests(unittest.TestCase):
    def test_confirmed_and_rejected_network(self):
        with tempfile.TemporaryDirectory() as directory:
            engine = Path(directory) / "engine"
            engine.write_text('''#!/usr/bin/env python3
import sys
for line in sys.stdin:
    line = line.strip()
    if line == "uci": print("id name Farzin test\\nuciok", flush=True)
    elif line.endswith("good.pnn"): print("info string experimental network loaded", flush=True)
    elif line.startswith("setoption"): print("info string failed to load network", flush=True)
    elif line == "isready": print("readyok", flush=True)
    elif line == "quit": break
''', encoding="utf-8")
            os.chmod(engine, 0o755)
            self.assertTrue(preflight.verify_farzin_network(str(engine), "good.pnn")["load_confirmed"])
            with self.assertRaises(RuntimeError):
                preflight.verify_farzin_network(str(engine), "bad.pnn")


if __name__ == "__main__":
    unittest.main()
