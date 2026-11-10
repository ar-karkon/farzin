#!/usr/bin/env python3
"""SPSA tuning of Farzin search parameters exposed as `tune_*` UCI options.

Each iteration perturbs all parameters by +/-c_k, plays a small paired match
(theta+ vs theta-) with tools/arena.py and moves theta along the measured
gradient. Progress is appended to --log as JSON lines so runs can resume.

  tools/spsa.py --engine target/release/farzin --params tools/spsa-params.json \
      --iterations 200 --pairs 8 --tc 5+0.05 --log out/spsa.jsonl
"""
import argparse
import json
import os
import random
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))


def run_match(engine, plus, minus, args, seed):
    def spec(name, values):
        opts = ",".join(f"opt.tune_{k}={int(round(v))}" for k, v in values.items())
        return f"name={name},cmd={engine},{opts}"
    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as tmp:
        out = tmp.name
    cmd = [sys.executable, os.path.join(HERE, "arena.py"), "--engine", spec("plus", plus),
           "--engine", spec("minus", minus), "--tc", args.tc, "--pairs", str(args.pairs),
           "--concurrency", str(args.concurrency), "--seed", str(seed), "--out", out]
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL)
    with open(out) as f:
        data = json.load(f)
    os.unlink(out)
    s = data["summary"]["plus vs minus"]
    return s["score"] - 0.5  # in [-0.5, 0.5]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engine", required=True)
    ap.add_argument("--params", required=True, help='JSON {"name": [value, min, max, c_end]}')
    ap.add_argument("--iterations", type=int, default=100)
    ap.add_argument("--pairs", type=int, default=8)
    ap.add_argument("--tc", default="5+0.05")
    ap.add_argument("--concurrency", type=int, default=os.cpu_count())
    ap.add_argument("--a-ratio", type=float, default=0.1)
    ap.add_argument("--r-end", type=float, default=0.002)
    ap.add_argument("--log", required=True)
    ap.add_argument("--seed", type=int, default=1)
    args = ap.parse_args()
    with open(args.params) as f:
        spec = json.load(f)
    theta = {k: float(v[0]) for k, v in spec.items()}
    start = 0
    if os.path.exists(args.log):
        with open(args.log) as f:
            lines = [json.loads(l) for l in f if l.strip()]
        if lines:
            theta = lines[-1]["theta"]
            start = lines[-1]["k"] + 1
    rng = random.Random(args.seed + start)
    n = args.iterations
    big_a = args.a_ratio * n
    alpha, gamma = 0.602, 0.101
    for k in range(start, n):
        plus, minus, signs, cks = {}, {}, {}, {}
        for name, (_, lo, hi, c_end) in spec.items():
            c_k = c_end * (n ** gamma) / ((k + 1) ** gamma)
            delta = rng.choice((-1, 1))
            signs[name], cks[name] = delta, c_k
            plus[name] = min(hi, max(lo, theta[name] + c_k * delta))
            minus[name] = min(hi, max(lo, theta[name] - c_k * delta))
        result = run_match(args.engine, plus, minus, args, args.seed * 100003 + k)
        for name, (_, lo, hi, c_end) in spec.items():
            a_end = args.r_end * c_end ** 2
            a = a_end * (big_a + n) ** alpha
            a_k = a / (k + 1 + big_a) ** alpha
            grad = result * 2 * args.pairs / (cks[name] * signs[name])
            theta[name] = min(hi, max(lo, theta[name] + a_k * grad))
        with open(args.log, "a") as f:
            f.write(json.dumps({"k": k, "result": result, "theta": theta}) + "\n")
        print(f"iter {k} result {result:+.3f} " + " ".join(f"{a}={b:.1f}" for a, b in theta.items()), flush=True)


if __name__ == "__main__":
    main()
