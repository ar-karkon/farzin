#!/usr/bin/env python3
"""Run a fixed-depth UCI benchmark on known positions; no third-party packages."""
import argparse
import platform
import subprocess
import time

POSITIONS = [
    "startpos",
    "fen r1bq1rk1/ppp2ppp/2n2n2/3pp3/3PP3/2N2N2/PPP2PPP/R1BQ1RK1 w - - 0 6",
    "fen 8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("engine", help="path to UCI engine executable")
    parser.add_argument("--depth", type=int, default=4)
    args = parser.parse_args()
    if not 1 <= args.depth <= 64:
        parser.error("depth must be between 1 and 64")
    print(f"platform={platform.platform()} engine={args.engine} depth={args.depth}")
    with subprocess.Popen([args.engine], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          text=True, bufsize=1) as engine:
        def send(command):
            engine.stdin.write(command + "\n")
            engine.stdin.flush()

        def until(prefix):
            while True:
                line = engine.stdout.readline()
                if not line:
                    raise RuntimeError("engine exited unexpectedly")
                if line.startswith(prefix):
                    return line.strip()

        send("uci")
        until("uciok")
        send("isready")
        until("readyok")
        for position in POSITIONS:
            send("ucinewgame")
            send("position " + position)
            start = time.monotonic()
            send(f"go depth {args.depth}")
            move = until("bestmove ")
            print(f"{position}: {move} elapsed_ms={(time.monotonic()-start)*1000:.1f}")
        send("quit")
        engine.wait(timeout=5)


if __name__ == "__main__":
    main()
