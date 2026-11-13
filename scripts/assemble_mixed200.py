#!/usr/bin/env python3
"""Freeze a 200-document development corpus, retaining all original 112 inputs.
Selection uses source metadata only, never converter outcomes. No input redistribution.
"""
import collections, datetime, hashlib, json, re, subprocess, urllib.parse, zipfile
from pathlib import Path
from prepare_realworld_corpus import sha, write
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'reports/mixed-200-2026-11-13'
SEED='bulkparse-mixed200-2026-11-13-v1'
TARGETS={**dict.fromkeys('csv doc docx epub ods odt rtf xls xlsx'.split(),12),**dict.fromkeys('odp ppt pptx xlsm'.split(),10),'pdf':12,**dict.fromkeys('docm xlsb txt md ipynb tsv'.split(),5),**dict.fromkeys('pps pot pptm ppsx ppsm'.split(),2)}
def package(p):
 if not zipfile.is_zipfile(p):return sha(p.read_bytes())
 with zipfile.ZipFile(p) as z:
  # Include worksheet values and all body parts; exclude volatile package metadata.
  names=sorted(n for n in z.namelist() if n=='content.xml' or n.startswith(('word/','xl/','ppt/')) and n.endswith('.xml') and '/_rels/' not in n)
  return sha(b'\n'.join(n.encode()+b'\0'+z.read(n) for n in names)) if names else sha(p.read_bytes())
def pool():
 rows=[]
 for p in (ROOT/'target/realworld/discovery').glob('*-candidates.json'):rows+=json.loads(p.read_text())
 # Reuse public publication metadata already discovered, adding PDF and rare aliases.
 def walk(x):
  if isinstance(x,dict):
   if isinstance(x.get('url'),str):yield x
   for v in x.values():yield from walk(v)
  elif isinstance(x,list):
   for v in x:yield from walk(v)
 for p in (ROOT/'target/realworld/discovery').glob('*.json'):
  d=json.loads(p.read_text())
  if not isinstance(d,dict) or 'details' not in d:continue
  for a in walk(d['details']):
   f=Path(urllib.parse.urlparse(a['url']).path).suffix[1:].lower()
   if f not in TARGETS:continue
   rows.append(dict(url=a['url'],format=f,title=a.get('title',d.get('title','Publication')),publication_title=d.get('title','Publication'),landing_url='https://www.gov.uk'+d.get('base_path',''),publisher='; '.join(x['title'] for x in d.get('links',{}).get('organisations',[])) or 'GOV.UK',source_group='uk-government',license='OGL v3 except where otherwise stated; retain source notice',license_url='https://www.nationalarchives.gov.uk/doc/open-government-licence/version/3/',size_hint=a.get('file_size')))
 def add(url,f,title,publisher,landing,license,family=None):
  rows.append(dict(url=url,format=f,title=title,publication_title=title,publisher=publisher,landing_url=landing,source_group=publisher,license=license,license_url=landing,related_publication_family=family))
 for book,title in [(2701,'Moby Dick'),(2600,'War and Peace'),(345,'Dracula'),(76,'Huckleberry Finn'),(1952,'The Yellow Wallpaper'),(1232,'The Prince'),(4300,'Ulysses'),(1080,'A Modest Proposal'),(5200,'Metamorphosis'),(2554,'Crime and Punishment')]:
  add(f'https://www.gutenberg.org/ebooks/{book}.epub3.images','epub',title,'Project Gutenberg',f'https://www.gutenberg.org/ebooks/{book}','Retain included Gutenberg license; inputs not redistributed')
 for book,title in [(74,'Tom Sawyer'),(1661,'Sherlock Holmes'),(84,'Frankenstein'),(11,'Alice in Wonderland'),(98,'A Tale of Two Cities')]:
  add(f'https://www.gutenberg.org/ebooks/{book}.txt.utf-8','txt',title,'Project Gutenberg',f'https://www.gutenberg.org/ebooks/{book}','Retain included Gutenberg license; inputs not redistributed',f'gutenberg-{book}')
 for repo in ['pallets/flask','pallets/click','encode/httpx','psf/requests','fastapi/fastapi']:
  add(f'https://raw.githubusercontent.com/{repo}/HEAD/README.md','md',repo+' README',repo,'https://github.com/'+repo,'Upstream repository license; inputs not redistributed')
 for name in ['02.00-Introduction-to-NumPy','02.01-Understanding-Data-Types','02.02-The-Basics-of-NumPy-Arrays','02.03-Computation-on-arrays-ufuncs','02.04-Computation-on-arrays-aggregates']:
  add(f'https://raw.githubusercontent.com/jakevdp/PythonDataScienceHandbook/master/notebooks/{name}.ipynb','ipynb',name,'Jake VanderPlas','https://github.com/jakevdp/PythonDataScienceHandbook/blob/master/notebooks/'+name+'.ipynb','Upstream CC-BY-NC-ND text / MIT code; inputs not redistributed','Python Data Science Handbook chapters')
 add('https://raw.githubusercontent.com/jennybc/gapminder/main/inst/extdata/gapminder.tsv','tsv','Gapminder country-year data','Gapminder / Jenny Bryan','https://github.com/jennybc/gapminder','Upstream data attribution/license; inputs not redistributed')
 return list({r['url']:r for r in rows}.values())
def main():
 OUT.mkdir(parents=True,exist_ok=True)
 assert not (OUT/'corpus-manifest.json').exists(),'Frozen manifest already exists'
 old=json.loads((ROOT/'reports/realworld-2026-11-12/corpus-manifest.json').read_text())
 selected=[dict(r,cohort='retained-112') for r in old];rows=pool();write(OUT/'candidate-pool.json',rows)
 used_hash={r['sha256'] for r in old};used_pkg={package(ROOT/r['path']) for r in old};pages={r['landing_url'] for r in old};publishers=collections.Counter(r['publisher'] for r in old)
 prior=set()
 for p in (ROOT/'reports').rglob('*.json'):
  if OUT in p.parents or 'source-snapshot' in p.parts or not any(s in p.name for s in ['manifest','corpus']):continue
  prior.update(re.findall(r'\b[a-f0-9]{64}\b',p.read_text(errors='replace')))
 excluded=[];ranked={f:sorted([r for r in rows if r['format']==f],key=lambda r:sha((SEED+r['url']).encode())) for f in TARGETS}
 def take(f):
  while ranked[f]:
   r=dict(ranked[f].pop(0));reason=None
   if r['landing_url'] in pages:reason='publication already represented'
   elif 'section-62a-planning-application' in r['landing_url']:reason='planning case family excluded before conversion'
   elif publishers[r['publisher']]>=24:reason='publisher cap 24'
   elif (r.get('size_hint') or 0)>15*1024*1024:reason='source exceeds 15 MiB download cap'
   if reason:excluded.append(dict(url=r['url'],reason=reason));continue
   path=ROOT/'target/realworld/corpus'/f'{f}-{sha(r["url"].encode())[:16]}.{f}'
   if not path.exists():
    tmp=path.with_suffix('.download');c=subprocess.run(['curl','-sS','-L','--fail','--max-time','25','--max-filesize',str(15*1024*1024),r['url'],'-o',str(tmp)],capture_output=True)
    if c.returncode:excluded.append(dict(url=r['url'],reason='download failure',detail=c.stderr.decode()[:200]));tmp.unlink(missing_ok=True);continue
    tmp.rename(path)
   try:
    b=path.read_bytes();h=sha(b)
    if not b or len(b)>15*1024*1024:raise ValueError('size bounds')
    if b.lstrip()[:30].lower().startswith((b'<html',b'<!doctype html')):raise ValueError('HTML response')
    req={'docx':'word/document.xml','docm':'word/document.xml','xlsx':'xl/workbook.xml','xlsm':'xl/workbook.xml','xlsb':'xl/workbook.bin','pptx':'ppt/presentation.xml','pptm':'ppt/presentation.xml','ppsx':'ppt/presentation.xml','ppsm':'ppt/presentation.xml','odt':'content.xml','ods':'content.xml','odp':'content.xml','epub':'META-INF/container.xml'}
    if f in req:
     with zipfile.ZipFile(path) as z:
      if req[f] not in z.namelist():raise ValueError('wrong package family')
    if f in ['doc','xls','ppt','pps','pot'] and not b.startswith(bytes.fromhex('d0cf11e0a1b11ae1')):raise ValueError('missing OLE signature')
    if f=='pdf' and not b.startswith(b'%PDF-'):raise ValueError('missing PDF signature')
    if f=='rtf' and not b.lstrip().startswith(b'{\\rtf'):raise ValueError('missing RTF signature')
    if f=='ipynb':assert isinstance(json.loads(b)['cells'],list)
    if f in ['txt','md','tsv']:b.decode('utf-8-sig')
    sig=package(path)
    if h in used_hash or sig in used_pkg:raise ValueError('duplicate file or package content')
    if h in prior:raise ValueError('previously recorded exact input')
   except Exception as ex:excluded.append(dict(url=r['url'],reason=str(ex)));continue
   r.update(id=path.stem,path=str(path.relative_to(ROOT)),sha256=h,bytes=len(b),package_signature=sig,prior_exact_matches=[],cohort='added-88',downloaded_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
   selected.append(r);used_hash.add(h);used_pkg.add(sig);pages.add(r['landing_url']);publishers[r['publisher']]+=1
   print('Selected',len(selected),f,r['title'][:65],flush=True);write(OUT/'assembly-progress.json',selected);return True
  return False
 # Scarce formats first, round-robin to reduce publisher depletion.
 for _ in range(max(TARGETS.values())):
  for f in sorted(TARGETS,key=lambda f:len([r for r in rows if r['format']==f])):
   if sum(r['format']==f for r in selected)<TARGETS[f]:take(f)
 while len(selected)<200:
  progress=False
  for f in sorted(TARGETS,key=lambda f:sum(r['format']==f for r in selected)):
   if len(selected)==200:break
   progress=take(f) or progress
  if not progress:raise RuntimeError('Insufficient eligible sources')
 assert len(selected)==len(used_hash)==200
 assert all(sha((ROOT/r['path']).read_bytes())==r['sha256'] for r in selected)
 write(OUT/'corpus-manifest.json',selected);write(OUT/'corpus-exclusions.json',excluded)
 write(OUT/'corpus-freeze.json',dict(frozen_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),seed=SEED,targets=TARGETS,counts=dict(collections.Counter(r['format'] for r in selected)),publishers=dict(publishers),manifest_sha256=sha((OUT/'corpus-manifest.json').read_bytes()),candidate_pool_sha256=sha((OUT/'candidate-pool.json').read_bytes()),assembler_sha256=sha(Path(__file__).read_bytes()),retained=112,added=88,prior_hashes_checked=len(prior),selection='SHA-256 ranked public sources; one publication landing URL; publisher cap24; 15 MiB cap; scarce-format shortages redistributed before conversion',limitations='Development corpus includes112 previously used inputs; predominantly English public-sector publications plus books, technical docs and related notebook chapters. Not a random customer sample or independent quality test. Rare extensions may be absent. Inputs not redistributed.'))
 print('FROZEN',collections.Counter(r['format'] for r in selected),flush=True)
if __name__=='__main__':main()
