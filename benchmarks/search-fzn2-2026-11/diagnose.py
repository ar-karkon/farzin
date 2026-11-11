#!/usr/bin/env python3
"""Alternate two engines over a FEN suite; record tree shape, CPU, RSS and legal PVs.

Requires python-chess. Run without concurrent matches for throughput comparisons.
CPU and peak RSS readings use Linux /proc and exclude this Python driver.
"""
import argparse
import hashlib
import json
from pathlib import Path
import os
import platform
import subprocess
import time

import chess
import chess.engine


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def process_usage(engine):
    root = Path('/proc') / str(engine.transport.get_pid())
    fields = (root / 'stat').read_text().rpartition(')')[2].split()
    cpu = (int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK')
    peak = next(int(line.split()[1]) for line in (root / 'status').read_text().splitlines()
                if line.startswith('VmHWM:'))
    return cpu, peak


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', required=True)
    parser.add_argument('--candidate', required=True)
    parser.add_argument('--network', required=True)
    parser.add_argument('--positions', default='tools/positions-v3.fen')
    parser.add_argument('--nodes', type=int, default=100000)
    parser.add_argument('--depth', type=int, help='fixed depth instead of fixed nodes, for throughput')
    parser.add_argument('--threads', type=int, default=1)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    network = str(Path(args.network).resolve())
    paths = {name: str(Path(path).resolve()) for name, path in
             [('baseline', args.baseline), ('candidate', args.candidate)]}
    positions = [line.strip() for line in Path(args.positions).read_text().splitlines()
                 if line.strip() and not line.startswith('#')]
    engines = {}
    report = dict(nodes=args.nodes if args.depth is None else None, depth=args.depth,
                  threads=args.threads, repeats=args.repeats,
                  platform=platform.platform(), network_sha256=sha256(network),
                  positions_sha256=sha256(args.positions),
                  binaries={name: dict(path=path, sha256=sha256(path)) for name, path in paths.items()},
                  note='nodes_reported is the final completed iteration; at a hard node limit, '
                       'work in the interrupted iteration is absent from UCI info. Use fixed '
                       'depth for throughput. CPU readings have Linux clock-tick granularity.',
                  records=[])
    try:
        for name, path in paths.items():
            commands = f'uci\nsetoption name EvalFile value {network}\nisready\nquit\n'
            setup = subprocess.run([path], input=commands, capture_output=True, text=True,
                                   check=True, timeout=30).stdout
            if f'loaded network {network}: FZN2 ' not in setup:
                raise RuntimeError(f'{name} did not confirm the requested FZN2 network')
            engine = engines[name] = chess.engine.SimpleEngine.popen_uci(path)
            engine.configure({'EvalFile': network, 'Hash': 16, 'Threads': args.threads})
        for repeat in range(args.repeats):
            for index, fen in enumerate(positions):
                board = chess.Board() if fen == 'startpos' else chess.Board(fen.removeprefix('fen '))
                order = ['baseline', 'candidate'] if (repeat + index) % 2 == 0 else ['candidate', 'baseline']
                for name in order:
                    engine = engines[name]
                    # Each position starts a fresh game/table/history for both engines.
                    engine.protocol.send_line('ucinewgame')
                    engine.ping()
                    cpu_before, _ = process_usage(engine)
                    start = time.perf_counter()
                    iterations = []
                    final = {}
                    limit = chess.engine.Limit(depth=args.depth) if args.depth is not None else chess.engine.Limit(nodes=args.nodes)
                    with engine.analysis(board, limit,
                                         game=object(), info=chess.engine.INFO_ALL) as analysis:
                        for info in analysis:
                            final.update(info)
                            pv = info.get('pv')
                            if pv:
                                check = board.copy()
                                for move in pv:
                                    if move not in check.legal_moves:
                                        raise RuntimeError(f'{name} illegal PV: {fen}, {pv}')
                                    check.push(move)
                                iterations.append(dict(depth=info.get('depth'),
                                    seldepth=info.get('seldepth'), nodes=info.get('nodes'),
                                    score=info['score'].pov(board.turn).score(mate_score=32000),
                                    pv=[m.uci() for m in pv],
                                    lowerbound=info.get('lowerbound', False),
                                    upperbound=info.get('upperbound', False)))
                    wall = time.perf_counter() - start
                    cpu_after, peak = process_usage(engine)
                    report['records'].append(dict(engine=name, repeat=repeat, position=fen,
                        nodes_reported=final.get('nodes'), depth=final.get('depth'),
                        seldepth=final.get('seldepth'), cpu_s=cpu_after-cpu_before,
                        wall_s=wall, max_rss_kib=peak, iterations=iterations))
    finally:
        for engine in engines.values():
            engine.quit()
    Path(args.out).write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
