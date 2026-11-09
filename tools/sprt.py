#!/usr/bin/env python3
"""Pentanomial GSPRT and Elo estimate for paired match reports from match.py.

Each opening is played twice with reversed colours. The pair score of engine A
(0, 0.25, 0.5, 0.75 or 1) is the unit of observation, which removes most of the
variance caused by unbalanced openings. The log-likelihood ratio uses the
normal approximation of the generalized SPRT used by common engine testing
frameworks, with logistic Elo bounds H0: elo0 and H1: elo1.

Unfinished games (result "*", e.g. ply limit) are excluded by default together
with their pair; `--unfinished-as-draw` counts them as draws instead. Either
choice is reported. The Elo interval is a normal-approximation 95% interval of
the mean pair score; it is only meaningful for large samples.
"""
import argparse
import json
import math


def score_for_a(game, unfinished_as_draw):
    result = game.get("result")
    if result == "*":
        return 0.5 if unfinished_as_draw else None
    if result == "1/2-1/2":
        return 0.5
    if result not in ("1-0", "0-1") or game.get("white_engine") not in (0, 1):
        raise ValueError("invalid game result or engine assignment")
    return float((result == "1-0") == (game["white_engine"] == 0))


def pentanomial(reports, unfinished_as_draw=False):
    counts = [0, 0, 0, 0, 0]
    games = {"wins": 0, "draws": 0, "losses": 0, "unfinished": 0}
    excluded = 0
    for report in reports:
        played = report["games"]
        if len(played) % 2:
            raise ValueError("games must be complete ordered pairs")
        for i in range(0, len(played), 2):
            first, second = played[i:i + 2]
            if first.get("opening_fen") != second.get("opening_fen") or \
                    {first.get("white_engine"), second.get("white_engine")} != {0, 1}:
                raise ValueError(f"games {i + 1}-{i + 2} do not form an opening/color pair")
            scores = []
            for game in (first, second):
                if game.get("result") == "*":
                    games["unfinished"] += 1
                value = score_for_a(game, unfinished_as_draw)
                if value is not None and game.get("result") != "*":
                    games[{1.0: "wins", 0.5: "draws", 0.0: "losses"}[value]] += 1
                scores.append(value)
            if None in scores:
                excluded += 1
                continue
            counts[int(round(sum(scores) * 2))] += 1
    return counts, games, excluded


def expected_score(elo):
    return 1 / (1 + 10 ** (-elo / 400))


def elo_from_score(score):
    score = min(max(score, 1e-9), 1 - 1e-9)
    return -400 * math.log10(1 / score - 1)


def analyse(counts, elo0, elo1, alpha, beta):
    pairs = sum(counts)
    result = {"pentanomial": counts, "pairs": pairs, "elo0": elo0, "elo1": elo1,
              "alpha": alpha, "beta": beta,
              "lower_bound": math.log(beta / (1 - alpha)),
              "upper_bound": math.log((1 - beta) / alpha)}
    if pairs == 0:
        result.update(llr=0.0, decision="continue", score=None, elo=None, elo_95=None)
        return result
    values = [0, 0.25, 0.5, 0.75, 1]
    mean = sum(v * c for v, c in zip(values, counts)) / pairs
    variance = sum(c * (v - mean) ** 2 for v, c in zip(values, counts)) / pairs
    # Avoid a zero variance for tiny or one-sided samples.
    variance = max(variance, 1e-3 / pairs)
    s0, s1 = expected_score(elo0), expected_score(elo1)
    llr = pairs * (s1 - s0) * (2 * mean - s0 - s1) / (2 * variance)
    radius = 1.96 * math.sqrt(variance / pairs)
    decision = "continue"
    if llr >= result["upper_bound"]:
        decision = "accept H1"
    elif llr <= result["lower_bound"]:
        decision = "accept H0"
    result.update(llr=llr, decision=decision, score=mean,
                  elo=elo_from_score(mean),
                  elo_95=[elo_from_score(mean - radius), elo_from_score(mean + radius)])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reports", nargs="+", help="match.py JSON reports for the same pairing")
    parser.add_argument("--elo0", type=float, default=0.0)
    parser.add_argument("--elo1", type=float, default=10.0)
    parser.add_argument("--alpha", type=float, default=0.05)
    parser.add_argument("--beta", type=float, default=0.05)
    parser.add_argument("--unfinished-as-draw", action="store_true")
    parser.add_argument("--json", help="optional output path")
    args = parser.parse_args()
    if not (0 < args.alpha < 1 and 0 < args.beta < 1) or args.elo1 <= args.elo0:
        parser.error("alpha/beta must be in (0, 1) and elo1 must exceed elo0")
    reports = []
    for path in args.reports:
        with open(path, encoding="utf-8") as source:
            reports.append(json.load(source))
    keys = [(r.get("engine_sha256"), r.get("options"), r.get("depth"), r.get("movetime_ms")) for r in reports]
    if any(key != keys[0] for key in keys):
        parser.error("reports must use the same engines, options and limits")
    counts, games, excluded = pentanomial(reports, args.unfinished_as_draw)
    result = analyse(counts, args.elo0, args.elo1, args.alpha, args.beta)
    result.update(games=games, excluded_pairs=excluded, unfinished_as_draw=args.unfinished_as_draw,
                  reports=args.reports, options=reports[0].get("options"),
                  movetime_ms=reports[0].get("movetime_ms"), depth=reports[0].get("depth"),
                  engine_sha256=reports[0].get("engine_sha256"))
    text = json.dumps(result, indent=2)
    if args.json:
        with open(args.json, "w", encoding="utf-8") as output:
            output.write(text + "\n")
    print(text)


if __name__ == "__main__":
    main()
