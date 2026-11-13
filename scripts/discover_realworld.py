#!/usr/bin/env python3
"""Collect public publication attachments; no parser predictions used in selection."""
import concurrent.futures,hashlib,json,subprocess,urllib.parse
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'target/realworld/discovery';OUT.mkdir(parents=True,exist_ok=True)
EXTS={'doc','docx','docm','odt','ods','odp','rtf','ppt','pptx','xls','xlsx','xlsm','xlsb','csv','epub'}
def fetch(url):
 name=hashlib.sha256(url.encode()).hexdigest();p=OUT/(name+'.json')
 if not p.exists():
  r=subprocess.run(['curl','-sS','-L','--fail','--max-time','40',url],capture_output=True)
  if r.returncode:raise RuntimeError(r.stderr.decode()[:150])
  data=json.loads(r.stdout);p.write_text(json.dumps(data))
 return json.loads(p.read_text())
def walk(obj):
 if isinstance(obj,dict):
  if 'url' in obj and isinstance(obj['url'],str):
   suffix=Path(urllib.parse.unquote(urllib.parse.urlparse(obj['url']).path)).suffix[1:].lower()
   if suffix in EXTS:yield obj,suffix
  for v in obj.values():yield from walk(v)
 elif isinstance(obj,list):
  for v in obj:yield from walk(v)
def main():
 queries=['odt','ods','odp','docm','xlsm','rtf','ppt','pptx','docx','xls','xlsx','csv','annual report','statistics tables','consultation response','training presentation','spreadsheet calculator']
 pages={}
 for q in queries:
  url='https://www.gov.uk/api/search.json?'+urllib.parse.urlencode({'q':q,'count':35,'fields':'title,link,format'})
  try:
   result=fetch(url)
   for r in result.get('results',[]):pages[r['link']]=r
   print(q,len(result.get('results',[])),flush=True)
  except Exception as e:print(q,e,flush=True)
 pages.update({x:{'title':x,'link':x} for x in ['/government/publications/dcms-science-and-research-advisory-committee','government/publications/spreadsheet-tools-for-users','/guidance/criminal-legal-aid-means-testing']})
 rows=[];errors=[]
 def one(item):
  link,r=item;link='/'+link.lstrip('/');obj=fetch('https://www.gov.uk/api/content'+link)
  orgs=[x.get('title') for x in obj.get('links',{}).get('organisations',[])]
  return [dict(url=a['url'],format=f,title=a.get('title',r['title']),publication_title=r['title'],landing_url='https://www.gov.uk'+link,publisher='; '.join(orgs) or 'GOV.UK',source_group='uk-government',license='Open Government Licence v3 except where otherwise stated; retain source notice',license_url='https://www.nationalarchives.gov.uk/doc/open-government-licence/version/3/',size_hint=a.get('file_size')) for a,f in walk(obj.get('details',{}))]
 with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
  future={pool.submit(one,item):item for item in pages.items()}
  for f in concurrent.futures.as_completed(future):
   try:rows+=f.result()
   except Exception as e:errors.append({'page':future[f][0],'error':str(e)})
 dedup={r['url']:r for r in rows};rows=list(dedup.values());(OUT/'government-candidates.json').write_text(json.dumps(rows,indent=2));(OUT/'government-errors.json').write_text(json.dumps(errors,indent=2))
 from collections import Counter
 print('Candidates',len(rows),dict(Counter(r['format'] for r in rows)),flush=True)
if __name__=='__main__':main()
