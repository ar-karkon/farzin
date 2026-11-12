import pathlib,time,subprocess,json,hashlib,sys
root=pathlib.Path('/workspace/farzin'); out=root/'benchmarks/ccrl-phase2'
# Avoid compiling or running another tournament while the first batch searches.
while pathlib.Path('/proc/729/stat').exists():
 if pathlib.Path('/proc/729/stat').read_text().split()[2]=='Z':break
 time.sleep(10)
src=pathlib.Path('/workspace/inputs/berserk/src')
cmd=['gcc','-std=gnu11','-O3','-flto','-DNDEBUG','-DVERSION="9"','-DEVALFILE="networks/berserk-27c2bc72e784.nn"','-mavx2','-mbmi2','-mpopcnt','-DUSE_PEXT']+[str(x.relative_to(src)) for x in sorted(src.glob('*.c'))]+['pyrrhic/tbprobe.c']+[str(x.relative_to(src)) for x in sorted((src/'noobprobe').glob('*.c'))]+['-pthread','-lm','-o','berserk-phase2']
(out/'berserk-build.command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with (out/'berserk-build.log').open('w') as f:subprocess.run(cmd,cwd=src,stdout=f,stderr=subprocess.STDOUT,check=True)
p=src/'berserk-phase2';m=json.loads((out/'manifest.json').read_text());m['opponents']={'Berserk-9':dict(path=str(p),sha256=hashlib.sha256(p.read_bytes()).hexdigest(),source='https://github.com/jhonnold/berserk/tree/d6c81447c0bddfdf56a30628f4f1ca36ec04a117',commit='d6c81447c0bddfdf56a30628f4f1ca36ec04a117',license='GPL-3.0',network='berserk-27c2bc72e784.nn',network_sha256=hashlib.sha256((src/'networks/berserk-27c2bc72e784.nn').read_bytes()).hexdigest(),build='berserk-build.command.json',blitz=dict(rating=3631,error=13,cpu=1,row='Berserk 9 64-bit 3631 +13 −13 57.5% −50.0 62.9% 1650',url='https://computerchess.org.uk/ccrl/404/rating_list_all.html'))};m['protocol'].update(pairs=2,stopping='Fixed two-pair independent-family diagnostic selected after first two SF16 pairs were all losses; adaptive selection, not confirmatory pooled inference');(out/'berserk-manifest.json').write_text(json.dumps(m,indent=2)+'\n')
subprocess.run([str(root/'.venv/bin/python'),'tools/ccrl/run_phase2.py','--manifest',str(out/'berserk-manifest.json'),'--out',str(out/'raw/berserk')],cwd=root,check=True)
