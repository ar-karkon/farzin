#!/usr/bin/env python3
"""Optional local-model comparison with independent Python scalar format mathematics.

Requires python-chess. This does not replace validation against the Caissa executable.
No model is downloaded. Use nonzero output weights for meaningful raw comparisons.
"""
import argparse
import json
import os
from pathlib import Path
import random
import subprocess
import chess
from bench import sha256_file
from compare_caissa_accumulators import scalar_accumulator, scalar_output
from inspect_nnue import validate_header


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('network')
    parser.add_argument('probe')
    parser.add_argument('--plies', type=int, default=40)
    parser.add_argument('--seed', type=int, default=2026)
    parser.add_argument('--json', required=True)
    parser.add_argument('--network-kind', choices=['synthetic', 'trained', 'unverified'], default='unverified')
    args = parser.parse_args()
    if not 0 <= args.plies <= 200:
        parser.error('plies must be between 0 and 200')
    rng = random.Random(args.seed)
    board = chess.Board()
    cases = [('initial', board.fen())]
    for ply in range(args.plies):
        legal = list(board.legal_moves)
        if not legal:
            break
        board.push(rng.choice(legal))
        cases.append((f'random-{ply + 1}', board.fen()))
    for side, rank in [('w', '1'), ('b', '8')]:
        for target in ['g', 'c']:
            board = chess.Board(f'r3k2r/8/8/8/8/8/8/R3K2R {side} KQkq - 0 1')
            board.push_uci(f'e{rank}{target}{rank}')
            cases.append((f'castle-{side}-{target}', board.fen()))
    for promotion in 'qrbn':
        board = chess.Board('4k3/P7/8/8/8/8/8/4K3 w - - 0 1')
        board.push_uci('a7a8' + promotion)
        cases.append(('promotion-' + promotion, board.fen()))
    for name, fen, moves in [
        ('en-passant', '4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1', ['e5d6']),
        ('king-mirror', '4k3/8/8/8/8/8/8/3K4 w - - 0 1', ['d1e1']),
        ('successive-captures', '4k3/8/2p5/3p4/4P3/8/8/4K3 w - - 0 1', ['e4d5', 'c6d5']),
    ]:
        board = chess.Board(fen)
        for move in moves:
            board.push_uci(move)
        cases.append((name, board.fen()))
    report = dict(network_kind=args.network_kind, reference='independent Python scalar mathematics; not Caissa executable',
                  network_sha256=sha256_file(args.network), probe_sha256=sha256_file(args.probe),
                  reference_sha256=sha256_file(Path(__file__).with_name('compare_caissa_accumulators.py')),
                  seed=args.seed, cases=[])
    with open(args.network, 'rb') as stream:
        validate_header(stream.read(64), os.fstat(stream.fileno()).st_size)
        for name, fen in cases:
            board = chess.Board(fen)
            accumulators = [scalar_accumulator(stream, board, perspective) for perspective in (board.turn, not board.turn)]
            expected = [scalar_output(stream, accumulators, bucket) for bucket in range(8)]
            outputs = {}
            for backend in ('scalar', 'simd-requested'):
                env = os.environ.copy()
                env.pop('FARZIN_PROBE_SIMD', None)
                if backend == 'simd-requested':
                    env['FARZIN_PROBE_SIMD'] = '1'
                result = subprocess.run([str(Path(args.probe).resolve()), str(Path(args.network).resolve()), fen],
                                        capture_output=True, text=True, check=True, timeout=30, env=env)
                actual = [int(line.split('raw=')[1]) for line in result.stdout.splitlines() if line.startswith('bucket=')]
                if actual != expected:
                    raise RuntimeError(f'{backend} raw output mismatch at {name}: {fen}')
                outputs[backend] = actual
            report['cases'].append(dict(name=name, fen=fen, outputs=outputs))
    Path(args.json).write_text(json.dumps(report, indent=2) + '\n')
    print(f'{len(cases)} positions: all eight raw outputs match both requested backends')


if __name__ == '__main__':
    main()
