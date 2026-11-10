#!/usr/bin/env python3
"""Compare Farzin's HalfKAv2_hm output with a reference engine.

Usage: hka_oracle.py REFERENCE_ENGINE positions.tsv

`positions.tsv` holds `fen<TAB>raw` lines from `examples/hka_check.rs`.
The reference engine must answer a `rawnnue` command with `raw <int>`
(a one-line local debugging patch to the reference engine's UCI loop that
prints `NNUE::evaluate(pos, false)`; it is not distributed with Farzin).
"""
import subprocess
import sys


def main() -> int:
    engine, path = sys.argv[1], sys.argv[2]
    rows = [line.rstrip("\n").split("\t") for line in open(path) if line.strip()]
    p = subprocess.Popen([engine], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    p.stdin.write("uci\nisready\n")
    while p.stdout.readline().strip() != "readyok":
        pass
    bad = 0
    for fen, raw in rows:
        p.stdin.write(f"position fen {fen}\nrawnnue\n")
        while True:
            line = p.stdout.readline().strip()
            if line.startswith("raw "):
                ref = int(line.split()[1])
                break
        if ref != int(raw):
            bad += 1
            if bad <= 10:
                print(f"MISMATCH {fen}: farzin {raw} reference {ref}")
    p.stdin.write("quit\n")
    print(f"{len(rows) - bad}/{len(rows)} positions identical")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
