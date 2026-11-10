#!/usr/bin/env python3
"""Concurrent UCI engine match runner with clocks, paired openings, SPRT.

Each opening is played twice with colours swapped ("game pair"), so results
can be summarised with the pentanomial model. Requires python-chess.

Engine spec:  name=NAME,cmd=PATH[,opt.KEY=VALUE...]
Examples:
  tools/arena.py --engine name=new,cmd=target/release/farzin \
      --engine name=sf1800,cmd=/opt/engines/stockfish,opt.UCI_LimitStrength=true,opt.UCI_Elo=1800 \
      --tc 10+0.1 --pairs 100 --concurrency 4 --book tools/books/farzin-openings.epd \
      --out results/run.json

Modes: with exactly two engines a head-to-head match (optionally SPRT);
with more, --gauntlet makes the first engine play all others, otherwise a
round robin is played.
"""
import argparse
import hashlib
import itertools
import json
import math
import os
import random
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed

import chess
import chess.engine
import chess.pgn


def parse_engine(spec):
    out = {"options": {}}
    for part in spec.split(","):
        key, _, value = part.partition("=")
        if key.startswith("opt."):
            out["options"][key[4:]] = value
        else:
            out[key] = value
    if "cmd" not in out:
        raise SystemExit(f"engine spec needs cmd=: {spec}")
    out.setdefault("name", os.path.basename(out["cmd"]))
    return out


def parse_tc(text):
    if "+" in text:
        base, inc = text.split("+")
    else:
        base, inc = text, "0"
    return float(base), float(inc)


def sha256(path):
    try:
        with open(path, "rb") as f:
            return hashlib.sha256(f.read()).hexdigest()
    except OSError:
        return None


def load_book(path):
    fens = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            fields = line.split()
            fen = " ".join(fields[:4])
            fens.append(fen + " 0 1")
    return fens


class EnginePool:
    """Per-thread engine processes, reused across games."""

    def __init__(self, specs, hash_mb):
        self.specs = specs
        self.hash_mb = hash_mb
        self.local = threading.local()
        self.all = []
        self.lock = threading.Lock()

    def get(self, name):
        cache = getattr(self.local, "engines", None)
        if cache is None:
            cache = self.local.engines = {}
        eng = cache.get(name)
        if eng is None:
            spec = self.specs[name]
            eng = chess.engine.SimpleEngine.popen_uci(spec["cmd"].split())
            opts = {"Hash": self.hash_mb, "Threads": 1}
            opts.update(spec["options"])
            for k, v in opts.items():
                if k in eng.options:
                    eng.configure({k: v})
            cache[name] = eng
            with self.lock:
                self.all.append(eng)
        return eng

    def drop(self, name):
        cache = getattr(self.local, "engines", {})
        eng = cache.pop(name, None)
        if eng is not None:
            try:
                eng.quit()
            except Exception:
                pass

    def close(self):
        for eng in self.all:
            try:
                eng.quit()
            except Exception:
                pass


def play_game(pool, white, black, fen, tc, args):
    board = chess.Board(fen)
    base, inc = tc
    clocks = {chess.WHITE: base, chess.BLACK: base}
    names = {chess.WHITE: white, chess.BLACK: black}
    for name in (white, black):
        eng = pool.get(name)
        try:
            eng.protocol.send_line("ucinewgame")
            eng.ping()
        except Exception:
            pool.drop(name)
            pool.get(name)
    game = chess.pgn.Game()
    game.setup(chess.Board(fen))
    game.headers["White"] = white
    game.headers["Black"] = black
    node = game
    scores = []  # white-relative cp per ply
    result, reason = None, None
    while result is None:
        if board.is_game_over(claim_draw=True):
            outcome = board.outcome(claim_draw=True)
            result = outcome.result()
            reason = outcome.termination.name.lower()
            break
        side = board.turn
        eng = pool.get(names[side])
        limit = chess.engine.Limit(
            white_clock=clocks[chess.WHITE], black_clock=clocks[chess.BLACK],
            white_inc=inc, black_inc=inc,
        ) if args.nodes is None else chess.engine.Limit(nodes=args.nodes)
        start = time.perf_counter()
        try:
            res = eng.play(board, limit, info=chess.engine.INFO_SCORE)
        except Exception as e:  # crash or protocol error: loss for that side
            pool.drop(names[side])
            result = "0-1" if side == chess.WHITE else "1-0"
            reason = f"crash: {e}"
            break
        elapsed = time.perf_counter() - start
        if args.nodes is None:
            clocks[side] -= elapsed
            if clocks[side] < -args.time_margin:
                result = "0-1" if side == chess.WHITE else "1-0"
                reason = "time forfeit"
                break
            clocks[side] = max(clocks[side], 0.0) + inc
        move = res.move
        if move is None or move not in board.legal_moves:
            result = "0-1" if side == chess.WHITE else "1-0"
            reason = f"illegal move {move}"
            break
        score = res.info.get("score")
        cp = None
        if score is not None:
            cp = score.white().score(mate_score=100000)
        scores.append(cp)
        board.push(move)
        node = node.add_variation(move)
        if cp is not None:
            node.comment = f"{cp / 100:+.2f}"
        # Adjudication.
        n = len(scores)
        if n >= 2 * args.resign_moves and all(s is not None for s in scores[-2 * args.resign_moves:]):
            last = scores[-2 * args.resign_moves:]
            if all(s >= args.resign_cp for s in last):
                result, reason = "1-0", "adjudication"
            elif all(s <= -args.resign_cp for s in last):
                result, reason = "0-1", "adjudication"
        if result is None and board.fullmove_number >= args.draw_after and n >= 2 * args.draw_moves:
            last = scores[-2 * args.draw_moves:]
            if all(s is not None and abs(s) <= args.draw_cp for s in last):
                result, reason = "1/2-1/2", "adjudication"
        if result is None and board.ply() >= args.max_plies:
            result, reason = "1/2-1/2", "max plies"
    game.headers["Result"] = result
    game.headers["Termination"] = reason
    game.headers["FEN"] = fen
    game.headers["SetUp"] = "1"
    return result, reason, str(game)


def elo_from_score(s):
    s = min(max(s, 1e-6), 1 - 1e-6)
    return -400 * math.log10(1 / s - 1)


def pentanomial_summary(pairs):
    """pairs: list of pair scores for engine A in {0,0.5,1,1.5,2}."""
    n = len(pairs)
    if n == 0:
        return {}
    counts = [sum(1 for p in pairs if abs(p - k / 2) < 1e-9) for k in range(5)]
    mean = sum(pairs) / (2 * n)
    var = sum((p / 2 - mean) ** 2 for p in pairs) / n
    se = math.sqrt(var / n) if n > 1 else 0.5
    lo, hi = mean - 1.96 * se, mean + 1.96 * se
    return {
        "pairs": n,
        "pentanomial": counts,
        "score": mean,
        "elo": elo_from_score(mean),
        "elo_95": [elo_from_score(lo), elo_from_score(hi)],
        "los": 0.5 * (1 + math.erf((mean - 0.5) / (se * math.sqrt(2)))) if se > 0 else None,
    }


def sprt_llr(pairs, elo0, elo1):
    """Pentanomial GSPRT log-likelihood ratio (normalized-free, logistic Elo)."""
    n = len(pairs)
    if n < 2:
        return 0.0
    counts = [sum(1 for p in pairs if abs(p - k / 2) < 1e-9) + 1e-3 for k in range(5)]
    total = sum(counts)
    probs = [c / total for c in counts]
    xs = [k / 4 for k in range(5)]
    mean = sum(p * x for p, x in zip(probs, xs))
    var = sum(p * (x - mean) ** 2 for p, x in zip(probs, xs))
    s0 = 1 / (1 + 10 ** (-elo0 / 400))
    s1 = 1 / (1 + 10 ** (-elo1 / 400))
    return n * (s1 - s0) * (2 * mean - s0 - s1) / (2 * var) if var > 0 else 0.0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--engine", action="append", required=True)
    ap.add_argument("--tc", default="10+0.1")
    ap.add_argument("--nodes", type=int, default=None, help="fixed nodes per move instead of clocks")
    ap.add_argument("--pairs", type=int, default=50, help="game pairs per engine pairing")
    ap.add_argument("--concurrency", type=int, default=os.cpu_count())
    ap.add_argument("--book", default=os.path.join(os.path.dirname(__file__), "books", "farzin-openings.epd"))
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--hash", type=int, default=16)
    ap.add_argument("--gauntlet", action="store_true")
    ap.add_argument("--sprt", nargs=2, type=float, metavar=("ELO0", "ELO1"))
    ap.add_argument("--alpha", type=float, default=0.05)
    ap.add_argument("--beta", type=float, default=0.05)
    ap.add_argument("--resign-cp", type=int, default=1000)
    ap.add_argument("--resign-moves", type=int, default=4)
    ap.add_argument("--draw-cp", type=int, default=8)
    ap.add_argument("--draw-moves", type=int, default=10)
    ap.add_argument("--draw-after", type=int, default=40)
    ap.add_argument("--max-plies", type=int, default=400)
    ap.add_argument("--time-margin", type=float, default=0.25)
    ap.add_argument("--out", default=None, help="JSON results file")
    ap.add_argument("--pgn", default=None)
    args = ap.parse_args()

    specs = [parse_engine(e) for e in args.engine]
    by_name = {s["name"]: s for s in specs}
    if len(by_name) != len(specs):
        raise SystemExit("engine names must be unique")
    names = [s["name"] for s in specs]
    if len(names) == 2:
        pairings = [(names[0], names[1])]
    elif args.gauntlet:
        pairings = [(names[0], n) for n in names[1:]]
    else:
        pairings = list(itertools.combinations(names, 2))
    book = load_book(args.book)
    rng = random.Random(args.seed)
    rng.shuffle(book)
    tc = parse_tc(args.tc)
    jobs = []
    for pi, (a, b) in enumerate(pairings):
        for k in range(args.pairs):
            fen = book[(k + pi * 7919) % len(book)]
            jobs.append((a, b, k, fen))
    rng.shuffle(jobs)

    pool = EnginePool(by_name, args.hash)
    results = {f"{a} vs {b}": {} for a, b in pairings}
    games = []
    lock = threading.Lock()
    stop = threading.Event()
    pgn_file = open(args.pgn, "a") if args.pgn else None
    started = time.time()
    sprt_state = {"llr": 0.0, "decision": None}
    lower = math.log(args.beta / (1 - args.alpha))
    upper = math.log((1 - args.beta) / args.alpha)

    def run_pair(job):
        a, b, k, fen = job
        if stop.is_set():
            return None
        out = []
        for white, black in ((a, b), (b, a)):
            r, reason, pgn = play_game(pool, white, black, fen, tc, args)
            out.append((white, black, r, reason, pgn))
        return job, out

    def score_for(a, white, r):
        if r == "1/2-1/2":
            return 0.5
        white_won = r == "1-0"
        return 1.0 if (white == a) == white_won else 0.0

    def save():
        summary = {}
        for key, pair_map in results.items():
            summary[key] = pentanomial_summary(list(pair_map.values()))
        payload = {
            "engines": {n: {"cmd": s["cmd"], "options": s["options"], "sha256": sha256(s["cmd"].split()[0])}
                        for n, s in by_name.items()},
            "tc": args.tc, "nodes": args.nodes, "book": os.path.basename(args.book),
            "seed": args.seed, "concurrency": args.concurrency,
            "elapsed_s": round(time.time() - started, 1),
            "summary": summary, "sprt": sprt_state if args.sprt else None,
            "games": [g[:4] for g in games],
        }
        if args.out:
            tmp = args.out + ".tmp"
            with open(tmp, "w") as f:
                json.dump(payload, f, indent=1)
            os.replace(tmp, args.out)
        return summary

    try:
        with ThreadPoolExecutor(max_workers=args.concurrency) as ex:
            futures = [ex.submit(run_pair, j) for j in jobs]
            done = 0
            for fut in as_completed(futures):
                res = fut.result()
                if res is None:
                    continue
                (a, b, k, fen), out = res
                with lock:
                    pair_score = 0.0
                    for white, black, r, reason, pgn in out:
                        games.append((white, black, r, reason))
                        pair_score += score_for(a, white, r)
                        if pgn_file:
                            pgn_file.write(pgn + "\n\n")
                    results[f"{a} vs {b}"][k] = pair_score
                    done += 1
                    if args.sprt:
                        pairs = list(results[f"{a} vs {b}"].values())
                        llr = sprt_llr(pairs, *args.sprt)
                        sprt_state["llr"] = llr
                        sprt_state["bounds"] = [lower, upper]
                        if llr >= upper:
                            sprt_state["decision"] = "H1 accepted"
                            stop.set()
                        elif llr <= lower:
                            sprt_state["decision"] = "H0 accepted"
                            stop.set()
                    summary = save()
                    if done % max(1, args.concurrency) == 0 or stop.is_set():
                        line = " | ".join(
                            f"{key}: {s.get('pairs', 0)}p {s.get('elo', 0):+.1f} [{s.get('elo_95', [0, 0])[0]:+.1f},{s.get('elo_95', [0, 0])[1]:+.1f}]"
                            for key, s in summary.items() if s)
                        if args.sprt:
                            line += f" LLR {sprt_state['llr']:.2f} {sprt_state['decision'] or ''}"
                        print(f"[{time.time() - started:7.0f}s] {line}", flush=True)
                if stop.is_set():
                    for f in futures:
                        f.cancel()
    finally:
        pool.close()
        if pgn_file:
            pgn_file.close()
    summary = save()
    print(json.dumps({"summary": summary, "sprt": sprt_state if args.sprt else None}, indent=1))


if __name__ == "__main__":
    main()
