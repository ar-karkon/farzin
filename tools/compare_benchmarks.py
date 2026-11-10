#!/usr/bin/env python3
"""Require exact completed-search signatures across all runs of repeated bench reports."""
import argparse
import json
from pathlib import Path
from bench import sha256_file


def signature(run):
    return [(item['position'], item['nodes'], item['depth'], item['bestmove'], item['score'],
             [line.split(' pv ', 1)[1] for line in item['info'] if line.startswith('info depth ')])
            for item in run['positions']]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', nargs='+')
    parser.add_argument('--json', required=True)
    args = parser.parse_args()
    reports = [json.loads(Path(path).read_text()) for path in args.reports]
    reference = signature(reports[0]['runs'][0])
    for path, report in zip(args.reports, reports):
        for index, run in enumerate(report['runs']):
            if signature(run) != reference:
                raise RuntimeError(f'search signature mismatch: {path}, repeat {index + 1}')
    Path(args.json).write_text(json.dumps(dict(
        positions=len(reference), all_runs_match=True,
        fields=['position', 'final_nodes', 'completed_depth', 'bestmove', 'score', 'iteration_PVs'],
        reports=[dict(path=p, sha256=sha256_file(p), commit=r['commit'], repeats=len(r['runs']))
                 for p, r in zip(args.reports, reports)]), indent=2) + '\n')
    print(f'{sum(len(r["runs"]) for r in reports)} runs have equal signatures over {len(reference)} positions')


if __name__ == '__main__':
    main()
