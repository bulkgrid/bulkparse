#!/usr/bin/env python3
"""Freeze public-document selection before running any converter."""
import collections,datetime,hashlib,json,re,subprocess,zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];BASE=ROOT/'target/realworld';REPORT=ROOT/'reports/realworld-2026-11-12';POLICY=ROOT/'benchmarks/realworld/protocol.json'
def sha(b):return hashlib.sha256(b).hexdigest()
def write(p,v):p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(v,indent=2,ensure_ascii=False)+'\n')
def package_signature(p):
 if zipfile.is_zipfile(p):
  with zipfile.ZipFile(p) as z:
   names=sorted(n for n in z.namelist() if n in ['word/document.xml','content.xml'] or re.fullmatch(r'ppt/slides/slide\d+.xml',n) or n in ['xl/sharedStrings.xml','xl/workbook.xml'])
   if names:return sha(b'\n'.join(z.read(n) for n in names))
 return sha(p.read_bytes())
def main():
 protocol=json.loads(POLICY.read_text());out=REPORT/'corpus-manifest.json'
 if out.exists():raise SystemExit('Frozen manifest exists; do not replace it')
 rows=[]
 for p in sorted((BASE/'discovery').glob('*-candidates.json')):rows+=json.loads(p.read_text())
 rows=list({r['url']:r for r in rows}.values());write(REPORT/'candidate-pool.json',rows)
 # Recorded old hashes plus all small fixture file hashes; no converter output drives selection.
 prior={}
 for p in (ROOT/'reports').rglob('*.json'):
  if REPORT in p.parents or 'source-snapshot' in p.parts:continue
  if 'manifest' not in p.name and 'corpus' not in p.name:continue
  for h in re.findall(r'\b[a-f0-9]{64}\b',p.read_text(errors='replace')):prior.setdefault(h,str(p.relative_to(ROOT)))
 for folder in ['fixtures','legacy/anydoc-main/tests/fixtures','legacy/docling-main/tests/data']:
  for p in (ROOT/folder).rglob('*'):
   if p.is_file() and p.stat().st_size<16*1024*1024:prior.setdefault(sha(p.read_bytes()),str(p.relative_to(ROOT)))
 selected=[];excluded=[];used_pages=set();used_hash=set();used_packages=set();publishers=collections.Counter()
 ranked={fmt:sorted([r for r in rows if r['format']==fmt],key=lambda r:sha((protocol['selection_seed']+r['url']).encode())) for fmt in protocol['formats']}
 formats=sorted(ranked,key=lambda fmt:len(ranked[fmt]))
 def take(fmt):
  while ranked[fmt]:
   r=dict(ranked[fmt].pop(0));reason=None
   if r['landing_url'] in used_pages:reason='publication already represented'
   elif 'section-62a-planning-application' in r['landing_url']:reason='planning case family excluded before conversion'
   elif publishers[r['publisher']]>=16:reason='publisher cap 16'
   elif r.get('size_hint') and r['size_hint']>protocol['max_download_bytes']:reason='source size exceeds limit'
   if reason:excluded.append(dict(url=r['url'],reason=reason));continue
   ident=sha(r['url'].encode())[:16];path=BASE/'corpus'/f'{fmt}-{ident}.{fmt}';path.parent.mkdir(parents=True,exist_ok=True)
   if not path.exists():
    tmp=path.with_suffix('.download')
    proc=subprocess.run(['curl','-sS','-L','--fail','--max-time','45','--max-filesize',str(protocol['max_download_bytes']),r['url'],'-o',str(tmp)],capture_output=True)
    if proc.returncode:excluded.append(dict(url=r['url'],reason='download failure',detail=proc.stderr.decode()[:250]));tmp.unlink(missing_ok=True);continue
    tmp.rename(path)
   data=path.read_bytes();digest=sha(data)
   try:
    if not data or len(data)>protocol['max_download_bytes']:raise ValueError('size bounds')
    if data.lstrip()[:20].lower().startswith((b'<!doctype html',b'<html')):raise ValueError('HTML response')
    if fmt in ['docx','docm','xlsx','xlsm','pptx','odt','ods','odp','epub']:
     if not zipfile.is_zipfile(path):raise ValueError('not a ZIP package')
     with zipfile.ZipFile(path) as z:
      names=set(z.namelist());required={'docx':'word/document.xml','docm':'word/document.xml','xlsx':'xl/workbook.xml','xlsm':'xl/workbook.xml','pptx':'ppt/presentation.xml','odt':'content.xml','ods':'content.xml','odp':'content.xml','epub':'META-INF/container.xml'}[fmt]
      if required not in names:raise ValueError('wrong package family')
    if fmt in ['doc','xls','ppt'] and not data.startswith(bytes.fromhex('d0cf11e0a1b11ae1')):raise ValueError('legacy extension lacks OLE signature')
    if fmt=='rtf' and not data.lstrip().startswith(b'{\\rtf'):raise ValueError('RTF signature missing')
    signature=package_signature(path)
    if digest in prior:raise ValueError('previously recorded input: '+prior[digest])
    if digest in used_hash or signature in used_packages:raise ValueError('duplicate file or package content')
   except Exception as e:excluded.append(dict(url=r['url'],reason=str(e)));continue
   r.update(id=path.stem,path=str(path.relative_to(ROOT)),sha256=digest,bytes=len(data),package_signature=signature,prior_exact_matches=[],downloaded_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
   selected.append(r);used_pages.add(r['landing_url']);used_hash.add(digest);used_packages.add(signature);publishers[r['publisher']]+=1
   print('Selected',len(selected),fmt,r['title'][:80],flush=True);return True
  return False
 for _ in range(protocol['desired_per_format']):
  for fmt in formats:take(fmt)
 while len(selected)<protocol['target_documents']:
  progress=False
  for fmt in sorted(formats,key=lambda f:sum(r['format']==f for r in selected)):
   if len(selected)>=protocol['target_documents']:break
   progress=take(fmt) or progress
  if not progress:break
 if len(selected)<100:raise RuntimeError(f'Only {len(selected)} eligible inputs; expand primary sources before freezing')
 write(out,selected);write(REPORT/'corpus-exclusions.json',excluded)
 write(REPORT/'corpus-freeze.json',dict(frozen_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),protocol_sha256=sha(POLICY.read_bytes()),manifest_sha256=sha(out.read_bytes()),candidate_pool_sha256=sha((REPORT/'candidate-pool.json').read_bytes()),counts=dict(collections.Counter(r['format'] for r in selected)),publishers=dict(publishers),prior_hashes_checked=len(prior),source_limitations='Public-sector and classic-literature sample; predominantly English, convenience source pool, sparse rare formats. Not a random sample of customer uploads. No input redistribution promised.'))
 print('FROZEN',len(selected),collections.Counter(r['format'] for r in selected),flush=True)
if __name__=='__main__':main()
