#!/usr/bin/env python3
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
import hashlib,json,subprocess,re,unicodedata
ROOT=Path(__file__).resolve().parents[1];DATA=ROOT/'target/independent-pdf-v2';SPEC=ROOT/'benchmarks/independent-pdf-v2'
def words(s):return re.findall(r'[^\W_]+',unicodedata.normalize('NFKC',s).casefold())
def shingles(s):
 w=words(s);return set(tuple(w[i:i+5]) for i in range(len(w)-4))
def main():
 prior={r['sha256']:r for r in json.loads((SPEC/'prior-pdf-inventory.json').read_text())}
 out=DATA/'prior-text';out.mkdir(exist_ok=True)
 def extract(item):
  digest,r=item;p=out/(digest+'.txt')
  try:
   if not p.exists():
    run=subprocess.run([str(ROOT/'target/holdout-pdfkit'),'extract',str(ROOT/r['path']),str(p)],capture_output=True,timeout=20)
    if run.returncode:return {'sha256':digest,'path':r['path'],'error':'PDFKit exit '+str(run.returncode)}
   return {'sha256':digest,'path':r['path'],'text_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'shingles':shingles(p.read_text())}
  except subprocess.TimeoutExpired:return {'sha256':digest,'path':r['path'],'error':'PDFKit timeout'}
 with ThreadPoolExecutor(max_workers=4) as pool:prev=list(pool.map(extract,prior.items()))
 manifest=json.loads((SPEC/'manifest.json').read_text());audit=[]
 for r in manifest['rows']:
  d=json.loads((ROOT/r['paths']['cells']).read_text());s=shingles(' '.join(c['text'] for c in d['cells']))
  matches=[]
  for old in prev:
   if 'shingles' not in old:continue
   shared=len(s & old['shingles']);denom=min(len(s),len(old['shingles']))
   if shared>=50 and denom and shared/denom>=.8:matches.append({'prior_path':old['path'],'prior_sha256':old['sha256'],'shared_shingles':shared,'containment':shared/denom})
  audit.append({'image_id':r['image']['id'],'id':Path(r['image']['file_name']).stem,'probable_overlap':matches,'excluded':bool(matches) or 'excluded' in r})
 for r in prev:r.pop('shingles',None)
 result={'policy':'Frozen 5-token containment >=0.8 and >=50 shared shingles. Independent native cells vs prior PDFKit text; no tested parser outputs used. No replacements.','prior_pdfkit_checks':prev,'rows':audit}
 (SPEC/'overlap-audit.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps({'selected':len(audit),'excluded':sum(r['excluded'] for r in audit),'prior_extraction_failures':[r for r in prev if 'error' in r],'overlaps':[r for r in audit if r['probable_overlap']]},indent=2))
if __name__=='__main__':main()
