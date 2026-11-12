#!/usr/bin/env python3
"""Run the frozen Phase 2 pilot with hash gates and a fresh output prefix.

Paths may be adapted in a copy of the manifest; hashes must remain unchanged.
Do not run this concurrently with another tournament on the same CPU quota.
"""
import argparse
import hashlib
import json
import pathlib
import subprocess
import sys

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--manifest', default='benchmarks/ccrl-phase2/manifest.json')
p.add_argument('--out', required=True)
a = p.parse_args()
m = json.loads(pathlib.Path(a.manifest).read_text())
base = pathlib.Path(a.out)
base.parent.mkdir(parents=True, exist_ok=True)
if any(base.with_suffix(s).exists() for s in ('.json', '.pgn', '.log', '.command.json')):
    p.error('output prefix exists; preserve prior evidence and choose another')
for entry in [m['farzin'], m['book'], *m['opponents'].values()]:
    if hashlib.sha256(pathlib.Path(entry['path']).read_bytes()).hexdigest() != entry['sha256']:
        p.error('input checksum mismatch: ' + entry['path'])
quota, period = pathlib.Path('/sys/fs/cgroup/cpu.max').read_text().split()
cpus = 2 if quota == 'max' else int(quota)//int(period)
if cpus < 1:
    p.error('less than one CPU equivalent available')
x = m['protocol']
cmd = [sys.executable, 'tools/arena.py', '--engine', 'name=Farzin-0.5.2,cmd='+m['farzin']['path']]
for name, entry in m['opponents'].items():
    cmd += ['--engine', f"name={name},cmd={entry['path']}"]
cmd += ['--gauntlet', '--tc', x['tc'], '--pairs', str(x['pairs']), '--concurrency', str(min(cpus,x['concurrency'])),
        '--threads', '1', '--hash', str(x['hash_mb']), '--adjudicate', 'none', '--max-plies', str(x['max_plies']),
        '--time-margin', '0', '--seed', str(x['seed']), '--book', m['book']['path'], '--book-plies', str(m['book']['plies']),
        '--out', str(base.with_suffix('.json')), '--pgn', str(base.with_suffix('.pgn'))]
base.with_suffix('.command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with base.with_suffix('.log').open('w') as log:
    subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT, check=True)
