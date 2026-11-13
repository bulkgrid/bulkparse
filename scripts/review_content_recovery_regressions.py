#!/usr/bin/env python3
"""Record six-mode hard-case results; approve only source-verified additions."""
import hashlib,json,os,re,copy
from pathlib import Path
from benchmark_realworld import ROOT,Worker,digest,write
from check_realworld_failures import MODES,SUITE
OUT=ROOT/'reports/content-recovery-2026-11-13'

def legacy(ds):
 result=[]
 for d in (ds or []):
  d={k:v for k,v in d.items() if k not in ('category','evidence')}
  if d['code']=='xls_visual_content':d['message']='Drawing, object or comment omitted.'
  result.append(d)
 return result

def key(text):return ''.join(c.casefold() for c in text if c.isalnum())

def main():
 corpus=json.loads((SUITE/'manifest.json').read_text());old=json.loads((SUITE/'expected.json').read_text());new=copy.deepcopy(old)
 sources={r['id']:r['texts'] for r in json.loads((OUT/'xls-text-inventory.json').read_text())}
 checks=[]
 dest=OUT/'regressions';dest.mkdir(exist_ok=False)
 for mode,flags in MODES.items():
  w=Worker([str(OUT/'accepted-office-bench'),*flags],dest/(mode+'.log'),dict(os.environ,RAYON_NUM_THREADS='1'))
  baseline=None;raw=[];review=[]
  try:
   for d in corpus:
    r=w.call(ROOT/d['path']);md=r.pop('markdown');r.update(id=d['id'],output_sha256=hashlib.sha256(md.encode()).hexdigest());raw.append(r)
    want=old[mode]['documents'][d['id']]
    for k,v in want.items():
     if k in ['diagnostics','render_diagnostics']:assert legacy(r.get(k,[]))==legacy(v),(mode,d['id'],k)
     elif k!='output_sha256':assert r.get(k)==v,(mode,d['id'],k)
    if r['output_sha256']!=want['output_sha256']:
     assert r['ok'] and sources.get(d['id']),(mode,d['id'])
     if baseline is None:baseline=Worker([str(OUT/'before-office-bench'),*flags],dest/(mode+'-before.log'),dict(os.environ,RAYON_NUM_THREADS='1'))
     b=baseline.call(ROOT/d['path'])['markdown'];assert hashlib.sha256(b.encode()).hexdigest()==want['output_sha256']
     # Old nonempty lines must survive, unchanged and in their original order.
     lines=iter(md.splitlines());assert all(any(line==oldline for line in lines) for oldline in b.splitlines() if oldline.strip())
     additions=key(md)
     for t in sources[d['id']]:assert key(t['text']) in additions
     # Every extra alphanumeric token belongs to the source text or annotation heading.
     oldkey=key(b);newkey=key(md)
     for t in sources[d['id']]:newkey=newkey.replace(key('Drawing text'+t['text']),'',1)
     assert newkey==oldkey,(mode,d['id'],'unexpected output beyond source-backed caption')
     review.append(dict(id=d['id'],source_texts=sources[d['id']],before_sha256=want['output_sha256'],after_sha256=r['output_sha256']))
    new[mode]['documents'][d['id']]={k:r.get(k) for k in want}
    checks.append(dict(id=d['id'],mode=mode,passed=True))
  finally:
   w.close()
   if baseline:baseline.close()
  p=dest/(mode+'.json');write(p,dict(documents=raw,reviewed_additions=review))
  new[mode]['source']=str(p.relative_to(ROOT));new[mode]['source_sha256']=digest(p)
  print(mode,len(raw),'checked',len(review),'source-backed changes',flush=True)
 write(OUT/'previous-regression-expectations.json',old)
 write(OUT/'reviewed-regression-expectations.json',new)
 write(dest/'review.json',dict(checks=checks,passed=len(checks),policy='Preserve outcomes, all old diagnostic fields, and all original Markdown lines; allow only source-verified chart-caption additions, category metadata and clarified visual-warning wording.'))
 print('Review complete; expectations prepared, not installed.')
if __name__=='__main__':main()
