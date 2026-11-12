#!/usr/bin/env python3
"""Hash-gated runner. Preserves frozen Farzin default options except equal Hash.

No paid infrastructure. Sequential phases must not be run concurrently.
"""
import argparse,hashlib,json,pathlib,subprocess,sys
p=argparse.ArgumentParser();p.add_argument('phase',choices=['screening','blitz','4015']);p.add_argument('--manifest',default='benchmarks/ccrl-calibration/manifest.json');p.add_argument('--pairs',type=int);p.add_argument('--out',required=True);p.add_argument('--opponent',action='append');a=p.parse_args()
m=json.load(open(a.manifest)); selected=a.opponent or list(m['opponents'])
if a.phase=='4015': selected=[n for n in selected if '4015' in m['opponents'][n]]
if a.pairs is not None and a.pairs<1: p.error('pairs must be positive')
out=pathlib.Path(a.out);out.parent.mkdir(parents=True,exist_ok=True)
if out.with_suffix('.json').exists() or out.with_suffix('.pgn').exists():p.error('output exists; use a new prefix')
cmd=[sys.executable,'tools/arena.py']
for name,d in [('Farzin-0.5.2',m['farzin'])]+[(n,m['opponents'][n]) for n in selected]:
 if hashlib.sha256(pathlib.Path(d['path']).read_bytes()).hexdigest()!=d['sha256']:p.error('binary hash mismatch: '+name)
 cmd+=['--engine',f"name={name},cmd={d['path']}"]
b=m['book'];assert hashlib.sha256(pathlib.Path(b['path']).read_bytes()).hexdigest()==b['sha256']
quota=pathlib.Path('/sys/fs/cgroup/cpu.max').read_text().split()
concurrency=min(2,int(quota[0])//int(quota[1])) if quota[0]!='max' else 2
cmd+=['--gauntlet','--tc',{'screening':'10+0.1','blitz':'120+1','4015':'40/900'}[a.phase],'--pairs',str(a.pairs or {'screening':20,'blitz':25,'4015':20}[a.phase]),'--concurrency',str(max(1,concurrency)),'--threads','1','--hash','256' if a.phase=='4015' else '128','--adjudicate','conservative','--time-margin','0','--seed','20261112','--book',b['path'],'--book-plies',str(b['plies']),'--out',str(out.with_suffix('.json')),'--pgn',str(out.with_suffix('.pgn'))]
out.with_suffix('.command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with out.with_suffix('.log').open('w') as log:subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT,check=True)
