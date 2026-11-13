#!/usr/bin/env python3
"""Rotate frozen persistent workers: warm-up then five sequential sample passes."""
from pathlib import Path
import hashlib,json,subprocess,statistics
from evaluate_independent_pdf import ROOT,DATA,SPEC,VERSIONS,sha

def main():
 out=ROOT/'reports/independent-pdf-v2-2026-11-11'
 if (out/'timing.json').exists():raise ValueError('Existing timing report')
 m=json.loads((SPEC/'manifest.json').read_text());excluded={r['id'] for r in json.loads((SPEC/'overlap-audit.json').read_text())['rows'] if r['excluded']}
 paths=sorted(str((ROOT/r['paths']['pdfs']).resolve()) for r in m['rows'] if Path(r['image']['file_name']).stem not in excluded)
 processes={};logs={};runs={v:[] for v in VERSIONS};hashes={};binaries={}
 try:
  for v in VERSIONS:
   binary=DATA/('build-'+v)/'release/bulkparse-opendataloader-bench';binaries[v]=sha(binary)
   logs[v]=open(out/(v+'-worker.stderr.log'),'w')
   processes[v]=subprocess.Popen([str(binary)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=logs[v],text=True)
  for round in range(6):
   order=VERSIONS[round%3:]+VERSIONS[:round%3]
   for v in order:
    dest=DATA/'markdown'/v
    req={'documents':paths,'output_dir':str(dest)}
    proc=processes[v];proc.stdin.write(json.dumps(req)+'\n');proc.stdin.flush()
    line=proc.stdout.readline()
    if not line:raise RuntimeError('worker exited: '+v)
    run=json.loads(line);run['warmup']=round==0;run['round']=round;run['version_order']=order
    pred={p.name:sha(p) for p in dest.glob('*.md')};assert len(pred)==len(paths)
    if v in hashes:assert pred==hashes[v]
    else:hashes[v]=pred
    run['prediction_sha256']=pred;runs[v].append(run)
  summary={v:{'median_seconds':statistics.median(r['elapsed_ns'] for r in rs[1:])/1e9,'measured_seconds':[r['elapsed_ns']/1e9 for r in rs[1:]],'errors':sum(not d['ok'] for r in rs for d in r['documents']),'deterministic':True} for v,rs in runs.items()}
  report={'documents':len(paths),'worker_binary_sha256':binaries,'runs':runs,'summary':summary,'harness_sha256':sha(Path(__file__))}
  (out/'timing.json').write_text(json.dumps(report,indent=2)+'\n')
  print(json.dumps(summary,indent=2))
 finally:
  for v,p in processes.items():
   p.stdin.close();code=p.wait(timeout=15);logs[v].close();assert code==0,(v,code)
if __name__=='__main__':main()
