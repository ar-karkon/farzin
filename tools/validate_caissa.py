#!/usr/bin/env python3
"""Compare Rust Caissa inference with a local reference over reproducible positions."""
import argparse
import hashlib
import json
import os
import random
import subprocess

import chess
from compare_caissa_accumulators import reference_accumulators


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("network")
    parser.add_argument("reference")
    parser.add_argument("probe", help="compiled Rust caissa_probe executable")
    parser.add_argument("--plies", type=int, default=12)
    parser.add_argument("--seed", type=int, default=2026)
    parser.add_argument("--json", required=True)
    args = parser.parse_args()
    if args.plies < 0 or args.plies > 200:
        parser.error("plies must be between 0 and 200")
    randomizer = random.Random(args.seed)
    board = chess.Board()
    moves = []
    cases = [("initial", chess.STARTING_FEN, [])]
    for ply in range(args.plies):
        legal = list(board.legal_moves)
        if not legal:
            break
        move = randomizer.choice(legal)
        moves.append(move.uci())
        board.push(move)
        cases.append((f"random-{ply + 1}", chess.STARTING_FEN, moves.copy()))
    cases.extend([
        ("castling", "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", ["e1g1", "e8c8"]),
        ("en-passant", "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", ["e5d6"]),
        ("underpromotion", "4k3/P7/8/8/8/8/8/4K3 w - - 0 1", ["a7a8n"]),
    ])
    report = {"seed": args.seed, "requested_plies": args.plies,
              "network_sha256": sha256_file(args.network),
              "reference_sha256": sha256_file(args.reference),
              "probe_sha256": sha256_file(args.probe), "cases": []}
    for label, fen, case_moves in cases:
        position = chess.Board(fen)
        for token in case_moves:
            position.push_uci(token)
        command = [os.path.abspath(args.probe), os.path.abspath(args.network), fen, *case_moves]
        completed = subprocess.run(command, capture_output=True, text=True, timeout=60, check=True)
        outputs = {}
        for line in completed.stdout.splitlines():
            if line.startswith("bucket="):
                bucket, raw = line.split()
                outputs[int(bucket.split("=")[1])] = int(raw.split("=")[1])
        _, expected = reference_accumulators(args.reference, args.network, position.fen())
        if outputs != expected:
            raise RuntimeError(f"network mismatch in {label}: {position.fen()}")
        report["cases"].append({"name": label, "initial_fen": fen, "moves": case_moves,
                                "final_fen": position.fen(), "outputs": outputs})
        print(f"{label}: 8 raw outputs matched; incremental refresh checks passed", flush=True)
    with open(args.json, "w", encoding="utf-8") as output:
        json.dump(report, output, indent=2)
        output.write("\n")


if __name__ == "__main__":
    main()
