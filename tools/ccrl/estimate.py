#!/usr/bin/env python3
"""Single-target anchored logistic Elo; stratified paired bootstrap.

Only compares target against fixed anchors. Does not convert time controls.
Published anchor errors are treated as normal 95% half-widths, independently;
this assumption does not capture shared pool or hardware uncertainty.
"""
import argparse
import json
import math
import random


def expectation(delta):
    x = delta * math.log(10) / 400
    return 1 / (1 + math.exp(-x)) if x >= 0 else math.exp(x) / (1 + math.exp(x))


def fit(groups, anchors):
    games = sum(len(v) * 2 for v in groups.values())
    points = sum(sum(v) for v in groups.values())
    if not games:
        raise ValueError('no complete pairs')
    if points == 0:
        return -math.inf
    if points == games:
        return math.inf
    lo, hi = min(anchors.values()) - 20000, max(anchors.values()) + 20000
    for _ in range(100):
        mid = (lo + hi) / 2
        predicted = sum(2 * len(v) * expectation(mid - anchors[n]) for n, v in groups.items())
        if predicted < points:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2


class Groups(dict):
    shared = False


def load(path, target, anchors):
    d = json.load(open(path))
    groups = Groups()
    keyed = {}
    games = d['games']
    if len(games) % 2:
        raise ValueError('incomplete pair')
    for i in range(0, len(games), 2):
        a, b = games[i:i+2]
        if a[:2] != b[:2][::-1] or target not in a[:2]:
            raise ValueError('invalid color pair or disconnected target')
        opponent = next(x for x in a[:2] if x != target)
        if opponent not in anchors:
            raise ValueError('missing anchor: ' + opponent)
        points = 0
        for w, black, result, reason in (a, b):
            s = {'1-0': 1, '0-1': 0, '1/2-1/2': .5}[result]
            points += s if w == target else 1 - s
        groups.setdefault(opponent, []).append(points)
        if "game_details" in d:
            details = d["game_details"]
            if details[i]["fen"] != details[i+1]["fen"]:
                raise ValueError("opening mismatch in pair")
            keyed.setdefault(opponent, []).append((details[i]["fen"], points))
    # Common opening blocks preserve dependence across opponents when coverage
    # is identical. Repeated FENs require a separate nested bootstrap.
    if keyed:
        keys = [sorted(fen for fen, _ in v) for v in keyed.values()]
        if any(len(set(k)) != len(k) for k in keys):
            raise ValueError("repeated openings require a clustered analysis")
        if all(k == keys[0] for k in keys) and len(set(keys[0])) == len(keys[0]):
            for n, v in keyed.items():
                groups[n] = [score for fen, score in sorted(v)]
            groups.shared = True
    return groups


def encode(x):
    return x if math.isfinite(x) else ('+infinity' if x > 0 else '-infinity')


def estimate(groups, anchors, errors, repetitions=4000, seed=20261112):
    rng = random.Random(seed)
    samples = []
    for _ in range(repetitions):
        if getattr(groups, "shared", False):
            count = len(next(iter(groups.values())))
            indices = rng.choices(range(count), k=count)
            resampled = {n: [v[i] for i in indices] for n, v in groups.items()}
        else:
            resampled = {n: rng.choices(v, k=len(v)) for n, v in groups.items()}
        noisy = {n: rng.gauss(a, errors[n] / 1.96) for n, a in anchors.items()}
        samples.append(fit(resampled, noisy))
    samples.sort()
    return dict(rating=encode(fit(groups, anchors)), ci95=None if all(len(set(v)) == 1 for v in groups.values()) else [encode(samples[int(.025*repetitions)]), encode(samples[min(repetitions-1, math.ceil(.975*repetitions)-1)])], ci_status='not estimable: no observed within-anchor pair-score variation' if all(len(set(v)) == 1 for v in groups.values()) else 'provisional conditional bootstrap interval', bootstrap=repetitions, seed=seed,
                pairs={n: len(v) for n,v in groups.items()}, method=('shared-opening block' if getattr(groups, 'shared', False) else 'stratified paired-opening') + ' percentile bootstrap; independent Gaussian anchor uncertainty')


def main():
    p = argparse.ArgumentParser()
    p.add_argument('results'); p.add_argument('manifest'); p.add_argument('--category', choices=['blitz','4015'], default='blitz'); p.add_argument('--target', default='Farzin-0.5.2')
    a = p.parse_args(); m = json.load(open(a.manifest))
    anchors = {n: v[a.category]['rating'] for n,v in m['opponents'].items() if a.category in v}
    errors = {n: v[a.category]['error'] for n,v in m['opponents'].items() if a.category in v}
    groups = load(a.results, a.target, anchors)
    print(json.dumps(estimate(groups, anchors, errors), indent=2, allow_nan=False))

if __name__ == '__main__':
    main()
