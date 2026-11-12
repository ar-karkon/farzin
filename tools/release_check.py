#!/usr/bin/env python3
"""Release conformance and stability checks for a Farzin binary.

    tools/release_check.py [--quick] [--expect-sha256 HEX] -- ENGINE_CMD...

ENGINE_CMD is the full command, e.g. `-- dist/farzin.exe` on Windows or
`-- wine dist/farzin.exe` under Wine. Talks raw UCI over pipes (so protocol
details are checked exactly) and uses python-chess only as the legality
oracle. Prints one line per check and a JSON summary; exit status 1 if any
check fails.
"""
import argparse
import json
import queue
import random
import subprocess
import sys
import threading
import time

import chess

PERFT = [  # (fen, depth, nodes) - standard published perft results
    ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 5, 4865609),
    ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", 4, 4085603),
    ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 5, 674624),
    ("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1", 4, 422333),
    ("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8", 4, 2103487),
    ("r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10", 4, 3894594),
]

SPECIAL = [  # positions exercising castling, en passant, promotion, checks
    ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", None),
    ("rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3", None),
    ("8/P7/8/8/8/8/k6p/4K3 w - - 0 1", None),
    ("8/1P6/8/8/8/8/p5k1/4K3 b - - 0 1", None),
    ("4k3/8/8/8/8/8/4q3/4K3 w - - 0 1", None),  # in check, few replies
    ("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", "0000"),  # stalemate
    ("6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1", "d1d8"),  # mate in 1
    ("8/8/8/8/8/8/6k1/4K2R w K - 0 1", None),
]


class Engine:
    def __init__(self, cmd):
        self.cmd = cmd
        self.p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=1)
        self.q = queue.Queue()
        self.log = []
        threading.Thread(target=self._reader, daemon=True).start()

    def _reader(self):
        for line in self.p.stdout:
            self.q.put(line.rstrip("\r\n"))
        self.q.put(None)

    def send(self, text):
        self.log.append("> " + text)
        self.p.stdin.write(text + "\n")
        self.p.stdin.flush()

    def until(self, prefix, timeout=30.0):
        lines, end = [], time.time() + timeout
        while True:
            left = end - time.time()
            if left <= 0:
                raise TimeoutError(f"no '{prefix}' within {timeout}s; last: {lines[-3:]}")
            try:
                line = self.q.get(timeout=left)
            except queue.Empty:
                continue
            if line is None:
                raise EOFError(f"engine exited while waiting for '{prefix}'")
            lines.append(line)
            if line.startswith(prefix):
                return lines

    def lines_for(self, seconds):
        lines, end = [], time.time() + seconds
        while time.time() < end:
            try:
                line = self.q.get(timeout=max(0.0, end - time.time()))
            except queue.Empty:
                break
            if line is None:
                break
            lines.append(line)
        return lines

    def ready(self, timeout=30.0):
        self.send("isready")
        return self.until("readyok", timeout)

    def quit(self, timeout=10.0):
        try:
            self.send("quit")
        except (BrokenPipeError, OSError):
            pass
        try:
            return self.p.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.p.kill()
            return None


def bestmove(lines):
    words = lines[-1].split()
    return words[1] if len(words) > 1 else None


def legal(fen, mv):
    board = chess.Board(fen)
    try:
        return chess.Move.from_uci(mv) in board.legal_moves
    except ValueError:
        return False


RESULTS = []


def check(name):
    def wrap(fn):
        def run(*a, **kw):
            t = time.time()
            try:
                detail = fn(*a, **kw) or ""
                RESULTS.append({"check": name, "ok": True, "detail": detail, "s": round(time.time() - t, 2)})
                print(f"PASS {name}: {detail}", flush=True)
            except Exception as e:  # noqa: BLE001 - report every failure
                RESULTS.append({"check": name, "ok": False, "detail": f"{type(e).__name__}: {e}",
                                "s": round(time.time() - t, 2)})
                print(f"FAIL {name}: {type(e).__name__}: {e}", flush=True)
        return run
    return wrap


@check("uci handshake")
def t_handshake(cmd, expect_sha, expect_name=None):
    e = Engine(cmd)
    t = time.time()
    e.send("uci")
    lines = e.until("uciok", 10)
    took = time.time() - t
    names = [l for l in lines if l.startswith("id name ")]
    assert names, "no id name"
    if expect_name:
        assert names == [f"id name {expect_name}"], names
    assert any(l.startswith("id author ") for l in lines), "no id author"
    opts = [l for l in lines if l.startswith("option name ")]
    for need in ("Hash", "Threads", "Move Overhead", "Ponder"):
        assert any(l.startswith(f"option name {need} type") for l in opts), f"missing option {need}"
    assert not any(l.startswith("option name tune_") for l in opts), "tuning options exposed"
    e.ready()
    e.quit()
    if expect_sha:
        out = subprocess.run(cmd + ["--version"], capture_output=True, text=True, timeout=60).stdout
        assert expect_sha in out, f"--version lacks network sha256 {expect_sha}: {out.strip()}"
        assert "FZN2" in out, out
    return f"{names[0][8:]}; {len(opts)} options; uciok after {took:.2f}s"


@check("options: set every advertised option")
def t_options(cmd):
    e = Engine(cmd)
    e.send("uci")
    e.until("uciok")
    for line in ["Hash value 1", "Hash value 1024", "Hash value 64", "Threads value 8",
                 "Threads value 1", "Move Overhead value 100", "Move Overhead value 50",
                 "Ponder value true", "Ponder value false", "Clear Hash", "EvalFile value <default>"]:
        e.send("setoption name " + line)
    e.send("ucinewgame")
    e.send("position startpos")
    e.send("go depth 8")
    mv = bestmove(e.until("bestmove"))
    assert legal(chess.STARTING_FEN, mv), mv
    e.quit()
    return "ok"


@check("perft (move generation)")
def t_perft(cmd):
    e = Engine(cmd)
    total = 0
    for fen, depth, nodes in PERFT:
        e.send(f"position fen {fen}")
        e.send(f"go perft {depth}")
        lines = e.until("info string perft", 120)
        got = int(lines[-1].split("nodes")[1].split()[0])
        assert got == nodes, f"{fen} d{depth}: {got} != {nodes}"
        e.until("")  # bare node-count line
        total += got
    e.quit()
    return f"{len(PERFT)} positions, {total} leaf nodes match"


@check("special positions and terminal answers")
def t_special(cmd):
    e = Engine(cmd)
    for fen, expect in SPECIAL:
        e.send(f"position fen {fen}")
        e.send("go depth 10")
        mv = bestmove(e.until("bestmove"))
        if expect is not None:
            assert mv == expect, f"{fen}: {mv} != {expect}"
        else:
            assert legal(fen, mv), f"illegal {mv} in {fen}"
    # moves list with castling, en passant and promotion
    line = "e2e4 a7a6 e4e5 d7d5 e5d6 c7d6 g1f3 b7b5 f1e2 c8b7 e1g1 b5b4 c2c4 b4c3 d2d4 c3b2 h2h3 b2a1q"
    board = chess.Board()
    for m in line.split():
        board.push_uci(m)
    fen = board.fen()
    e.send("position startpos moves " + line)
    e.send("go nodes 20000")
    mv = bestmove(e.until("bestmove"))
    assert legal(fen, mv), mv
    e.quit()
    return f"{len(SPECIAL) + 1} positions"


@check("legality: random games, every bestmove legal")
def t_random(cmd, games, rng):
    e = Engine(cmd)
    moves = 0
    for _ in range(games):
        board = chess.Board()
        e.send("ucinewgame")
        while not board.is_game_over(claim_draw=True) and board.ply() < 160:
            if rng.random() < 0.3:
                board.push(rng.choice(list(board.legal_moves)))
                continue
            e.send("position startpos moves " + " ".join(m.uci() for m in board.move_stack)
                   if board.move_stack else "position startpos")
            e.send(f"go nodes {rng.choice([1, 50, 500, 3000])}")
            mv = bestmove(e.until("bestmove"))
            assert legal(board.fen(), mv), f"illegal {mv} in {board.fen()}"
            board.push_uci(mv)
            moves += 1
    e.quit()
    return f"{games} games, {moves} engine moves"


@check("pv legality")
def t_pv(cmd, rng):
    e = Engine(cmd)
    n = 0
    for fen, _, _ in PERFT:
        e.send(f"position fen {fen}")
        e.send("go depth 12")
        lines = e.until("bestmove")
        for l in lines:
            if " pv " in l:
                board = chess.Board(fen)
                for mv in l.split(" pv ")[1].split():
                    assert chess.Move.from_uci(mv) in board.legal_moves, f"illegal pv {mv}: {l}"
                    board.push_uci(mv)
                n += 1
    e.quit()
    return f"{n} pv lines replayed"


@check("go limits: depth, nodes, movetime, infinite/stop, isready while searching")
def t_limits(cmd):
    e = Engine(cmd)
    e.send("position startpos")
    e.send("go depth 5")
    lines = e.until("bestmove")
    depths = [int(l.split()[2]) for l in lines if l.startswith("info depth")]
    assert max(depths) == 5, depths
    e.send("go nodes 10000")
    lines = e.until("bestmove")
    t = time.time()
    e.send("go movetime 1000")
    e.until("bestmove", 5)
    dt = time.time() - t
    assert 0.6 < dt < 1.3, f"movetime 1000 took {dt:.3f}s"
    e.send("go infinite")
    time.sleep(0.5)
    t = time.time()
    e.send("isready")
    e.until("readyok", 2)
    ready = time.time() - t
    e.send("stop")
    e.until("bestmove", 2)
    e.send("isready")
    lines = e.until("readyok")
    assert not any(l.startswith("bestmove") for l in lines), "second bestmove"
    e.quit()
    return f"movetime 1000 -> {dt:.3f}s, readyok during search after {ready * 1000:.0f} ms"


@check("ponder / ponderhit / stop")
def t_ponder(cmd):
    e = Engine(cmd)
    e.send("setoption name Ponder value true")
    e.send("position startpos moves e2e4 e7e5 g1f3")
    e.send("go ponder wtime 2000 btime 2000 winc 0 binc 0")
    time.sleep(1.5)
    early = [l for l in e.lines_for(0.1) if l.startswith("bestmove")]
    assert not early, "bestmove while pondering"
    t = time.time()
    e.send("ponderhit")
    e.until("bestmove", 3)
    after = time.time() - t
    assert after < 1.4, f"took {after:.2f}s after ponderhit with 2 s on the clock"
    e.send("position startpos moves d2d4")
    e.send("go ponder wtime 60000 btime 60000")
    time.sleep(0.3)
    e.send("stop")
    mv = bestmove(e.until("bestmove", 3))
    assert legal(chess.Board("rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq - 0 1").fen(), mv)
    e.quit()
    return f"answered {after * 1000:.0f} ms after ponderhit"


@check("clock: extreme time situations never overstep")
def t_clock(cmd):
    e = Engine(cmd)
    e.send("setoption name Move Overhead value 50")
    worst = 0.0
    cases = [("wtime 100 btime 100", 0.1), ("wtime 30 btime 30", 0.03),
             ("wtime 1000 btime 1000 movestogo 1", 1.0), ("wtime 500 btime 500 winc 1000 binc 1000", 0.5),
             ("wtime 2000 btime 2000 movestogo 40", 2.0), ("wtime 60000 btime 60000 movestogo 1", 60.0),
             ("wtime 15 btime 15 winc 100 binc 100", 0.015)]
    fens = ["position startpos", "position fen r1bq1rk1/ppp2ppp/2np1n2/2b1p3/2B1P3/2PP1N2/PP3PPP/RNBQ1RK1 w - - 0 7"]
    details = []
    for go, clock in cases:
        if clock > 10:
            continue
        for pos in fens:
            e.send(pos)
            t = time.time()
            e.send("go " + go)
            e.until("bestmove", clock + 5)
            dt = time.time() - t
            share = dt / clock
            worst = max(worst, share)
            details.append(f"{go.split()[1]}ms{'/mtg' + go.split()[-1] if 'movestogo' in go else ''}:{dt * 1000:.0f}")
            # The GUI clock includes pipe latency; the engine must leave a margin.
            if clock >= 0.5:
                assert dt < clock * 0.85, f"go {go}: used {dt:.3f}s of {clock}s"
            else:
                assert dt < clock + 0.06, f"go {go}: used {dt:.3f}s of {clock}s"
    e.quit()
    return "; ".join(details)


@check("threads + hash stress, quit during search")
def t_threads(cmd, seconds):
    e = Engine(cmd)
    e.send("setoption name Threads value 4")
    e.send("setoption name Hash value 256")
    e.ready(60)
    e.send("position fen r2q1rk1/pp2bppp/2n1pn2/2pp4/3P1B2/2PBPN2/PP1N1PPP/R2QK2R w KQ - 0 9")
    e.send("go infinite")
    time.sleep(seconds)
    e.send("stop")
    lines = e.until("bestmove", 5)
    info = [l for l in lines if l.startswith("info depth")][-1]
    words = info.split()
    nps = int(words[words.index("nps") + 1])
    depth = int(words[2])
    hashfull = int(words[words.index("hashfull") + 1])
    # rapid start/stop cycles and option changes between searches
    for i in range(40):
        e.send("position startpos moves e2e4" if i % 2 else "position startpos")
        e.send("go infinite")
        time.sleep(0.02)
        e.send("stop")
        e.until("bestmove", 5)
    e.send("setoption name Threads value 2")
    e.send("ucinewgame")
    e.send("position startpos")
    e.send("go wtime 3000 btime 3000")
    e.until("bestmove", 5)
    e.send("position startpos")
    e.send("go infinite")
    time.sleep(0.3)
    t = time.time()
    code = e.quit(5)
    assert code == 0, f"exit code {code} after quit during search"
    return f"4 threads {seconds}s: depth {depth}, {nps} nps, hashfull {hashfull}; 40 start/stop cycles; quit in {time.time() - t:.2f}s"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true")
    ap.add_argument("--expect-sha256")
    ap.add_argument("--expect-name")
    ap.add_argument("--json")
    ap.add_argument("--seed", type=int, default=2026)
    ap.add_argument("cmd", nargs="+")
    args = ap.parse_args()
    rng = random.Random(args.seed)
    cmd = args.cmd
    t_handshake(cmd, args.expect_sha256, args.expect_name)
    t_options(cmd)
    t_perft(cmd)
    t_special(cmd)
    t_random(cmd, 4 if args.quick else 30, rng)
    t_pv(cmd, rng)
    t_limits(cmd)
    t_ponder(cmd)
    t_clock(cmd)
    t_threads(cmd, 3 if args.quick else 20)
    ok = all(r["ok"] for r in RESULTS)
    summary = {"cmd": cmd, "ok": ok, "checks": RESULTS}
    if args.json:
        with open(args.json, "w") as f:
            json.dump(summary, f, indent=1)
    print(f"{sum(r['ok'] for r in RESULTS)}/{len(RESULTS)} checks passed")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
