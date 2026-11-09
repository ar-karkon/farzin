import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

TOOL = Path(__file__).parents[1] / 'tools' / 'compare_benchmarks.py'


class SearchSignatureTests(unittest.TestCase):
    def test_timing_changes_pass_but_pv_and_node_changes_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            report = {'commit': 'a' * 40, 'runs': [{'positions': [{
                'position': 'startpos', 'nodes': 100, 'depth': 2, 'bestmove': 'e2e4',
                'score': 'cp 20', 'elapsed_ms': 5,
                'info': ['info depth 2 score cp 20 nodes 100 time 5 pv e2e4 e7e5']
            }]}]}
            first, second, output = root / 'a.json', root / 'b.json', root / 'out.json'
            first.write_text(json.dumps(report))
            item = report['runs'][0]['positions'][0]
            item['elapsed_ms'] = 100
            item['info'][0] = item['info'][0].replace('time 5', 'time 100')
            second.write_text(json.dumps(report))
            command = [sys.executable, str(TOOL), str(first), str(second), '--json', str(output)]
            subprocess.run(command, check=True, capture_output=True, timeout=10)
            self.assertTrue(json.loads(output.read_text())['all_runs_match'])
            item['info'][0] = item['info'][0].replace('e7e5', 'c7c5')
            second.write_text(json.dumps(report))
            self.assertNotEqual(subprocess.run(command, capture_output=True, timeout=10).returncode, 0)
            item['info'][0] = item['info'][0].replace('c7c5', 'e7e5')
            item['nodes'] = 101
            second.write_text(json.dumps(report))
            self.assertNotEqual(subprocess.run(command, capture_output=True, timeout=10).returncode, 0)
