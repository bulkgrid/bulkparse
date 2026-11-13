#!/usr/bin/env python3
"""Independent, source-derived references; never uses a converter's Markdown."""
import argparse,csv,hashlib,io,json,os,shutil,signal,subprocess,sys,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
REPORT=ROOT/'reports/realworld-2026-11-12'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,obj):p.write_text(json.dumps(obj,indent=2,ensure_ascii=False)+'\n')
def one(r):
 import fitz
 p=ROOT/r['path'];assert sha(p)==r['sha256'];out=REPORT/'references'/r['id'];out.mkdir(parents=True,exist_ok=True)
 result={'id':r['id'],'source_sha256':r['sha256'],'format':r['format'],'images':[]}
 if r['format']=='csv':
  data=p.read_bytes();text=data.decode('utf-8-sig',errors='surrogateescape')
  def parse_csv(value):
   try:dialect=csv.Sniffer().sniff(value[:32768],delimiters=',;\t|')
   except csv.Error:dialect=csv.excel
   return list(csv.reader(io.StringIO(value),dialect)),dialect
  rows,dialect=parse_csv(text);selected=rows[:60]
  prefix_invalid=any(0xDC80<=ord(c)<=0xDCFF for row in selected for field in row for c in field)
  full_file_valid_utf8=not any(0xDC80<=ord(c)<=0xDCFF for c in text)
  if prefix_invalid:rows,dialect=parse_csv(data.decode('cp1252'));selected=rows[:60];enc='cp1252 fallback for source records containing invalid UTF-8 bytes'
  else:enc='utf-8-sig for selected records'
  reference={'scope':'First 60 source records, including header; all columns. Judge cell values and row/column relations. Original CSV has no rich visual formatting.','encoding':enc,'full_file_valid_utf8':full_file_valid_utf8,'delimiter':dialect.delimiter,'total_records':len(rows),'records':selected}
  dest=out/'source-records.json';write(dest,reference);result.update(kind='source_csv',text_path=str(dest.relative_to(ROOT)),text_sha256=sha(dest),record_count=len(selected),ok=bool(selected))
 else:
  if r['format']=='epub':
   doc=fitz.open(p);doc.layout(width=595,height=842,fontsize=11)
   result.update(kind='epub_source_render',renderer='MuPDF EPUB renderer, A4, 11pt; source CSS interpreted by renderer',total_pages=len(doc))
  else:
   soffice=shutil.which('soffice');assert soffice
   with tempfile.TemporaryDirectory(prefix='realworld-reference-') as td:
    tmp=Path(td);profile=tmp/'profile';(profile/'user').mkdir(parents=True)
    (profile/'user/registrymodifications.xcu').write_text('<oor:items xmlns:oor="http://openoffice.org/2001/registry"><item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop></item></oor:items>')
    family='calc' if r['format'] in ['xls','xlsx','xlsm','ods'] else 'impress' if r['format'] in ['ppt','pptx','odp'] else 'writer'
    filt='pdf:'+family+'_pdf_Export:'+json.dumps({'PageRange':{'type':'string','value':'1-6'}})
    run=subprocess.run([soffice,'-env:UserInstallation='+profile.as_uri(),'--headless','--norestore','--nologo','--nodefault','--convert-to',filt,'--outdir',td,str(p)],capture_output=True,timeout=100)
    pdf=tmp/(p.stem+'.pdf')
    if run.returncode or not pdf.exists():raise RuntimeError('Reference PDF export failed: '+run.stderr.decode(errors='replace')[:1000]+run.stdout.decode(errors='replace')[:1000])
    shutil.copy2(pdf,out/'reference.pdf')
   doc=fitz.open(out/'reference.pdf');result.update(kind='office_render',renderer='LibreOffice PDF export, first six print pages/slides; macro execution disabled',pdf_sha256=sha(out/'reference.pdf'),total_pages=None)
  result['rendered_page_count']=min(6,len(doc));texts=[]
  for i in range(min(6,len(doc))):
   page=doc[i];scale=min(1500/page.rect.height,1200/page.rect.width,2)
   pix=page.get_pixmap(matrix=fitz.Matrix(scale,scale),alpha=False);dest=out/f'page-{i+1:02}.png';pix.save(dest)
   texts.append(page.get_text());result['images'].append({'path':str(dest.relative_to(ROOT)),'sha256':sha(dest),'width':pix.width,'height':pix.height,'page':i+1})
  (out/'reference-text.txt').write_text('\n\n'.join(texts));doc.close();result['ok']=bool(result['images'])
 write(out/'reference.json',result)
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--one');a=ap.parse_args();corpus=json.loads((REPORT/'corpus-manifest.json').read_text())
 if a.one:one(next(r for r in corpus if r['id']==a.one));return
 results=[]
 for i,r in enumerate(corpus):
  dest=REPORT/'references'/r['id']/'reference.json'
  if dest.exists():result=json.loads(dest.read_text())
  else:
   log=REPORT/'references'/r['id']/'render.log';log.parent.mkdir(parents=True,exist_ok=True)
   with log.open('w') as f:
    p=subprocess.Popen([sys.executable,__file__,'--one',r['id']],stdout=f,stderr=f,start_new_session=True)
    try:p.wait(timeout=120)
    except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait()
   if dest.exists():result=json.loads(dest.read_text())
   else:result={'id':r['id'],'format':r['format'],'ok':False,'error':'Reference rendering failed or exceeded 120 seconds; see render.log','images':[]};write(dest,result)
  results.append(result);write(REPORT/'reference-manifest.json',results)
  print(f'{i+1}/{len(corpus)} {r["id"]}: {"ready" if result["ok"] else "FAILED"} ({len(result["images"])} pages)',flush=True)
 import fitz
 write(REPORT/'reference-environment.json',{'pymupdf':fitz.VersionBind,'libreoffice':subprocess.check_output(['soffice','--version'],text=True).strip(),'script_sha256':sha(Path(__file__)),'scope':'First six print pages/slides; reflowed first six EPUB pages; first 60 CSV records. References are renderer interpretations, not human-transcribed ground truth.'})
if __name__=='__main__':main()
