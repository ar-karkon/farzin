#!/usr/bin/env python3
"""Play paired fixed-depth games between two UCI engines using python-chess."""
import argparse
import json
import os
import platform
import time

import chess
import chess.engine


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("engine_a", help="path to first UCI executable")
    parser.add_argument("engine_b", help="path to second UCI executable")
    parser.add_argument("--pairs", type=int, default=2)
    parser.add_argument("--depth", type=int, default=3)
    parser.add_argument("--max-plies", type=int, default=200)
    parser.add_argument("--json", required=True, help="output report path")
    args = parser.parse_args()
    if args.pairs < 1 or args.depth < 1 or args.max_plies < 1:
        parser.error("pairs, depth and max-plies must be positive")
    engines = [os.path.abspath(args.engine_a), os.path.abspath(args.engine_b)]
    report = {"engines": engines, "depth": args.depth, "pairs": args.pairs,
              "max_plies": args.max_plies, "platform": platform.platform(), "games": []}
    for number in range(args.pairs * 2):
        white = number % 2
        board = chess.Board()
        start = time.monotonic()
        # Fresh processes ensure each game starts without cached state.
        with chess.engine.SimpleEngine.popen_uci(engines[0], timeout=10) as a:
            with chess.engine.SimpleEngine.popen_uci(engines[1], timeout=10) as b:
                players = [a, b]
                while not board.is_game_over(claim_draw=True) and board.ply() < args.max_plies:
                    player = white if board.turn == chess.WHITE else 1 - white
                    result = players[player].play(board, chess.engine.Limit(depth=args.depth))
                    if result.move is None or result.move not in board.legal_moves:
                        raise RuntimeError(f"engine {player} returned an illegal or missing move")
                    board.push(result.move)
        outcome = board.outcome(claim_draw=True)
        game = {"white_engine": white, "result": outcome.result() if outcome else "*",
                "termination": outcome.termination.name if outcome else "PLY_LIMIT",
                "plies": board.ply(), "moves": [move.uci() for move in board.move_stack],
                "elapsed_seconds": round(time.monotonic() - start, 3)}
        report["games"].append(game)
        print(f"game {number + 1}: white={white} result={game['result']} "
              f"termination={game['termination']} plies={game['plies']}", flush=True)
    with open(args.json, "w", encoding="utf-8") as output:
        json.dump(report, output, indent=2)
        output.write("\n")


if __name__ == "__main__":
    main()
