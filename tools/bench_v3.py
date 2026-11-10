#!/usr/bin/env python3
"""Repeated fixed-depth benchmark. Explicit source SHA avoids mislabelling old binaries."""
import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import shutil
import sys
import datetime
import subprocess
import tempfile
from bench import sha256_file


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('engine')
    parser.add_argument('--commit', required=True)
    parser.add_argument('--build-features', default='', help='Cargo features used to produce this binary')
    parser.add_argument('--depth', type=int, default=4)
    parser.add_argument('--repeats', type=int, default=5)
    parser.add_argument('--positions', default=str(Path(__file__).with_name('positions-v3.fen')))
    parser.add_argument('--option', action='append', default=[])
    parser.add_argument('--json', required=True)
    parser.add_argument('--network-kind', choices=['trained', 'synthetic', 'none'], default='none')
    args = parser.parse_args()
    if args.repeats < 3:
        parser.error('at least three repeats required')
    supplied = dict(option.split('=', 1) for option in args.option)
    if args.network_kind == 'none' and any(k.lower() == 'evalfile' and v != '<empty>' for k, v in supplied.items()):
        parser.error('declare --network-kind when using EvalFile')
    cpuinfo = Path('/proc/cpuinfo').read_text() if Path('/proc/cpuinfo').exists() else ''
    metadata = {key: next((line.split(':', 1)[1].strip() for line in cpuinfo.splitlines()
                          if line.startswith(key)), None) for key in ('model name', 'flags')}
    report = dict(commit=args.commit, timestamp_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  benchmark_tool_sha256=sha256_file(__file__),
                  uci_driver_sha256=sha256_file(Path(__file__).with_name("bench.py")),
                  rustc=subprocess.check_output([shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc'), '-Vv'], text=True) if shutil.which('rustc') or (Path.home() / '.cargo/bin/rustc').exists() else None,
                  build_profile='release', build_features=args.build_features, build_flags=os.environ.get('RUSTFLAGS', ''),
                  evaluator='classical' if args.network_kind == 'none' else 'caissa-v15',
                  network_sha256=None, threads=int(next((v for k,v in supplied.items() if k.lower() == 'threads'), '1')),
                  hash_mb=int(next((v for k,v in supplied.items() if k.lower() == 'hash'), '16')), binary_sha256=sha256_file(args.engine),
                  engine=str(Path(args.engine).resolve()), os=platform.platform(), hardware=metadata,
                  logical_cpus=os.cpu_count(), affinity=sorted(os.sched_getaffinity(0)) if hasattr(os, 'sched_getaffinity') else None,
                  cpu_quota=Path('/sys/fs/cgroup/cpu.max').read_text().strip() if Path('/sys/fs/cgroup/cpu.max').exists() else None,
                  depth=args.depth, repeats=args.repeats, network_kind=args.network_kind,
                  time_control={'type': 'fixed_depth', 'depth': args.depth},
                  positions_sha256=sha256_file(args.positions), options=args.option,
                  cycles_per_node=None, perf_status='hardware counters not collected', perf_available=shutil.which('perf') is not None,
                  runs=[])
    for repeat in range(args.repeats):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'run.json'
            command = [sys.executable, str(Path(__file__).with_name('bench.py')), args.engine,
                       '--depth', str(args.depth), '--positions', args.positions, '--json', str(output)]
            for option in ['Threads=1', 'Hash=16'] + args.option:
                # User settings replace these documented defaults.
                name = option.split('=', 1)[0].lower()
                if option in ('Threads=1', 'Hash=16') and any(o.split('=', 1)[0].lower() == name for o in args.option):
                    continue
                command += ['--option', option]
            subprocess.run(command, check=True, timeout=600, stdout=subprocess.DEVNULL)
            run = json.loads(output.read_text())
        for item in run['positions']:
            info = [line.split() for line in item['info'] if line.startswith('info depth ')][-1]
            item.update(nodes=int(info[info.index('nodes') + 1]), depth=int(info[2]),
                        score=' '.join(info[info.index('score') + 1:info.index('score') + 3]))
            item['wall_nps'] = item['nodes'] / (item['elapsed_ms'] / 1000)
        if 'network_sha256' in run:
            report['network_sha256'] = run['network_sha256']
        report['runs'].append(run)
        print(f'repeat {repeat + 1}/{args.repeats}', flush=True)
    totals = [sum(p['elapsed_ms'] for p in r['positions']) for r in report['runs']]
    report['summary'] = {'total_wall_ms_median': statistics.median(totals),
                         'total_wall_ms_min': min(totals), 'total_wall_ms_max': max(totals),
                         'total_wall_ms_stdev': statistics.stdev(totals),
                         'positions': []}
    for index, position in enumerate(report['runs'][0]['positions']):
        samples = [r['positions'][index] for r in report['runs']]
        signatures = {(s['nodes'], s['score'], s['bestmove']) for s in samples}
        report['summary']['positions'].append(dict(position=position['position'],
            wall_ms_median=statistics.median(s['elapsed_ms'] for s in samples),
            wall_ms_min=min(s['elapsed_ms'] for s in samples),
            wall_ms_max=max(s['elapsed_ms'] for s in samples), deterministic=len(signatures) == 1))
    Path(args.json).write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
