#!/usr/bin/env python3
"""Play paired fixed-depth games between two UCI engines using python-chess."""
import argparse
import hashlib
import json
import os
import platform
import time

import chess
import chess.engine
from uci_preflight import verify_farzin_network


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("engine_a", help="path to first UCI executable")
    parser.add_argument("engine_b", help="path to second UCI executable")
    parser.add_argument("--pairs", type=int, default=2)
    limit = parser.add_mutually_exclusive_group()
    limit.add_argument("--depth", type=int, help="fixed depth per move (default: 3)")
    limit.add_argument("--movetime-ms", type=int, help="fixed time per move in milliseconds")
    parser.add_argument("--max-plies", type=int, default=200)
    parser.add_argument("--openings", help="UTF-8 file with one FEN per line; # starts a comment")
    parser.add_argument("--option-a", action="append", default=[], metavar="NAME=VALUE")
    parser.add_argument("--option-b", action="append", default=[], metavar="NAME=VALUE")
    parser.add_argument("--json", required=True, help="output report path")
    args = parser.parse_args()
    depth = args.depth if args.depth is not None else (None if args.movetime_ms else 3)
    if args.pairs < 1 or (depth is not None and depth < 1) or (args.movetime_ms is not None and args.movetime_ms < 1) or args.max_plies < 1:
        parser.error("pairs, search limit and max-plies must be positive")
    openings = [chess.STARTING_FEN]
    if args.openings:
        with open(args.openings, encoding="utf-8") as source:
            openings = [line.strip() for line in source if line.strip() and not line.lstrip().startswith("#")]
        if not openings:
            parser.error("openings file is empty")
        for fen in openings:
            if not chess.Board(fen).is_valid():
                parser.error(f"invalid opening FEN: {fen}")
    engines = [os.path.abspath(args.engine_a), os.path.abspath(args.engine_b)]
    options = []
    for raw_options in [args.option_a, args.option_b]:
        parsed = {}
        for option in raw_options:
            if "=" not in option or "\n" in option or "\r" in option:
                parser.error("engine options must be NAME=VALUE on one line")
            name, value = option.split("=", 1)
            if not name.strip():
                parser.error("option name cannot be empty")
            if any(existing.lower() == name.strip().lower() for existing in parsed):
                parser.error("duplicate engine option name")
            parsed[name.strip()] = value
        options.append(parsed)
    verification = []
    for engine, requested in zip(engines, options):
        network = next((value for key, value in requested.items() if key.lower() == "evalfile"), None)
        if network and network != "<empty>":
            checked = verify_farzin_network(engine, network)
            checked["network_sha256"] = sha256_file(network)
            verification.append(checked)
        else:
            verification.append(None)
    report = {"engines": engines, "engine_sha256": [sha256_file(path) for path in engines], "depth": depth, "movetime_ms": args.movetime_ms, "pairs": args.pairs,
              "max_plies": args.max_plies, "platform": platform.platform(), "options": options,
              "evalfile_preflight": verification, "games": []}
    for number in range(args.pairs * 2):
        white = number % 2
        fen = openings[(number // 2) % len(openings)]
        board = chess.Board(fen)
        searches = []
        start = time.monotonic()
        # Fresh processes ensure each game starts without cached state.
        with chess.engine.SimpleEngine.popen_uci(engines[0], timeout=10) as a:
            with chess.engine.SimpleEngine.popen_uci(engines[1], timeout=10) as b:
                players = [a, b]
                for player, requested in zip(players, options):
                    player.configure(requested)
                    # setoption may return before a large model is loaded; wait
                    # for isready so setup time is not charged to the first move.
                    player.ping()
                while not board.is_game_over(claim_draw=True) and len(board.move_stack) < args.max_plies:
                    player = white if board.turn == chess.WHITE else 1 - white
                    search_limit = chess.engine.Limit(depth=depth, time=args.movetime_ms / 1000 if args.movetime_ms else None)
                    move_start = time.monotonic()
                    result = players[player].play(board, search_limit, info=chess.engine.INFO_ALL)
                    elapsed_ms = (time.monotonic() - move_start) * 1000
                    if result.move is None or result.move not in board.legal_moves:
                        raise RuntimeError(f"engine {player} returned an illegal or missing move")
                    searches.append({"engine": player, "depth": result.info.get("depth"),
                                     "nodes": result.info.get("nodes"), "reported_time_s": result.info.get("time"),
                                     "elapsed_ms": round(elapsed_ms, 3)})
                    board.push(result.move)
        outcome = board.outcome(claim_draw=True)
        game = {"white_engine": white, "opening_fen": fen, "result": outcome.result() if outcome else "*",
                "termination": outcome.termination.name if outcome else "PLY_LIMIT",
                "plies": len(board.move_stack), "moves": [move.uci() for move in board.move_stack], "searches": searches,
                "elapsed_seconds": round(time.monotonic() - start, 3)}
        report["games"].append(game)
        print(f"game {number + 1}: white={white} result={game['result']} "
              f"termination={game['termination']} plies={game['plies']}", flush=True)
    with open(args.json, "w", encoding="utf-8") as output:
        json.dump(report, output, indent=2)
        output.write("\n")


if __name__ == "__main__":
    main()
