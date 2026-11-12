#!/usr/bin/env python3
"""Replay and summarize a completed Phase 2 arena dataset (no outcome filtering)."""
import argparse
import collections
import hashlib
import json
import pathlib
import chess
import chess.pgn
from estimate import load, estimate, fit, encode


def audit(results, manifest):
    path = pathlib.Path(results)
    d = json.loads(path.read_text())
    m = json.loads(pathlib.Path(manifest).read_text())
    target = 'Farzin-0.5.2'
    assert d['tc'] == '120+1' and d['adjudication'] == 'none'
    assert d['threads'] == 1 and d['concurrency'] <= 2
    anchors = {n: x['blitz']['rating'] for n, x in m['opponents'].items()}
    errors = {n: x['blitz']['error'] for n, x in m['opponents'].items()}
    assert d['book_sha256'] == m['book']['sha256']
    assert d['engines'][target]['sha256'] == m['farzin']['sha256']
    for name in d['engines']:
        if name != target:
            assert d['engines'][name]['sha256'] == m['opponents'][name]['sha256']
    assert len(d['games']) == 2 * m['protocol']['pairs'], 'planned batch incomplete'
    assert not d.get('scheduler_errors'), 'scheduler errors require investigation'
    groups = load(path, target, anchors)
    summary = collections.defaultdict(collections.Counter)
    colors = collections.defaultdict(list)
    with path.with_suffix('.pgn').open() as stream:
        for index, row in enumerate(d['games']):
            game = chess.pgn.read_game(stream)
            assert game is not None and not game.errors
            white, black, result, reason = row
            assert [game.headers[k] for k in ('White', 'Black', 'Result', 'Termination')] == row
            board = game.end().board()
            if reason in {'checkmate', 'stalemate', 'threefold_repetition', 'fifty_moves', 'insufficient_material', 'fivefold_repetition', 'seventyfive_moves'}:
                assert board.outcome(claim_draw=True).result() == result
            opponent = black if white == target else white
            points = {'1-0': 1, '1/2-1/2': .5, '0-1': 0}[result]
            if black == target:
                points = 1 - points
            summary[opponent][{1: 'wins', .5: 'draws', 0: 'losses'}[points]] += 1
            colors['white' if white == target else 'black'].append(points)
        assert chess.pgn.read_game(stream) is None
    output = dict(games=len(d['games']), pairs=sum(map(len, groups.values())),
                  unique_openings=len({x['fen'] for x in d['game_details']}),
                  wdl=dict(summary), terminations=d['terminations'],
                  color_scores={k: dict(points=sum(v), games=len(v), score=sum(v)/len(v)) for k,v in colors.items()},
                  estimate=estimate(groups, anchors, errors),
                  per_opponent={n: encode(fit({n:v}, anchors)) for n,v in groups.items()},
                  warning='Feasibility pilot: percentile bootstrap with very few pairs is unstable; model interval excludes hardware/configuration/time-control systematics.',
                  pgn_replayed=True,
                  dataset_sha256=hashlib.sha256(path.read_bytes()).hexdigest())
    return output


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('results'); p.add_argument('manifest')
    a = p.parse_args()
    print(json.dumps(audit(a.results, a.manifest), indent=2, allow_nan=False))
