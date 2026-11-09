import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from test_bench import ENGINE

RUNNER = Path(__file__).parents[1] / 'tools' / 'bench_v3.py'


class RepeatedBenchmarkTests(unittest.TestCase):
    def test_repeats_capture_scores_nodes_hashes_and_dispersion(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            engine = root / 'engine'
            engine.write_text(ENGINE)
            os.chmod(engine, 0o755)
            positions = root / 'positions'
            positions.write_text('startpos\n')
            output = root / 'result.json'
            subprocess.run([sys.executable, str(RUNNER), str(engine), '--commit', 'a' * 40,
                            '--depth', '1', '--repeats', '3', '--positions', str(positions),
                            '--json', str(output)], check=True, capture_output=True, timeout=30)
            result = json.loads(output.read_text())
            self.assertEqual(len(result['runs']), 3)
            self.assertEqual(result['runs'][0]['positions'][0]['nodes'], 1)
            self.assertEqual(result['runs'][0]['positions'][0]['score'], 'cp 0')
            self.assertTrue(result['summary']['positions'][0]['deterministic'])
            self.assertEqual(len(result['binary_sha256']), 64)
            self.assertLessEqual(result['summary']['total_wall_ms_min'],
                                 result['summary']['total_wall_ms_median'])
            self.assertEqual(result['hash_mb'], 16)
            self.assertIsNone(result['network_sha256'])

    def test_external_network_cannot_be_labelled_classical(self):
        result = subprocess.run([sys.executable, str(RUNNER), 'missing', '--commit', 'a' * 40,
                                 '--option', 'EvalFile=missing', '--json', '/tmp/unused.json'],
                                capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('declare --network-kind', result.stderr)
