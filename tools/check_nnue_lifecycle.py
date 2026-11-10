#!/usr/bin/env python3
"""Optional local NNUE UCI lifecycle check (python-chess required, no downloads)."""
import subprocess,json,hashlib,argparse
from pathlib import Path
import chess
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('engine')
parser.add_argument('network')
parser.add_argument('--commit', required=True)
parser.add_argument('--network-kind', default='unverified')
parser.add_argument('--expected-backend', choices=['scalar','avx2'])
parser.add_argument('--json', required=True)
args=parser.parse_args()
if '\n' in args.network or '\r' in args.network:parser.error('network path cannot contain line breaks')
binary=str(Path(args.engine).resolve())
with subprocess.Popen([binary],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,bufsize=1) as engine:
 def send(s):engine.stdin.write(s+'\n');engine.stdin.flush()
 def until(prefix):
  lines=[]
  while True:
   line=engine.stdout.readline().strip()
   if not line:raise RuntimeError('engine exited')
   lines.append(line)
   if line.startswith(prefix):return lines
 send('uci');identity=until('uciok')
 send('setoption name NNUE Stack value true');send('setoption name NNUE SIMD value true');send('setoption name Threads value 2');send('setoption name EvalFile value '+str(Path(args.network).resolve()));send('isready');setup=until('readyok')
 backend=[s.rsplit(' ',1)[1] for s in setup if s.startswith('info string nnue backend ')][-1]
 if args.expected_backend:assert backend==args.expected_backend
 send('position startpos');send('go infinite');before=until('info depth 2 ')
 send('stop');send('isready');stopped=until('readyok')
 best=[s.split()[1] for s in stopped if s.startswith('bestmove ')]
 assert len(best)==1 and chess.Move.from_uci(best[0]) in chess.Board().legal_moves
 # Replacement must unwind workers and release all network owners before backend mutation.
 send('go infinite');send('setoption name NNUE SIMD value false');send('isready');replaced=until('readyok')
 assert not any(s.startswith('bestmove ') for s in replaced)
 assert 'info string nnue backend scalar' in replaced
 send('setoption name NNUE SIMD value true');send('position startpos');send('go depth 2');restart=until('bestmove ')
 assert chess.Move.from_uci(restart[-1].split()[1]) in chess.Board().legal_moves
 send('quit');engine.wait(timeout=5);assert engine.returncode==0
report={'commit':args.commit,'build_features':['nnue-stack'],'binary_sha256':hashlib.sha256(Path(binary).read_bytes()).hexdigest(),'network_kind':args.network_kind,'network_sha256':hashlib.sha256(Path(args.network).read_bytes()).hexdigest(),'initial_backend':backend,'threads':2,'identity':identity,'setup':setup,'before_stop':before,'stop':stopped,'replacement_and_backend_toggle':replaced,'restart':restart,'assertions_passed':True}
Path(args.json).write_text(json.dumps(report,indent=2)+'\n')
print(f'NNUE stack + {backend} + Threads=2: stop, replacement, backend toggle and restart passed')
