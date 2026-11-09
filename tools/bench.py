#!/usr/bin/env python3
"""Run a fixed-depth UCI benchmark on known positions; no third-party packages."""
import argparse
import hashlib
import json
import platform
import subprocess
import time

POSITIONS = [
    "startpos",
    "fen r1bq1rk1/ppp2ppp/2n2n2/3pp3/3PP3/2N2N2/PPP2PPP/R1BQ1RK1 w - - 0 6",
    "fen 8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
]


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("engine", help="path to UCI engine executable")
    parser.add_argument("--depth", type=int, default=4)
    parser.add_argument("--json", help="write results as JSON")
    parser.add_argument("--option", action="append", default=[], metavar="NAME=VALUE")
    args = parser.parse_args()
    if not 1 <= args.depth <= 64:
        parser.error("depth must be between 1 and 64")
    options = []
    option_names = set()
    for option in args.option:
        if "=" not in option or "\n" in option or "\r" in option:
            parser.error("options must be NAME=VALUE on one line")
        name, value = option.split("=", 1)
        if not name.strip():
            parser.error("option name cannot be empty")
        if name.strip().lower() in option_names:
            parser.error("duplicate option name")
        option_names.add(name.strip().lower())
        options.append((name.strip(), value))
    report = {"platform": platform.platform(), "engine": args.engine,
              "engine_sha256": sha256_file(args.engine), "depth": args.depth,
              "options": options, "positions": []}
    print(f"platform={platform.platform()} engine={args.engine} depth={args.depth}")
    with subprocess.Popen([args.engine], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          text=True, bufsize=1) as engine:
        def send(command):
            engine.stdin.write(command + "\n")
            engine.stdin.flush()

        def until(prefix, reject_failures=False):
            observed = []
            while True:
                line = engine.stdout.readline()
                if not line:
                    raise RuntimeError("engine exited unexpectedly")
                if reject_failures and line.startswith("info string failed to load network:"):
                    raise RuntimeError(line.strip())
                observed.append(line.strip())
                if line.startswith(prefix):
                    return observed

        send("uci")
        identity = until("uciok")
        for name, value in options:
            send(f"setoption name {name} value {value}")
        send("isready")
        setup = until("readyok", reject_failures=True)
        network = next((value for name, value in options if name.lower() == "evalfile"), None)
        farzin = any(line.startswith("id name Farzin ") for line in identity)
        if farzin and network and network != "<empty>":
            if "info string experimental network loaded" not in setup:
                raise RuntimeError("Farzin did not confirm loading the requested EvalFile")
            report["network_sha256"] = sha256_file(network)
        for position in POSITIONS:
            send("ucinewgame")
            send("position " + position)
            start = time.monotonic()
            send(f"go depth {args.depth}")
            infos = []
            while True:
                line = engine.stdout.readline()
                if not line:
                    raise RuntimeError("engine exited unexpectedly")
                if line.startswith("bestmove "):
                    move = line.strip()
                    break
                if line.startswith("info "):
                    infos.append(line.strip())
            elapsed = (time.monotonic() - start) * 1000
            completed = [int(line.split()[2]) for line in infos if line.startswith("info depth ")]
            if not completed or max(completed) < args.depth:
                raise RuntimeError(f"search did not complete depth {args.depth}: {position}")
            report["positions"].append({"position": position, "bestmove": move.split()[1], "elapsed_ms": elapsed, "info": infos})
            print(f"{position}: {move} elapsed_ms={elapsed:.1f}")
        send("quit")
        engine.wait(timeout=5)
    if args.json:
        with open(args.json, "w", encoding="utf-8") as result:
            json.dump(report, result, indent=2, ensure_ascii=False)
            result.write("\n")


if __name__ == "__main__":
    main()
