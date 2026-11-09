#!/usr/bin/env python3
"""Summarize paired games with a conservative fixed-sample score bound.

The bound assumes independent, identically distributed opening pairs. A curated
or repeated opening suite does not establish this assumption; this is not SPRT
or a calibrated Elo estimate.
"""
import argparse
import json
import math


def score_for_a(game):
    result = game.get("result")
    if result == "*":
        return None
    if result == "1/2-1/2":
        return 0.5
    if result not in ("1-0", "0-1") or game.get("white_engine") not in (0, 1):
        raise ValueError("invalid game result or engine assignment")
    return float((result == "1-0") == (game["white_engine"] == 0))


def summarize(report, alpha=0.05):
    if not 0 < alpha < 1:
        raise ValueError("alpha must be between 0 and 1")
    games = report["games"]
    if len(games) % 2:
        raise ValueError("games must be complete ordered pairs")
    pairs = []
    counts = {"wins": 0, "draws": 0, "losses": 0, "unfinished": 0}
    excluded = 0
    for i in range(0, len(games), 2):
        first, second = games[i:i + 2]
        if first.get("opening_fen") != second.get("opening_fen") or {first.get("white_engine"), second.get("white_engine")} != {0, 1}:
            raise ValueError(f"games {i + 1}-{i + 2} do not form an opening/color pair")
        scores = [score_for_a(first), score_for_a(second)]
        for value in scores:
            counts[{1.0: "wins", 0.5: "draws", 0.0: "losses", None: "unfinished"}[value]] += 1
        if None in scores:
            excluded += 1
        else:
            pairs.append(sum(scores) / 2)
    result = {"games": counts, "completed_pairs": len(pairs), "excluded_pairs": excluded,
              "alpha": alpha, "mean_pair_score": None, "score_bound": None,
              "assumption": "fixed sample of independent identically distributed completed opening pairs; curated/repeated openings and excluding unfinished pairs may bias results; not SPRT or Elo"}
    if pairs:
        mean = sum(pairs) / len(pairs)
        radius = math.sqrt(math.log(2 / alpha) / (2 * len(pairs)))
        result["mean_pair_score"] = mean
        result["score_bound"] = [max(0, mean - radius), min(1, mean + radius)]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report")
    parser.add_argument("--alpha", type=float, default=0.05)
    args = parser.parse_args()
    try:
        with open(args.report, encoding="utf-8") as source:
            result = summarize(json.load(source), args.alpha)
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"error: {error}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
