#!/usr/bin/env python3
"""Alternate engine versions on the same position suite to reduce temporal drift."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
from bench import sha256_file


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', action='append', nargs=3, metavar=('LABEL', 'COMMIT', 'BINARY'), required=True)
    parser.add_argument('--depth', type=int, default=4)
    parser.add_argument('--repeats', type=int, default=5)
    parser.add_argument('--positions', default=str(Path(__file__).with_name('positions-v3.fen')))
    parser.add_argument('--option', action='append', default=[])
    parser.add_argument('--json', required=True)
    args = parser.parse_args()
    if args.repeats < 3 or len({e[0] for e in args.engine}) != len(args.engine):
        parser.error('require >=3 repeats and unique engine labels')
    configs = {label: dict(commit=commit, binary=str(Path(binary).resolve()), binary_sha256=sha256_file(binary), runs=[])
               for label, commit, binary in args.engine}
    order = []
    for repeat in range(args.repeats):
        labels = list(configs)
        if repeat % 2:
            labels.reverse()
        order.append(labels)
        for label in labels:
            with tempfile.TemporaryDirectory() as directory:
                result = Path(directory) / 'run.json'
                cmd = [sys.executable, str(Path(__file__).with_name('bench.py')), configs[label]['binary'],
                       '--depth', str(args.depth), '--positions', args.positions, '--json', str(result)]
                settings = {'threads': ('Threads', '1'), 'hash': ('Hash', '16')}
                for option in args.option:
                    name, value = option.split('=', 1)
                    settings[name.lower()] = (name, value)
                for name, value in settings.values():
                    cmd += ['--option', f'{name}={value}']
                subprocess.run(cmd, check=True, timeout=600, stdout=subprocess.DEVNULL)
                run = json.loads(result.read_text())
            for item in run['positions']:
                info = [line.split() for line in item['info'] if line.startswith('info depth ')][-1]
                item.update(nodes=int(info[info.index('nodes') + 1]), depth=int(info[2]),
                            score=' '.join(info[info.index('score') + 1:info.index('score') + 3]))
            configs[label]['runs'].append(run)
            print(repeat + 1, label, sum(p['elapsed_ms'] for p in run['positions']), flush=True)
    baseline = next(iter(configs))
    ref_totals = [sum(p['elapsed_ms'] for p in r['positions']) for r in configs[baseline]['runs']]
    from compare_benchmarks import signature
    reference = signature(configs[baseline]['runs'][0])
    for label, config in configs.items():
        totals = [sum(p['elapsed_ms'] for p in r['positions']) for r in config['runs']]
        ratios = [before / after for before, after in zip(ref_totals, totals)]
        config['summary'] = dict(total_wall_ms_median=statistics.median(totals),
            total_wall_ms_min=min(totals), total_wall_ms_max=max(totals),
            total_wall_ms_stdev=statistics.stdev(totals),
            paired_speed_ratios_vs_first=ratios, paired_speed_ratio_median=statistics.median(ratios),
            all_search_signatures_equal_to_first=all(signature(r) == reference for r in config['runs']))
    report = dict(timestamp_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        tool_sha256=sha256_file(__file__), uci_driver_sha256=sha256_file(Path(__file__).with_name('bench.py')),
        os=platform.platform(), cpuinfo=Path('/proc/cpuinfo').read_text().split('\n\n')[0],
        cpu_quota=Path('/sys/fs/cgroup/cpu.max').read_text().strip(),
        affinity=sorted(os.sched_getaffinity(0)), depth=args.depth, repeats=args.repeats,
        positions_sha256=sha256_file(args.positions), options=settings, order=order,
        baseline=baseline, configurations=configs, note='Throughput experiment, not a playing-strength match')
    Path(args.json).write_text(json.dumps(report, indent=2) + '\n')
    if not all(c['summary']['all_search_signatures_equal_to_first'] for c in configs.values()):
        raise RuntimeError('search signatures changed; see report')


if __name__ == '__main__':
    main()
