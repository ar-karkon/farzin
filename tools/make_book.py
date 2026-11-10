#!/usr/bin/env python3
"""Generate a balanced opening book (EPD) with a reference UCI engine.

Each opening is a random walk of --plies moves where every move is drawn
from the reference engine's MultiPV candidates within --window cp of the
best move; the final position is kept if a deeper search scores it within
--max-cp of equality. Output positions are unique.

  tools/make_book.py --engine /opt/engines/stockfish --count 3000 \
      --out tools/books/farzin-openings.epd
"""
import argparse
import random
from concurrent.futures import ThreadPoolExecutor

import chess
import chess.engine


def worker(args, seed, count):
    rng = random.Random(seed)
    eng = chess.engine.SimpleEngine.popen_uci(args.engine)
    eng.configure({"Threads": 1, "Hash": 16})
    out = []
    try:
        while len(out) < count:
            board = chess.Board()
            ok = True
            for _ in range(args.plies):
                infos = eng.analyse(board, chess.engine.Limit(depth=args.walk_depth), multipv=args.multipv)
                best = infos[0]["score"].relative.score(mate_score=100000)
                cands = [i["pv"][0] for i in infos
                         if "pv" in i and best - i["score"].relative.score(mate_score=100000) <= args.window]
                if not cands:
                    ok = False
                    break
                board.push(rng.choice(cands))
                if board.is_game_over():
                    ok = False
                    break
            if not ok:
                continue
            info = eng.analyse(board, chess.engine.Limit(depth=args.check_depth))
            cp = info["score"].white().score(mate_score=100000)
            if abs(cp) <= args.max_cp:
                out.append((board.epd(), cp))
    finally:
        eng.quit()
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engine", required=True)
    ap.add_argument("--count", type=int, default=3000)
    ap.add_argument("--plies", type=int, default=8)
    ap.add_argument("--multipv", type=int, default=5)
    ap.add_argument("--window", type=int, default=50)
    ap.add_argument("--walk-depth", type=int, default=8)
    ap.add_argument("--check-depth", type=int, default=14)
    ap.add_argument("--max-cp", type=int, default=60)
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--seed", type=int, default=2026)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    per = args.count // args.workers + 1
    with ThreadPoolExecutor(args.workers) as ex:
        parts = list(ex.map(lambda i: worker(args, args.seed * 1000 + i, per), range(args.workers)))
    seen = {}
    for part in parts:
        for epd, cp in part:
            seen.setdefault(epd, cp)
    lines = list(seen.items())[: args.count]
    with open(args.out, "w") as f:
        f.write(f"# {len(lines)} openings, {args.plies} plies, MultiPV {args.multipv} window {args.window}cp, "
                f"|eval| <= {args.max_cp}cp at depth {args.check_depth}, seed {args.seed}\n")
        for epd, cp in lines:
            f.write(f"{epd} ; ce {cp}\n")
    print(f"wrote {len(lines)} openings")


if __name__ == "__main__":
    main()
