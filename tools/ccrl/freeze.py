#!/usr/bin/env python3
"""Freeze local inputs and CCRL excerpts. Run from repository root."""
import hashlib,json,subprocess,platform,pathlib
from bs4 import BeautifulSoup
ROOT=pathlib.Path('/workspace')
OUT=pathlib.Path('benchmarks/ccrl-calibration')
def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def cmd(*a):return subprocess.check_output(a,text=True).strip()
engines=[('Weiss-2.0','weiss','src/weiss','GPL-3.0-or-later','make -C src basic WARN="-Wall -Wextra -Wshadow"'),('Ethereal-12.00','ethereal','src/Ethereal','GPL-3.0-or-later','make -C src pext'),('Stockfish-10','sf10','src/stockfish','GPL-3.0-or-later','make -C src build ARCH=x86-64-bmi2 -j2'),('Berserk-4.4.0','berserk','src/berserk-x64-pext','GPL-3.0','make -C src pext')]
m=dict(snapshot='2026-11-07',retrieved_utc=cmd('date','-u','+%FT%TZ'),base_commit=cmd('git','rev-parse','main'),os=platform.platform(),cpu=cmd('lscpu'),cpu_quota=pathlib.Path('/sys/fs/cgroup/cpu.max').read_text().strip(),opponents={})
rows={}
for cat in ['blitz','4015']:
 soup=BeautifulSoup((ROOT/'ccrl-inputs'/f'{cat}-all.html').read_text(),'html.parser')
 rows[cat]=[r.get_text(' ',strip=True) for r in soup.select('tr') if len(r.get_text(' ',strip=True))<300]
for name,repo,exe,license,build in engines:
 p=ROOT/repo
 d=dict(path=str(p/exe),sha256=sha(p/exe),source=cmd('git','-C',str(p),'remote','get-url','origin'),commit=cmd('git','-C',str(p),'rev-parse','HEAD'),license=license,build=build)
 for cat,folder in [('blitz','404'),('4015','4040')]:
  prefix=name.replace('-',' ')+' 64-bit '
  candidates=[r for r in rows[cat] if r.startswith(prefix) and r[len(prefix):].split()[0].isdigit()]
  if candidates:
   r=candidates[0];fields=r[len(prefix):].split();d[cat]=dict(rating=int(fields[0]),error=int(fields[1].lstrip('+')),row=r,url=f'https://computerchess.org.uk/ccrl/{folder}/rating_list_all.html',cpu=1)
 m['opponents'][name]=d
m['farzin']=dict(path='/workspace/ccrl-inputs/farzin',sha256=sha('/workspace/ccrl-inputs/farzin'),identity=cmd('/workspace/ccrl-inputs/farzin','--version'),url='https://github.com/ar-karkon/farzin/releases/download/v0.5.2/farzin-0.5.2-linux-x64-avx2-bmi2')
m['book']=dict(path='tools/ccrl/openings.epd',sha256=sha('tools/ccrl/openings.epd'),positions=20,plies=8,origin='Explicit standard opening lines, curated before results; no engine evaluation filtering')
m['reference_gauntlet_commit']=cmd('git','-C','/workspace/reference-gauntlet','rev-parse','HEAD')
(OUT/'manifest.json').write_text(json.dumps(m,indent=2)+'\n')
