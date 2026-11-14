#!/usr/bin/env python3
"""Frozen independent corpus: eight converters, one warm-up, five measured passes."""
import argparse,hashlib,json,os,platform,re,selectors,shutil,signal,statistics,subprocess,time
from pathlib import Path
from benchmark_opendataloader import snapshot_sources
ROOT=Path(__file__).resolve().parents[1]
REPORT=ROOT/'reports/realworld-2026-11-12'
ENGINES=['bulkparse','anydoc','docling','markitdown','pandoc','mammoth','libreoffice','unstructured']
ALL=set('doc docx epub odp ods odt ppt pptx rtf xls xlsm xlsx csv'.split())
SUPPORT={'bulkparse':ALL,'anydoc':ALL,'docling':ALL,
 'markitdown':set('docx xlsx xls pptx epub csv'.split()),
 'pandoc':set('docx odt epub rtf csv'.split()),'mammoth':{'docx'},
 'libreoffice':ALL-{'epub'},'unstructured':ALL-{'ods','odp'}}
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,obj):p.write_text(json.dumps(obj,indent=2,ensure_ascii=False)+'\n')
def commands():
 py=ROOT/'target/competitors/broad-venv/bin/python'
 c={e:[str(py),str(ROOT/'scripts/worker_realworld.py'),e] for e in ENGINES}
 c['bulkparse']=[str(ROOT/'target/release/examples/office_bench')]
 c['anydoc']=[str(ROOT/'target/multiformat-anydoc/release/anydoc-bench-worker')]
 c['docling'][0]=str(ROOT/'target/competitors/docling-venv/bin/python')
 c['unstructured'][0]=str(ROOT/'target/competitors/realworld-venv/bin/python')
 return c
class Worker:
 def __init__(self,cmd,log,env):
  self.log=log.open('w');self.p=subprocess.Popen(['/usr/bin/time','-l',*cmd],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log,env=env,start_new_session=True,cwd=ROOT)
  self.pending=b'';self.sel=selectors.DefaultSelector();self.sel.register(self.p.stdout,selectors.EVENT_READ)
 def receive(self,timeout):
  end=time.monotonic()+timeout;chunks=[self.pending] if self.pending else []
  while not chunks or b'\n' not in chunks[-1]:
   if time.monotonic()>=end:raise TimeoutError('90-second conversion deadline exceeded')
   if not self.sel.select(min(1,max(0,end-time.monotonic()))):continue
   b=os.read(self.p.stdout.fileno(),1<<20)
   if not b:raise RuntimeError('Worker exited before returning output')
   chunks.append(b)
  b,self.pending=b''.join(chunks).split(b'\n',1);return json.loads(b)
 def call(self,p):
  self.p.stdin.write((json.dumps({'documents':[str(p)]})+'\n').encode());self.p.stdin.flush()
  return self.receive(90)['documents'][0]
 def close(self,kill=False):
  if kill:
   try:os.killpg(self.p.pid,signal.SIGKILL)
   except ProcessLookupError:pass
  else:
   try:self.p.stdin.close();self.p.wait(timeout=15)
   except (BrokenPipeError,subprocess.TimeoutExpired):
    try:os.killpg(self.p.pid,signal.SIGKILL)
    except ProcessLookupError:pass
  self.p.wait();self.sel.close();self.p.stdout.close();self.log.close();return self.p.returncode
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--smoke',action='store_true');ap.add_argument('--out',type=Path,required=True);ap.add_argument('--engines',nargs='+',choices=ENGINES,default=ENGINES);a=ap.parse_args()
 out=a.out.resolve();out.mkdir(parents=True,exist_ok=False);corpus=json.loads((REPORT/'corpus-manifest.json').read_text())
 if a.smoke:corpus=[next(r for r in corpus if r['format']==f) for f in sorted(ALL)]
 assert all(digest(ROOT/r['path'])==r['sha256'] for r in corpus)
 write(out/'manifest.json',corpus);cmds=commands();sources=snapshot_sources(out)
 paths=[ROOT/'scripts/benchmark_realworld.py',ROOT/'scripts/worker_realworld.py',ROOT/'crates/bulkparse/examples/office_bench.rs',ROOT/'benchmarks/multiformat/anydoc.rs',ROOT/'benchmarks/realworld/protocol.json']
 paths+=list((ROOT/'legacy/anydoc-main/src').rglob('*.rs'))+list((ROOT/'legacy/anydoc-main').glob('Cargo.*'))+list((ROOT/'benchmarks/multiformat/anydoc').glob('Cargo.*'))
 for p in paths:
  name=str(p.relative_to(ROOT));sources[name]=digest(p);dest=out/'source-snapshot'/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dest)
 write(out/'source-manifest.json',sources)
 for name in ['broad','docling','realworld']:
  py=ROOT/f'target/competitors/{name}-venv/bin/python';(out/f'{name}-requirements.txt').write_text(subprocess.check_output([str(py),'-m','pip','freeze'],text=True))
 env=dict(os.environ,RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1',HF_HUB_OFFLINE='1',TRANSFORMERS_OFFLINE='1',TOKENIZERS_PARALLELISM='false')
 identity={'hardware':subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string','hw.memsize','hw.logicalcpu'],text=True).splitlines(),'platform':platform.platform(),'commands':cmds,'binary_sha256':{e:digest(Path(cmds[e][0])) for e in a.engines},'warmup':1,'passes':0 if a.smoke else 5,'deadline_seconds':90,'support':{e:sorted(SUPPORT[e]) for e in a.engines},'environment':{k:env[k] for k in ['RAYON_NUM_THREADS','OMP_NUM_THREADS','OPENBLAS_NUM_THREADS','MKL_NUM_THREADS','HF_HUB_OFFLINE','TRANSFORMERS_OFFLINE']},'smoke':a.smoke,'started_utc':__import__('datetime').datetime.now(__import__('datetime').timezone.utc).isoformat()}
 write(out/'identity.json',identity)
 workers={};generations={e:0 for e in a.engines};ready={};resources={};runs={e:[] for e in a.engines}
 def start(e):
  g=generations[e];generations[e]+=1;w=Worker(cmds[e],out/f'{e}-worker-{g}.log',env)
  if e not in ['bulkparse','anydoc']:ready[e]=w.receive(240);assert ready[e]['ready'];write(out/'effective-options.json',ready)
  workers[e]=w;return w
 try:
  for i in range(1 if a.smoke else 6):
   order=a.engines[i%len(a.engines):]+a.engines[:i%len(a.engines)]
   for e in order:
    selected=[r for r in corpus if r['format'] in SUPPORT[e]];rows=[];folder=out/'prediction'/e/f'round-{i}';folder.mkdir(parents=True)
    print(f'Start round {i} {e}: {len(selected)} documents',flush=True)
    for n,r in enumerate(selected):
     if e not in workers:start(e)
     started=time.perf_counter_ns()
     try:row=workers[e].call(ROOT/r['path']);assert row['path']==str(ROOT/r['path'])
     except (TimeoutError,RuntimeError,BrokenPipeError) as ex:
      row={'path':str(ROOT/r['path']),'elapsed_ns':time.perf_counter_ns()-started,'ok':False,'status':'timeout_or_worker_failure','error':str(ex),'markdown':''};workers.pop(e).close(kill=True)
     md=row.pop('markdown');p=folder/(r['id']+'.md');p.write_text(md);row.update(id=r['id'],format=r['format'],output_sha256=digest(p),output_bytes=p.stat().st_size);rows.append(row)
     with (out/f'{e}-round-{i}.jsonl').open('a') as f:f.write(json.dumps(row)+'\n')
     if (n+1)%20==0 or not row['ok']:print(f'  {n+1}/{len(selected)} {r["id"]} {row["elapsed_ns"]/1e9:.3f}s {"ok" if row["ok"] else row.get("error")}',flush=True)
    runs[e].append(rows);write(out/f'{e}-round-{i}.json',{'index':i,'order':order,'documents':rows})
    print(f'Done {e}: {sum(r["elapsed_ns"] for r in rows)/1e9:.3f}s; {sum(r["ok"] for r in rows)}/{len(rows)} returned',flush=True)
 finally:
  for e,w in workers.items():resources[e]={'last_worker_exit':w.close()}
  for e in a.engines:
   peaks=[int(v) for p in out.glob(e+'-worker-*.log') for v in re.findall(r'(\d+)\s+maximum resident set size',p.read_text(errors='replace'))]
   resources.setdefault(e,{}).update(worker_starts=generations[e],lifetime_peak_rss_bytes=max(peaks) if peaks else None,scope='worker lifetime peak; not aggregate process-tree memory; killed workers may lack samples')
  write(out/'resources.json',resources)
 assert all(digest(ROOT/n)==h for n,h in sources.items()),'Sources changed during measurement'
 assert all(digest(ROOT/r['path'])==r['sha256'] for r in corpus)
 if a.smoke:return
 summary={}
 for e,rs in runs.items():
  docs=[]
  for j,row in enumerate(rs[1]):
   samples=[run[j] for run in rs[1:]];assert all(x['id']==row['id'] for x in samples)
   docs.append(dict(id=row['id'],format=row['format'],median_ms=statistics.median(x['elapsed_ns']/1e6 for x in samples),returned=sum(x['ok'] for x in samples),nonempty=sum(x['output_bytes']>0 for x in samples),deterministic=len({x['output_sha256'] for x in samples})==1))
  formats={f:{'attempted':len(ds:=[r for r in docs if r['format']==f]),'returned_all_passes':sum(r['returned']==5 for r in ds),'median_ms':statistics.median(r['median_ms'] for r in ds)} for f in sorted({r['format'] for r in docs})}
  summary[e]={'attempted':len(docs),'returned_all_passes':sum(r['returned']==5 for r in docs),'nonempty_all_passes':sum(r['nonempty']==5 for r in docs),'median_ms':statistics.median(r['median_ms'] for r in docs),'format_macro_median_ms':statistics.mean(r['median_ms'] for r in formats.values()),'measured_subset_seconds':[sum(r['elapsed_ns'] for r in run)/1e9 for run in rs[1:]],'formats':formats,'documents':docs,'resources':resources[e],'quality_prediction_round':1,'deterministic':all(r['deterministic'] for r in docs)}
 write(out/'summary.json',summary);write(out/'verification.json',{'all_input_hashes_unchanged':True,'all_source_hashes_unchanged':True,'complete':True})
 print('COMPLETE',flush=True)
if __name__=='__main__':main()
