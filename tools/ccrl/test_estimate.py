import json
import tempfile
import pathlib
import math
import unittest
from estimate import fit, estimate, expectation, load, Groups

class EstimatorTests(unittest.TestCase):
    def test_balanced(self):
        self.assertAlmostEqual(fit({'a':[0,2,1]}, {'a':3300}),3300)
    def test_boundary(self):
        self.assertEqual(fit({'a':[0]*4}, {'a':3300}),-math.inf)
        self.assertEqual(fit({'a':[2]*4}, {'a':3300}),math.inf)
    def test_empty(self):
        with self.assertRaises(ValueError): fit({}, {})
    def test_translation(self):
        g={'a':[1,2,1], 'b':[0,1,1]}
        self.assertAlmostEqual(fit(g,{'a':3000,'b':3200})+100,fit(g,{'a':3100,'b':3300}))
    def test_degenerate_bootstrap_is_flagged(self):
        e=estimate({'a':[1]*50},{'a':3300},{'a':20},1000)
        self.assertIsNone(e['ci95']); self.assertIn('not estimable',e['ci_status'])
    def test_stable_extremes(self):
        self.assertEqual(expectation(-1e6),0);self.assertEqual(expectation(1e6),1)

    def dataset(self, games, details=None):
        directory=tempfile.TemporaryDirectory(); self.addCleanup(directory.cleanup)
        path=pathlib.Path(directory.name)/'games.json'
        data={'games':games}
        if details is not None:data['game_details']=details
        path.write_text(json.dumps(data)); return path
    def test_bad_pair(self):
        for games in [[['t','a','1-0','mate']], [['t','a','1-0','mate'],['t','a','0-1','mate']]]:
            with self.assertRaises(ValueError):load(self.dataset(games),'t',{'a':3300})
    def test_missing_anchor(self):
        games=[['t','a','1-0','mate'],['a','t','0-1','mate']]
        with self.assertRaises(ValueError):load(self.dataset(games),'t',{})
    def test_shared_opening_alignment(self):
        games=[];details=[]
        for opponent, openings in [('a',['x','y']),('b',['y','x'])]:
            for fen in openings:
                games.extend([['t',opponent,'1-0' if fen=='x' else '0-1','mate'],[opponent,'t','0-1' if fen=='x' else '1-0','mate']])
                details.extend([{'fen':fen},{'fen':fen}])
        g=load(self.dataset(games,details),'t',{'a':3300,'b':3300})
        self.assertTrue(g.shared); self.assertEqual(g['a'],g['b'])
    def test_repeated_openings_not_independent(self):
        games=[['t','a','1-0','mate'],['a','t','0-1','mate']]*2
        with self.assertRaises(ValueError):load(self.dataset(games,[{'fen':'x'}]*4),'t',{'a':3300})

if __name__=='__main__': unittest.main()
