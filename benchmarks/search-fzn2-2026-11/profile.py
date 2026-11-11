#!/usr/bin/env python3
"""Alternate exact-node Rust audit binaries; keep TSV, CPU/RSS and summaries.

Run with no concurrent matches/builds. Each --binary is LABEL=/absolute/path.
Build every worktree with its own target directory, or clean the farzin package
before switching worktrees; a shared target cache can otherwise reuse stale code.
"""
import argparse
import collections
import csv
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import sys


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', action='append', required=True)
    parser.add_argument('--network', required=True)
    parser.add_argument('--positions', default='tools/positions-v3.fen')
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    binaries = dict(spec.split('=', 1) for spec in args.binary)
    assert len(binaries) == len(args.binary) and args.repeats >= 3
    assert all(name.replace('-', '_').isidentifier() for name in binaries)
    output = Path(args.out)
    output.mkdir(parents=True, exist_ok=False)
    expected = sum(bool(s.strip()) and not s.lstrip().startswith('#')
                   for s in Path(args.positions).read_text().splitlines())
    report = dict(network_sha256=sha256(args.network), positions_sha256=sha256(args.positions),
        binaries={name: dict(path=str(Path(path).resolve()), sha256=sha256(path))
                  for name, path in binaries.items()}, positions=expected,
        repeats=args.repeats, runs=[], summary={})
    results = collections.defaultdict(list)
    for mode_index, (mode, limit) in enumerate([('nodes', 100000), ('depth', 14)]):
        for repeat in range(args.repeats):
            order = list(binaries)
            if (repeat + mode_index) % 2:
                order.reverse()
            for name in order:
                stem = f'{name}-{mode}-{repeat}'
                tsv = output / f'{stem}.tsv'
                usage = output / f'{stem}.resource.json'
                command = [sys.executable, str(Path(__file__).with_name('measure.py')), str(usage),
                           binaries[name], args.network, args.positions, mode, str(limit), '1']
                with tsv.open('w') as target:
                    subprocess.run(command, stdout=target, check=True)
                with tsv.open() as source:
                    rows = list(csv.DictReader(source, delimiter='\t'))
                assert len(rows) == expected
                signature = [(r['position'], r['nodes'], r['depth'], r['score'],
                              r['bestmove'], r['pv']) for r in rows]
                results[name, mode].append((rows, json.loads(usage.read_text()), signature))
                report['runs'].append(dict(engine=name, mode=mode, repeat=repeat,
                                            tsv=tsv.name, resource=usage.name))
                print(stem, flush=True)
    baseline = next(iter(binaries))
    for (name, mode), runs in results.items():
        assert all(run[2] == runs[0][2] for run in runs), f'non-deterministic signature: {name}/{mode}'
        rows = [r for run in runs for r in run[0]]
        nodes = sum(int(r['nodes']) for r in rows)
        seconds = sum(int(r['elapsed_ns']) for r in rows) / 1e9
        depths = [int(r['depth']) for r in rows]
        report['summary'][f'{name}/{mode}'] = dict(actual_nodes=nodes, search_s=seconds,
            actual_nps=round(nodes / seconds),
            process_cpu_s=sum(run[1]['child_user_s'] + run[1]['child_system_s'] for run in runs),
            peak_rss_kib=max(run[1]['max_child_rss_kib'] for run in runs),
            depth_min=min(depths), depth_median=statistics.median(depths), depth_max=max(depths),
            depth_distribution=dict(sorted(collections.Counter(depths).items())),
            all_repeats_same_signature=True,
            signature_matches_baseline=runs[0][2] == results[baseline, mode][0][2],
            all_returned_pvs_checked_legal=True)
    (output / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
