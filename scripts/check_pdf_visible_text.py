#!/usr/bin/env python3
"""Account for native text changes: only diagnosed off-page omissions are allowed.

Counts are compared per page with Unicode normalization disabled. This does not
validate reading order or prove geometric correctness; the latter is covered by
page-boundary tests and visual review. Inspection must remain identical.
"""
import argparse, hashlib, json, re
from collections import Counter
from pathlib import Path
from pdf_regression_support import convert, page_characters
ROOT=Path(__file__).resolve().parents[1]
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__)
 for k in ['before','after','pdf_dir','out']:p.add_argument('--'+k.replace('_','-'),type=Path,required=True)
 p.add_argument('--manifest',type=Path,action='append',default=[],help='Additional JSONL input manifests');a=p.parse_args()
 if a.out.exists():raise ValueError('Use a fresh output path')
 paths=set(a.pdf_dir.glob('*.pdf'))
 if a.manifest:
  for m in a.manifest:
   for r in map(json.loads,m.read_text().splitlines()):
    path=Path(r['source_path'])
    if path.suffix.lower()=='.pdf':
     assert sha(path)==r['source_sha256'];paths.add(path.resolve())
 records=[]
 for path in sorted(paths):
  oc,old=convert(a.before,path,True);nc,new=convert(a.after,path,True)
  r={'source':str(path),'sha256':sha(path),'before_exit':oc,'after_exit':nc}
  if oc not in (0,2):r.update(passed=nc==oc,expected_failure=True);records.append(r);continue
  if nc not in (0,2) or new is None:r['passed']=False;records.append(r);continue
  before,after=page_characters(old['document']),page_characters(new['document']);reported=Counter()
  for d in new['diagnostics']:
   if d['code']=='pdf_offpage_text':
    reported[d['source']['page']]+=int(re.search(r'Omitted (\d+)',d['message'])[1])
  pages=[]
  for page in set(before)|set(after)|set(reported):
   removed=before.get(page,Counter())-after.get(page,Counter());added=after.get(page,Counter())-before.get(page,Counter())
   pages.append({'page':page,'removed_characters':sum(removed.values()),'added_characters':sum(added.values()),'diagnosed_offpage_characters':reported[page],'passed':not added and sum(removed.values())==reported[page]})
  r.update(pages=pages,inspection_unchanged=old['pdf_inspection']==new['pdf_inspection'])
  r['passed']=oc==nc and r['inspection_unchanged'] and all(v['passed'] for v in pages);records.append(r)
 result={'scope':__doc__,'passed':all(r['passed'] for r in records),'before_binary_sha256':sha(a.before),'after_binary_sha256':sha(a.after),'harness_sha256':sha(Path(__file__)),'records':records}
 a.out.write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps({'passed':result['passed'],'documents':len(records),'documents_with_offpage_omissions':sum(any(v['removed_characters'] for v in r.get('pages',[])) for r in records),'failures':[r for r in records if not r['passed']]},indent=2));raise SystemExit(0 if result['passed'] else 1)
if __name__=='__main__':main()
