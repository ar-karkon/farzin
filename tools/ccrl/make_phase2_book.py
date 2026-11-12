#!/usr/bin/env python3
"""Reproduce the frozen opening suite from a checkout of lichess chess-openings."""
import csv
import io
import pathlib
import random
import sys
import chess
import chess.pgn

rows = set()
for file in sorted(pathlib.Path(sys.argv[1]).glob('?.tsv')):
    for row in csv.DictReader(file.open(), delimiter='\t'):
        game = chess.pgn.read_game(io.StringIO(row['pgn']))
        moves = list(game.mainline_moves())
        if len(moves) < 8:
            continue
        board = chess.Board()
        for move in moves[:8]:
            board.push(move)
        rows.add(' '.join(board.fen().split()[:4]))
print('\n'.join(random.Random(20261112).sample(sorted(rows), 150)))
