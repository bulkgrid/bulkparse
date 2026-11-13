#!/usr/bin/env python3
"""Untimed allocator diagnostics for both builds on the frozen mixed corpus."""
import hashlib,json,os,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'reports/biff-legacy-2026-11-13'
manifest=json.loads((ROOT/'reports/mixed-200-2026-11-13/corpus-manifest.json').read_text())
inputs=''.join(str(ROOT/d['path'])+'\n' for d in manifest)
results={};hashes={}
for name in ['before','after']:
 exe=OUT/(name+'-allocation-profile');hashes[name]=hashlib.sha256(exe.read_bytes()).hexdigest()
 raw=OUT/(name+'-allocations.jsonl')
 if not raw.exists():
  r=subprocess.run([str(exe),'--structured'],input=inputs,text=True,capture_output=True,check=True,env=dict(os.environ,RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1'))
  raw.write_text(r.stdout);(OUT/(name+'-allocations.log')).write_text(r.stderr)
 rows=[json.loads(line) for line in raw.read_text().splitlines()];assert len(rows)==200
 assert all(row['path']==str((ROOT/d['path'])) for row,d in zip(rows,manifest))
 results[name]=rows;print(name,'peak parser/render live bytes',max(r['rendered']['peak'] for r in rows),flush=True)
changed=[]
for d,a,b in zip(manifest,results['before'],results['after']):
 fields=[]
 for phase in ['parsed','rendered','transport']:
  for metric in ['live','peak','allocated','calls']:
   if a[phase][metric]!=b[phase][metric]:fields.append(phase+'.'+metric)
 if fields:changed.append(dict(id=d['id'],fields=fields))
summary=dict(binary_sha256=hashes,scope='Instrumented Rust allocation counters, not timing or whole-process RSS; parser/render and transport snapshots on all200 in the same order.',profiles={name:dict(max_parser_render_peak_bytes=max(r['rendered']['peak'] for r in rows),max_transport_peak_bytes=max(r['transport']['peak'] for r in rows),total_parser_render_allocated_bytes=sum(r['rendered']['allocated'] for r in rows)) for name,rows in results.items()},changed=changed)
(OUT/'allocations-summary.json').write_text(json.dumps(summary,indent=2)+'\n');print('Changed allocator profiles:',len(changed),flush=True)
