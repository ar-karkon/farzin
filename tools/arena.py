#!/usr/bin/env python3
"""Concurrent UCI engine match runner with clocks, paired openings, SPRT.

Each opening is played twice with colours swapped ("game pair"), so results
can be summarised with the pentanomial model. Requires python-chess.

Engine spec:  name=NAME,cmd=PATH[,nodes=N][,opt.KEY=VALUE...]
Examples:
  tools/arena.py --engine name=new,cmd=target/release/farzin \\
      --engine name=sf1800,cmd=/opt/engines/stockfish,opt.UCI_LimitStrength=true,opt.UCI_Elo=1800 \\
      --tc 10+0.1 --pairs 100 --concurrency 4 --book tools/books/farzin-openings.epd \\
      --out results/run.json

Modes: with exactly two engines a head-to-head match (optionally SPRT);
with more, --gauntlet makes the first engine play all others, otherwise a
round robin is played.

Limits: clock (--tc), fixed nodes (--nodes, or per engine nodes=N), or
fixed time per move (--movetime seconds).

Adjudication (--adjudicate):
  none          every game is played to mate / stalemate / repetition /
                50-move rule / insufficient material (max-plies still applies)
  conservative  resign only when BOTH engines agree on >= 1500 cp for 5
                consecutive moves each; draw only after move 60 when both
                report |score| <= 5 for 12 moves each
  default       resign >= --resign-cp for --resign-moves moves, draw after
                --draw-after when |score| <= --draw-cp for --draw-moves moves
The summary reports how many games ended naturally.
"""
import argparse
import hashlib
import itertools
import json
import math
import os
import platform
import random
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed, CancelledError

try:  # python-chess is only needed to actually play games
    import chess
    import chess.engine
    import chess.pgn
except ImportError:  # pragma: no cover - exercised in CI without python-chess
    chess = None


ADJUDICATION_PRESETS = {
    "none": None,
    "conservative": dict(resign_cp=1500, resign_moves=5, draw_cp=5, draw_moves=12, draw_after=60),
}


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
    if "nodes" in out:
        out["nodes"] = int(out["nodes"])
    return out


def parse_tc(text):
    """'10+0.1' -> (10.0, 0.1); '40/900' or '40/900+0' (repeating moves
    per control, as CCRL 40/15) -> (900.0, 0.0, 40)."""
    mtg = None
    if "/" in text:
        mtg, text = text.split("/", 1)
        mtg = int(mtg)
    if "+" in text:
        base, inc = text.split("+")
    else:
        base, inc = text, "0"
    if mtg is not None:
        return float(base), float(inc), mtg
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

    def __init__(self, specs, hash_mb, threads=1):
        self.specs = specs
        self.hash_mb = hash_mb
        self.threads = threads
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
            opts = {"Hash": self.hash_mb, "Threads": self.threads}
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


class Adjudicator:
    """Score-based adjudication from both engines' reported evaluations.

    ``scores`` holds the white-relative score reported by the side that made
    each ply (None when no score was reported)."""

    def __init__(self, resign_cp, resign_moves, draw_cp, draw_moves, draw_after):
        self.resign_cp = resign_cp
        self.resign_moves = resign_moves
        self.draw_cp = draw_cp
        self.draw_moves = draw_moves
        self.draw_after = draw_after

    def check(self, scores, fullmove_number):
        n = len(scores)
        k = 2 * self.resign_moves
        if n >= k:
            last = scores[-k:]
            if all(s is not None for s in last):
                if all(s >= self.resign_cp for s in last):
                    return "1-0"
                if all(s <= -self.resign_cp for s in last):
                    return "0-1"
        k = 2 * self.draw_moves
        if fullmove_number >= self.draw_after and n >= k:
            last = scores[-k:]
            if all(s is not None and abs(s) <= self.draw_cp for s in last):
                return "1/2-1/2"
        return None


def make_adjudicator(args):
    if args.adjudicate == "none":
        return None
    if args.adjudicate == "conservative":
        return Adjudicator(**ADJUDICATION_PRESETS["conservative"])
    return Adjudicator(args.resign_cp, args.resign_moves, args.draw_cp, args.draw_moves, args.draw_after)


def move_limit(spec, clocks, tc, args):
    if spec.get("nodes") is not None:
        return chess.engine.Limit(nodes=spec["nodes"]), False
    if args.nodes is not None:
        return chess.engine.Limit(nodes=args.nodes), False
    if args.movetime is not None:
        return chess.engine.Limit(time=args.movetime), False
    base, inc = tc[0], tc[1]
    remaining = None
    if len(tc) > 2:
        remaining = tc[2] - clocks.get("moves", 0) % tc[2]
    return chess.engine.Limit(
        white_clock=clocks[chess.WHITE], black_clock=clocks[chess.BLACK],
        white_inc=inc, black_inc=inc, remaining_moves=remaining,
    ), True


def play_game(pool, white, black, fen, tc, args, adjudicator=None):
    board = chess.Board(fen)
    base, inc = tc[0], tc[1]
    mtg = tc[2] if len(tc) > 2 else None
    clocks = {chess.WHITE: base, chess.BLACK: base}
    moves_done = {chess.WHITE: 0, chess.BLACK: 0}
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
    stats = {"plies": 0, "time": {white: 0.0, black: 0.0}, "moves": {white: 0, black: 0},
             "depth": {white: 0, black: 0}, "nodes": {white: 0, black: 0},
             # clock safety: smallest clock left after a move (s) and the
             # largest share of the available clock spent on one move
             "min_clock": {white: None, black: None}, "max_share": {white: 0.0, black: 0.0}}
    while result is None:
        if board.is_game_over(claim_draw=True):
            outcome = board.outcome(claim_draw=True)
            result = outcome.result()
            reason = outcome.termination.name.lower()
            break
        side = board.turn
        name = names[side]
        eng = pool.get(name)
        clocks["moves"] = moves_done[side]
        limit, clocked = move_limit(pool.specs[name], clocks, tc, args)
        start = time.perf_counter()
        try:
            res = eng.play(board, limit, info=chess.engine.INFO_SCORE | chess.engine.INFO_BASIC)
        except Exception as e:  # crash, protocol error or hang: loss for that side
            pool.drop(name)
            result = "0-1" if side == chess.WHITE else "1-0"
            reason = f"crash: {type(e).__name__}: {e}"
            break
        elapsed = time.perf_counter() - start
        stats["time"][name] += elapsed
        stats["moves"][name] += 1
        stats["depth"][name] += res.info.get("depth", 0) or 0
        stats["nodes"][name] += res.info.get("nodes", 0) or 0
        if clocked:
            if clocks[side] > 0:
                stats["max_share"][name] = max(stats["max_share"][name], elapsed / clocks[side])
            clocks[side] -= elapsed
            left = stats["min_clock"][name]
            stats["min_clock"][name] = clocks[side] if left is None else min(left, clocks[side])
            if clocks[side] < -args.time_margin:
                result = "0-1" if side == chess.WHITE else "1-0"
                reason = "time forfeit"
                break
            clocks[side] = max(clocks[side], 0.0) + inc
            moves_done[side] += 1
            if mtg is not None and moves_done[side] % mtg == 0:
                clocks[side] += base
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
        if adjudicator is not None:
            r = adjudicator.check(scores, board.fullmove_number)
            if r is not None:
                result, reason = r, "adjudication"
        if result is None and board.ply() >= args.max_plies:
            result, reason = "1/2-1/2", "max plies"
    stats["plies"] = len(scores)
    game.headers["Result"] = result
    game.headers["Termination"] = reason
    game.headers["FEN"] = fen
    game.headers["SetUp"] = "1"
    return result, reason, str(game), stats


# ---------------------------------------------------------------------------
# Statistics


def elo_from_score(s):
    s = min(max(s, 1e-6), 1 - 1e-6)
    return -400 * math.log10(1 / s - 1)


def pentanomial_counts(pairs):
    return [sum(1 for p in pairs if abs(p - k / 2) < 1e-9) for k in range(5)]


def pentanomial_summary(pairs):
    """pairs: list of pair scores for engine A in {0,0.5,1,1.5,2}.

    Returns logistic Elo with a 95% interval from the pentanomial variance
    and normalized Elo (nElo, Elo divided by the per-pair standard deviation
    and scaled as in fishtest), which is comparable across time controls and
    draw rates."""
    n = len(pairs)
    if n == 0:
        return {}
    counts = pentanomial_counts(pairs)
    mean = sum(pairs) / (2 * n)
    var = sum((p / 2 - mean) ** 2 for p in pairs) / n
    if n > 1 and var > 0:
        se = math.sqrt(var / n)
    else:
        # Degenerate sample (all pairs identical): fall back to the
        # variance of a pair with one decisive game, so the interval is
        # honest about the tiny sample instead of collapsing to a point.
        se = math.sqrt(0.0625 / n)
    lo, hi = mean - 1.96 * se, mean + 1.96 * se
    out = {
        "pairs": n,
        "pentanomial": counts,
        "score": mean,
        "elo": elo_from_score(mean),
        "elo_95": [elo_from_score(lo), elo_from_score(hi)],
        "los": 0.5 * (1 + math.erf((mean - 0.5) / (se * math.sqrt(2)))),
    }
    if var > 0:
        # Fishtest convention: per-game sigma = sqrt(2 * var of pair score/2).
        sigma = math.sqrt(2 * var)
        k = 800 / math.log(10)
        out["nelo"] = (mean - 0.5) / sigma * k
        out["nelo_95"] = [(lo - 0.5) / sigma * k, (hi - 0.5) / sigma * k]
    return out


def sprt_llr(pairs, elo0, elo1):
    """Pentanomial GSPRT log-likelihood ratio (normal approximation,
    logistic Elo bounds)."""
    n = len(pairs)
    if n < 2:
        return 0.0
    # A small symmetric prior keeps the variance estimate sane for tiny samples.
    counts = [c + 0.5 for c in pentanomial_counts(pairs)]
    total = sum(counts)
    probs = [c / total for c in counts]
    xs = [k / 4 for k in range(5)]
    mean = sum(p * x for p, x in zip(probs, xs))
    var = sum(p * (x - mean) ** 2 for p, x in zip(probs, xs))
    s0 = 1 / (1 + 10 ** (-elo0 / 400))
    s1 = 1 / (1 + 10 ** (-elo1 / 400))
    return n * (s1 - s0) * (2 * mean - s0 - s1) / (2 * var) if var > 0 else 0.0


def sprt_bounds(alpha, beta):
    return math.log(beta / (1 - alpha)), math.log((1 - beta) / alpha)


def score_for(a, white, r):
    if r == "1/2-1/2":
        return 0.5
    white_won = r == "1-0"
    return 1.0 if (white == a) == white_won else 0.0


# ---------------------------------------------------------------------------
# Scheduling


def run_jobs(jobs, worker, on_result, concurrency, stop):
    """Run ``worker(job)`` for every job on a thread pool and feed results
    to ``on_result(job, result)`` in completion order.

    ``stop`` is a threading.Event; once set (by ``on_result`` or anyone
    else), pending jobs are cancelled and in-flight jobs are allowed to
    finish (their results are still delivered so no played game is lost).
    Cancelled futures are skipped instead of raising CancelledError, and a
    worker that raises is reported as an error instead of aborting the run.
    Returns a dict with counts of completed / cancelled / failed jobs."""
    counts = {"completed": 0, "cancelled": 0, "failed": 0, "skipped": 0, "errors": []}
    with ThreadPoolExecutor(max_workers=max(1, concurrency)) as ex:
        futures = {ex.submit(worker, j): j for j in jobs}
        cancelled_all = False
        for fut in as_completed(futures):
            if fut.cancelled():
                counts["cancelled"] += 1
                continue
            try:
                res = fut.result()
            except CancelledError:  # cancelled between the check and result()
                counts["cancelled"] += 1
                continue
            except Exception as e:  # a crashed worker must not kill the match
                counts["failed"] += 1
                counts["errors"].append(f"{type(e).__name__}: {e}")
                continue
            if res is None:
                counts["skipped"] += 1
            else:
                counts["completed"] += 1
                on_result(futures[fut], res)
            if stop.is_set() and not cancelled_all:
                for f in futures:
                    f.cancel()
                cancelled_all = True
    return counts


def hardware_info():
    info = {"platform": platform.platform(), "python": platform.python_version(),
            "cpus": os.cpu_count()}
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    info["cpu"] = line.split(":", 1)[1].strip()
                    break
    except OSError:
        pass
    try:
        info["loadavg_start"] = os.getloadavg()
    except (OSError, AttributeError):  # not available on Windows
        pass
    return info


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--engine", action="append", required=True)
    ap.add_argument("--tc", default="10+0.1")
    ap.add_argument("--nodes", type=int, default=None, help="fixed nodes per move instead of clocks")
    ap.add_argument("--movetime", type=float, default=None, help="fixed seconds per move instead of clocks")
    ap.add_argument("--pairs", type=int, default=50, help="game pairs per engine pairing")
    ap.add_argument("--concurrency", type=int, default=os.cpu_count())
    ap.add_argument("--threads", type=int, default=1, help="UCI Threads for every engine")
    ap.add_argument("--book", default=os.path.join(os.path.dirname(__file__), "books", "farzin-openings.epd"))
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--hash", type=int, default=16)
    ap.add_argument("--gauntlet", action="store_true")
    ap.add_argument("--sprt", nargs=2, type=float, metavar=("ELO0", "ELO1"))
    ap.add_argument("--sprt-min-pairs", type=int, default=20)
    ap.add_argument("--alpha", type=float, default=0.05)
    ap.add_argument("--beta", type=float, default=0.05)
    ap.add_argument("--adjudicate", choices=["default", "conservative", "none"], default="default")
    ap.add_argument("--resign-cp", type=int, default=1000)
    ap.add_argument("--resign-moves", type=int, default=4)
    ap.add_argument("--draw-cp", type=int, default=8)
    ap.add_argument("--draw-moves", type=int, default=10)
    ap.add_argument("--draw-after", type=int, default=40)
    ap.add_argument("--max-plies", type=int, default=400)
    ap.add_argument("--time-margin", type=float, default=0.25)
    ap.add_argument("--out", default=None, help="JSON results file")
    ap.add_argument("--pgn", default=None)
    args = ap.parse_args(argv)
    if chess is None:
        raise SystemExit("arena.py needs python-chess: pip install chess")

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

    adjudicator = make_adjudicator(args)
    pool = EnginePool(by_name, args.hash, args.threads)
    results = {f"{a} vs {b}": {} for a, b in pairings}
    games = []
    terminations = {}
    engine_stats = {n: {"moves": 0, "time": 0.0, "depth": 0, "nodes": 0} for n in names}
    stop = threading.Event()
    pgn_file = open(args.pgn, "a") if args.pgn else None
    started = time.time()
    sprt_state = {"llr": 0.0, "decision": None}
    lower, upper = sprt_bounds(args.alpha, args.beta)
    hw = hardware_info()

    def run_pair(job):
        a, b, k, fen = job
        if stop.is_set():
            return None
        out = []
        for white, black in ((a, b), (b, a)):
            out.append((white, black) + play_game(pool, white, black, fen, tc, args, adjudicator))
        return out

    def save(final=False, sched=None):
        summary = {}
        for key, pair_map in results.items():
            summary[key] = pentanomial_summary(list(pair_map.values()))
        n_games = len(games)
        natural = sum(v for k, v in terminations.items()
                      if k not in ("adjudication", "max plies", "time forfeit") and not k.startswith(("crash", "illegal")))
        per_engine = {}
        for n, s in engine_stats.items():
            if s["moves"]:
                per_engine[n] = {"avg_depth": round(s["depth"] / s["moves"], 2),
                                 "avg_ms_per_move": round(1000 * s["time"] / s["moves"], 1),
                                 "nps": round(s["nodes"] / s["time"]) if s["time"] > 0 else None,
                                 "min_clock_s": None if s.get("min_clock") is None else round(s["min_clock"], 3),
                                 "max_move_share": round(s.get("max_share", 0.0), 3)}
        payload = {
            "engines": {n: {"cmd": s["cmd"], "options": s["options"], "nodes": s.get("nodes"),
                            "sha256": sha256(s["cmd"].split()[0])}
                        for n, s in by_name.items()},
            "tc": args.tc, "nodes": args.nodes, "movetime": args.movetime,
            "book": os.path.basename(args.book), "book_sha256": sha256(args.book),
            "seed": args.seed, "concurrency": args.concurrency, "threads": args.threads, "hash": args.hash,
            "adjudication": args.adjudicate if args.adjudicate != "default" else {
                "resign_cp": args.resign_cp, "resign_moves": args.resign_moves, "draw_cp": args.draw_cp,
                "draw_moves": args.draw_moves, "draw_after": args.draw_after},
            "hardware": hw,
            "elapsed_s": round(time.time() - started, 1),
            "summary": summary, "sprt": sprt_state if args.sprt else None,
            "terminations": terminations,
            "natural_fraction": round(natural / n_games, 3) if n_games else None,
            "engine_stats": per_engine,
            "games": [g[:4] for g in games],
        }
        if sched is not None:
            payload["scheduler"] = {k: v for k, v in sched.items() if k != "errors"}
            payload["scheduler_errors"] = sched["errors"][:20]
        if args.out:
            tmp = args.out + ".tmp"
            with open(tmp, "w") as f:
                json.dump(payload, f, indent=1)
            os.replace(tmp, args.out)
        return summary

    done = [0]

    def on_result(job, out):
        a, b, k, fen = job
        pair_score = 0.0
        for white, black, r, reason, pgn, st in out:
            games.append((white, black, r, reason))
            terminations[reason] = terminations.get(reason, 0) + 1
            for n in (white, black):
                es = engine_stats[n]
                es["moves"] += st["moves"][n]
                es["time"] += st["time"][n]
                es["depth"] += st["depth"][n]
                es["nodes"] += st["nodes"][n]
                mc = st.get("min_clock", {}).get(n)
                if mc is not None:
                    es["min_clock"] = mc if es.get("min_clock") is None else min(es["min_clock"], mc)
                es["max_share"] = max(es.get("max_share", 0.0), st.get("max_share", {}).get(n, 0.0))
            pair_score += score_for(a, white, r)
            if pgn_file:
                pgn_file.write(pgn + "\n\n")
        results[f"{a} vs {b}"][k] = pair_score
        done[0] += 1
        if args.sprt and sprt_state["decision"] is None:
            pairs = list(results[f"{a} vs {b}"].values())
            llr = sprt_llr(pairs, *args.sprt)
            sprt_state["llr"] = llr
            sprt_state["bounds"] = [lower, upper]
            sprt_state["pairs_at_decision"] = None
            if len(pairs) >= args.sprt_min_pairs:
                if llr >= upper:
                    sprt_state["decision"] = "H1 accepted"
                elif llr <= lower:
                    sprt_state["decision"] = "H0 accepted"
                if sprt_state["decision"]:
                    sprt_state["pairs_at_decision"] = len(pairs)
                    stop.set()
        summary = save()
        if done[0] % max(1, args.concurrency) == 0 or stop.is_set():
            line = " | ".join(
                f"{key}: {s.get('pairs', 0)}p {s.get('elo', 0):+.1f} [{s['elo_95'][0]:+.1f},{s['elo_95'][1]:+.1f}]"
                for key, s in summary.items() if s)
            if args.sprt:
                line += f" LLR {sprt_state['llr']:.2f} {sprt_state['decision'] or ''}"
            print(f"[{time.time() - started:7.0f}s] {line}", flush=True)

    sched = None
    try:
        sched = run_jobs(jobs, run_pair, on_result, args.concurrency, stop)
        if sched["errors"]:
            print(f"warning: {sched['failed']} pair(s) failed: {sched['errors'][:3]}", flush=True)
    finally:
        pool.close()
        if pgn_file:
            pgn_file.close()
    summary = save(final=True, sched=sched)
    print(json.dumps({"summary": summary, "sprt": sprt_state if args.sprt else None,
                      "terminations": terminations}, indent=1))


if __name__ == "__main__":
    main()
