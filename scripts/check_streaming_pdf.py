#!/usr/bin/env python3
"""Byte/diagnostic regression check for the shared renderer on 200 PDFs; no new quality score."""
import hashlib,json,os
from pathlib import Path
from benchmark_realworld import ROOT,Worker,digest,write
OUT=ROOT/ os.environ.get('BULKPARSE_CHECK_REPORT','reports/streaming-2026-11-12')
def main():
 paths=sorted((ROOT/'target/opendataloader-bench/pdfs').glob('*.pdf'));assert len(paths)==200
 commands={'before':[OUT/'before-office-bench'],'default':[ROOT/'target/release/examples/office_bench'],'stream':[ROOT/'target/release/examples/office_bench','--stream-markdown']}
 hashes={k:digest(v[0]) for k,v in commands.items()};inputs={str(p.relative_to(ROOT)):digest(p) for p in paths}
 workers={k:Worker(list(map(str,v)),OUT/(k+'-pdf-check.log'),dict(os.environ,RAYON_NUM_THREADS='1')) for k,v in commands.items()}
 rows=[]
 try:
  for i,p in enumerate(paths):
   results={}
   for name,w in workers.items():
    r=w.call(p);md=r.pop('markdown');r.pop('elapsed_ns',None);r['output_sha256']=hashlib.sha256(md.encode()).hexdigest();results[name]=r
   differences={name:[k for k in ['ok','status','diagnostics','render_diagnostics','output_sha256','error_code'] if r.get(k)!=results['before'].get(k)] for name,r in results.items() if name!='before'}
   rows.append({'path':str(p.relative_to(ROOT)),'results':results,'differences':differences})
   if (i+1)%50==0:print(i+1,'PDFs checked',flush=True)
 finally:
  for w in workers.values():w.close()
 assert all(digest(commands[k][0])==h for k,h in hashes.items())
 assert all(digest(ROOT/p)==h for p,h in inputs.items())
 failures=[r['path'] for r in rows if any(r['differences'].values())]
 write(OUT/'pdf-regression-checks.json',{'method':__doc__,'binary_sha256':hashes,'input_sha256':inputs,'documents':rows,'changed_documents':failures})
 assert not failures,failures
 print('All 200 PDFs unchanged')
if __name__=='__main__':main()
