#!/usr/bin/env python3
"""Estimate ratings from arena.py result files with fixed anchors.

Bradley-Terry (logistic Elo) maximum likelihood over all games; anchors are
held at given ratings, the rest are fitted. Confidence intervals come from a
bootstrap over game pairs (both colours of one opening resampled together).

  tools/rating.py results/calib-*.json --anchor sf1800=1800 --anchor sf2200=2200
"""
import argparse
import json
import math
import random


def load_pairs(paths):
    pairs = []
    for path in paths:
        with open(path) as f:
            data = json.load(f)
        games = data["games"]
        # arena.py appends both games of a pair consecutively.
        for i in range(0, len(games) - 1, 2):
            pairs.append((games[i], games[i + 1]))
    return pairs


def score(result):
    return {"1-0": 1.0, "0-1": 0.0, "1/2-1/2": 0.5}[result]


def fit(pairs, anchors, iters=2000):
    players = sorted({g[0] for p in pairs for g in p} | {g[1] for p in pairs for g in p})
    r = {p: anchors.get(p, 0.0) for p in players}
    if not anchors:
        anchors = {players[0]: 0.0}
    k = math.log(10) / 400
    for _ in range(iters):
        grad = {p: 0.0 for p in players}
        hess = {p: 1e-9 for p in players}
        for pair in pairs:
            for white, black, res, _ in pair:
                e = 1 / (1 + math.exp(-k * (r[white] - r[black])))
                s = score(res)
                grad[white] += k * (s - e)
                grad[black] -= k * (s - e)
                h = k * k * e * (1 - e)
                hess[white] += h
                hess[black] += h
        delta = 0.0
        for p in players:
            if p in anchors:
                continue
            step = max(-200, min(200, grad[p] / hess[p]))
            r[p] += step
            delta = max(delta, abs(step))
        if delta < 1e-3:
            break
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="+")
    ap.add_argument("--anchor", action="append", default=[], help="NAME=RATING")
    ap.add_argument("--bootstrap", type=int, default=200)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--json", default=None)
    args = ap.parse_args()
    anchors = {}
    for a in args.anchor:
        name, _, value = a.partition("=")
        anchors[name] = float(value)
    pairs = load_pairs(args.files)
    ratings = fit(pairs, anchors)
    rng = random.Random(args.seed)
    samples = {p: [] for p in ratings}
    for _ in range(args.bootstrap):
        boot = [pairs[rng.randrange(len(pairs))] for _ in pairs]
        br = fit(boot, anchors, iters=300)
        for p in ratings:
            if p in br:
                samples[p].append(br[p])
    stats = {}
    games = {p: 0 for p in ratings}
    points = {p: 0.0 for p in ratings}
    for pair in pairs:
        for white, black, res, _ in pair:
            games[white] += 1
            games[black] += 1
            points[white] += score(res)
            points[black] += 1 - score(res)
    print(f"{'player':<28}{'rating':>8}{'95% CI':>20}{'games':>8}{'score':>8}")
    for p in sorted(ratings, key=lambda q: -ratings[q]):
        s = sorted(samples[p])
        lo = s[int(0.025 * len(s))] if s else ratings[p]
        hi = s[int(0.975 * len(s)) - 1] if s else ratings[p]
        tag = " (anchor)" if p in anchors else ""
        stats[p] = {"rating": ratings[p], "ci95": [lo, hi], "games": games[p],
                    "score": points[p] / max(1, games[p]), "anchor": p in anchors}
        print(f"{p:<28}{ratings[p]:8.0f}   [{lo:7.0f},{hi:7.0f}]{games[p]:8d}{100 * points[p] / max(1, games[p]):7.1f}%{tag}")
    if args.json:
        with open(args.json, "w") as f:
            json.dump({"anchors": anchors, "players": stats, "files": args.files}, f, indent=1)


if __name__ == "__main__":
    main()
