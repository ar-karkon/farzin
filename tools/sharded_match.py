#!/usr/bin/env python3
"""Run a paired match.py match over an opening suite in parallel shards.

Opening i (0-based) of `--openings` is assigned to shard i % shards, so every
opening is played exactly once per colour across all shards. Each shard is an
independent match.py process pinned to its own CPU when `--cpus` is given.
The shard reports are written next to `--prefix` and summarised with sprt.py.
"""
import argparse
import os
import subprocess
import sys
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("engine_a")
    parser.add_argument("engine_b")
    parser.add_argument("--openings", required=True)
    parser.add_argument("--first", type=int, default=0, help="index of the first opening used")
    parser.add_argument("--count", type=int, required=True, help="number of openings (pairs)")
    parser.add_argument("--shards", type=int, default=3)
    parser.add_argument("--cpus", help="comma-separated CPU list, one per shard")
    parser.add_argument("--movetime-ms", type=int, required=True)
    parser.add_argument("--max-plies", type=int, default=300)
    parser.add_argument("--option-a", action="append", default=[])
    parser.add_argument("--option-b", action="append", default=[])
    parser.add_argument("--prefix", required=True, help="output path prefix for shard reports")
    parser.add_argument("--python", default=sys.executable)
    args = parser.parse_args()
    tools = Path(__file__).resolve().parent
    with open(args.openings, encoding="utf-8") as source:
        openings = [line.strip() for line in source if line.strip() and not line.startswith("#")]
    selected = openings[args.first:args.first + args.count]
    if len(selected) != args.count or args.shards < 1:
        parser.error("not enough openings for the requested range")
    cpus = args.cpus.split(",") if args.cpus else []
    if cpus and len(cpus) != args.shards:
        parser.error("--cpus must list one CPU per shard")
    prefix = Path(args.prefix)
    prefix.parent.mkdir(parents=True, exist_ok=True)
    processes = []
    for shard in range(args.shards):
        subset = selected[shard::args.shards]
        if not subset:
            continue
        fen_file = prefix.with_name(f"{prefix.name}-shard{shard}.fen")
        fen_file.write_text("".join(f + "\n" for f in subset), encoding="utf-8")
        command = [args.python, str(tools / "match.py"), args.engine_a, args.engine_b,
                   "--pairs", str(len(subset)), "--movetime-ms", str(args.movetime_ms),
                   "--max-plies", str(args.max_plies), "--openings", str(fen_file),
                   "--json", str(prefix.with_name(f"{prefix.name}-shard{shard}.json"))]
        for option in args.option_a:
            command += ["--option-a", option]
        for option in args.option_b:
            command += ["--option-b", option]
        if cpus:
            command = ["taskset", "-c", cpus[shard]] + command
        log = open(prefix.with_name(f"{prefix.name}-shard{shard}.log"), "w", encoding="utf-8")
        processes.append((subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT), log))
    failed = False
    for process, log in processes:
        failed |= process.wait() != 0
        log.close()
    if failed:
        sys.exit("a shard failed; see the shard logs")
    reports = sorted(str(p) for p in prefix.parent.glob(f"{prefix.name}-shard*.json"))
    subprocess.run([sys.executable, str(tools / "sprt.py"), *reports, "--unfinished-as-draw",
                    "--json", str(prefix.with_name(f"{prefix.name}-sprt.json"))], check=True,
                   stdout=subprocess.DEVNULL)
    for path in prefix.parent.glob(f"{prefix.name}-shard*.fen"):
        os.remove(path)


if __name__ == "__main__":
    main()
