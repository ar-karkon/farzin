"""Verify the first repeating control counts moves already played in the book."""
import pathlib
import sys
import unittest
from types import SimpleNamespace
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]))
import arena

@unittest.skipIf(arena.chess is None, 'python-chess required')
class BookClockTest(unittest.TestCase):
    def test_first_control_has_36_search_moves_after_four_book_moves(self):
        class Engine:
            protocol=SimpleNamespace(send_line=lambda line:None)
            def ping(self):pass
            def play(self, board, limit, **kwargs):
                self.limit=limit
                return SimpleNamespace(move=None,info={})
        engine=Engine()
        pool=SimpleNamespace(get=lambda name:engine,specs={'a':{},'b':{}})
        args=SimpleNamespace(book_plies=8,nodes=None,movetime=None,time_margin=0,max_plies=400)
        result,reason,pgn,stats=arena.play_game(pool,'a','b',arena.chess.STARTING_FEN,(900.,0.,40),args)
        self.assertEqual(engine.limit.remaining_moves,36)
        self.assertIn(' 0 5',pgn)
        self.assertEqual(reason,'illegal move None')
