#!/usr/bin/env python3
"""Generate a deterministic, de-duplicated opening suite from random legal plies.

Each opening is reached by `--plies` uniformly random legal moves from the
initial position using Python's `random.Random(seed)`. Positions that are
terminal, in check, or whose side to move is unbalanced in material by more
than `--max-material` centipawns are rejected. The result is a reproducible
starting suite for paired engine tests, not a curated human opening book:
random plies produce odd but legal positions.
"""
import argparse
import random

import chess

VALUES = {chess.PAWN: 100, chess.KNIGHT: 320, chess.BISHOP: 330, chess.ROOK: 500, chess.QUEEN: 900}


def material(board):
    score = 0
    for piece, value in VALUES.items():
        score += value * (len(board.pieces(piece, chess.WHITE)) - len(board.pieces(piece, chess.BLACK)))
    return score


def generate(count, plies, seed, max_material):
    rng = random.Random(seed)
    seen = set()
    openings = []
    attempts = 0
    while len(openings) < count:
        attempts += 1
        if attempts > count * 1000:
            raise RuntimeError("could not generate enough openings")
        board = chess.Board()
        for _ in range(plies):
            moves = list(board.legal_moves)
            if not moves:
                break
            board.push(rng.choice(moves))
        if board.is_game_over() or board.is_check() or abs(material(board)) > max_material:
            continue
        # Keep the FEN key without move counters so transpositions are unique.
        key = " ".join(board.fen().split()[:4])
        if key in seen:
            continue
        seen.add(key)
        openings.append(board.fen())
    return openings


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--count", type=int, default=400)
    parser.add_argument("--plies", type=int, default=8)
    parser.add_argument("--seed", type=int, default=2026)
    parser.add_argument("--max-material", type=int, default=0)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    if args.count < 1 or args.plies < 0 or args.max_material < 0:
        parser.error("count must be positive and plies/max-material non-negative")
    openings = generate(args.count, args.plies, args.seed, args.max_material)
    with open(args.output, "w", encoding="utf-8") as output:
        output.write(f"# make_openings.py --count {args.count} --plies {args.plies} "
                     f"--seed {args.seed} --max-material {args.max_material}\n")
        for fen in openings:
            output.write(fen + "\n")


if __name__ == "__main__":
    main()
