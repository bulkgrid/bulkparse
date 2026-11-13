#!/usr/bin/env python3
"""Source-cell checks for partial recovery, plus unchanged Office fixtures.
Run with target/competitors/docling-venv/bin/python. No quality score is inferred.
"""
import argparse,csv,hashlib,json,os,re,sys,zipfile,xml.etree.ElementTree as ET
from pathlib import Path
from bs4 import BeautifulSoup
from markdown_it import MarkdownIt
from benchmark_realworld import ROOT,Worker,digest,write
OUT=ROOT/'reports/large-documents-2026-11-12'
RUN=OUT/'benchmark-final'
def tag(e):return e.tag.rsplit('}',1)[-1]
def attrs(e):return {k.rsplit('}',1)[-1]:v for k,v in e.attrib.items()}
def norm(s):return ' '.join(s.split())
def tables(doc):
 soup=BeautifulSoup(MarkdownIt('commonmark').enable('table').render((RUN/'predictions/bounded'/(doc+'.md')).read_text()),'html.parser');result={}
 for table in soup.find_all('table'):
  heading=table.find_previous(['h1','h2']);part=heading.get_text() if heading else ''
  result[part]=[(int(c[0].get_text()),int(c[1].get_text()),norm(c[2].get_text(separator=' '))) for tr in table.select('tbody tr') if len(c:=tr.find_all('td',recursive=False))==3]
 return result

def check_xlsx(path,wanted):
 issues=[];checked=0
 with zipfile.ZipFile(path) as z:
  strings=[]
  # Corpus relationship paths are recorded; resolve standard part for reference.
  sp='xl/sharedStrings.xml'
  if sp in z.namelist():
   with z.open(sp) as f:
    for _,e in ET.iterparse(f,events=('end',)):
     if tag(e)=='si':
      strings.append(''.join(''.join(n.itertext()) if tag(n)=='t' else ''.join(''.join(t.itertext()) for t in n if tag(t)=='t') if tag(n)=='r' else '' for n in e));e.clear()
  for part,values in wanted.items():
   pending={(r,c):v for r,c,v in values};lastrow=max(r for r,c in pending)
   with z.open(part) as f:
    for _,e in ET.iterparse(f,events=('end',)):
     if tag(e)!='c':continue
     address=e.get('r','');m=re.fullmatch(r'([A-Z]+)(\d+)',address)
     if not m:continue
     col=0
     for c in m[1]:col=col*26+ord(c)-64
     key=(int(m[2]),col)
     if key in pending:
      v=next((''.join(n.itertext()) for n in e if tag(n)=='v'),'');kind=e.get('t','')
      if kind=='s':v=strings[int(v)]
      elif kind=='b':v={'0':'FALSE','1':'TRUE'}[v]
      elif kind=='inlineStr':v=''.join(''.join(n.itertext()) if tag(n)=='t' else ''.join(''.join(t.itertext()) for t in n if tag(t)=='t') if tag(n)=='r' else '' for parent in e if tag(parent)=='is' for n in parent)
      if not v:
       formula=next((''.join(n.itertext()) for n in e if tag(n)=='f'),'')
       if formula:v='='+formula
      actual=pending.pop(key);checked+=1
      if actual!=norm(v):issues.append({'part':part,'row':key[0],'column':key[1],'expected':norm(v)[:160],'actual':actual[:160]})
     e.clear()
     if not pending:break
   issues.extend({'part':part,'missing_source_coordinate':list(k)} for k in pending)
 return checked,issues

def odf_text(e):
 out=e.text or ''
 for c in e:
  if tag(c)=='s':out+=' '*int(attrs(c).get('c','1'))
  elif tag(c)=='tab':out+='\t'
  elif tag(c)=='line-break':out+='\n'
  else:out+=odf_text(c)
  out+=c.tail or ''
 return out

def check_ods(path,wanted):
 pending={p:{(r,c):v for r,c,v in rows} for p,rows in wanted.items()};issues=[];checked=0
 stack=[];sheet=None;sheetdepth=None;rowdepth=None;row=1
 with zipfile.ZipFile(path) as z,z.open('content.xml') as f:
  for event,e in ET.iterparse(f,events=('start','end')):
   n=tag(e)
   if event=='start':
    if n=='table' and stack and stack[-1]=='spreadsheet':sheet='content.xml/'+attrs(e).get('name','Sheet');sheetdepth=len(stack)+1;row=1
    if n=='table-row' and sheetdepth is not None and rowdepth is None:rowdepth=len(stack)+1
    stack.append(n);continue
   depth=len(stack)
   if rowdepth==depth:
    repeat=int(attrs(e).get('number-rows-repeated','1'));col=1;values=[]
    for c in e:
     if tag(c) not in ('table-cell','covered-table-cell'):continue
     a=attrs(c);copies=int(a.get('number-columns-repeated','1'));text='\n'.join(odf_text(p) for p in c.iter() if tag(p)=='p')
     if not text:text=next((a[k] for k in ['string-value','date-value','time-value','boolean-value','value','formula'] if k in a),'')
     values.append((col,copies,norm(text)));col+=copies
    targets=pending.get(sheet,{})
    keys=[k for k in targets if row<=k[0]<row+repeat]
    for key in keys:
     expected=next((v for col,copies,v in values if col<=key[1]<col+copies),None);actual=targets.pop(key);checked+=1
     if expected!=actual:issues.append({'part':sheet,'row':key[0],'column':key[1],'expected':str(expected)[:160],'actual':actual[:160]})
    row+=repeat;rowdepth=None;e.clear()
   elif rowdepth is None:e.clear()
   if n=='table' and depth==sheetdepth:sheet=None;sheetdepth=None
   stack.pop()
   if all(not v for v in pending.values()):break
 issues.extend({'part':part,'missing_source_coordinate':list(k)} for part,values in pending.items() for k in values)
 return checked,issues

def main():
 global OUT,RUN
 ap=argparse.ArgumentParser(description=__doc__)
 ap.add_argument('--out',type=Path,help='Report root containing benchmark-final and before/source-snapshot')
 args=ap.parse_args()
 if args.out:OUT=args.out.resolve();RUN=OUT/'benchmark-final'
 summary=json.loads((RUN/'summary.json').read_text());identity=json.loads((RUN/'identity.json').read_text());binary=ROOT/'target/release/examples/office_bench';assert digest(binary)==identity['binary_sha256']['bounded']
 assert not summary['default_output_changes'] and not summary['previously_successful_outputs_changed'] and not summary['conversion_regressions']
 manifest={r['id']:r for r in json.loads((RUN/'corpus-manifest.json').read_text())};checks=[]
 for doc in summary['recovered_documents']:
  path=ROOT/manifest[doc]['path'];assert digest(path)==manifest[doc]['sha256']
  fmt=manifest[doc]['format']
  if fmt in ('xlsx','xlsm'):count,issues=check_xlsx(path,tables(doc));scope='All retained populated cells; coordinate/value match against source XML and shared strings, normalized whitespace.'
  elif fmt=='ods':count,issues=check_ods(path,tables(doc));scope='All retained populated cells; coordinate/value match against streamed source rows/repeats, normalized whitespace.'
  elif fmt=='csv':
   soup=BeautifulSoup(MarkdownIt('commonmark').enable('table').render((RUN/'predictions/bounded'/(doc+'.md')).read_text()),'html.parser');rows=[[norm(c.get_text(separator=' ')) for c in tr.find_all('td',recursive=False)] for tr in soup.select('tbody tr')]
   source=csv.reader(path.read_text(encoding='cp1252').splitlines());issues=[]
   for i,actual in enumerate(rows):
    expected=[norm(v) for v in next(source)]
    if actual!=expected:issues.append({'record':i+1})
   count=len(rows);scope='All retained complete CSV records, all columns, normalized whitespace; CP1252 source decoding.'
  else:count=0;issues=[];scope='XLS completed-sheet boundary is covered by synthetic BIFF regression tests; no independent per-cell source audit in this run.'
  checks.append({'document_id':doc,'checked':count,'scope':scope,'issues':issues});print(doc,count,'issues',len(issues),flush=True)
 old=json.loads((ROOT/'reports/multiformat-legacy-layout-2026-11-12/benchmark-final/bulkparse-pass-1.json').read_text())['documents'];development={}
 profiles=[('default',[]),('bounded',['--partial-on-limit'])]
 if 'stream' in identity['commands']:
  profiles += [('stream',['--stream-markdown']),('stream_bounded',['--stream-markdown','--partial-on-limit'])]
 for mode,args in profiles:
  w=Worker([str(binary),*args],OUT/(mode+'-development.log'),dict(os.environ,RAYON_NUM_THREADS='1'));results=[]
  try:
   for row in old:
    result=w.call(ROOT/row['path']);results.append({'path':row['path'],'unchanged':(result['ok'],result['markdown'])==(row['ok'],row['markdown'])})
  finally:w.close()
  development[mode]=results
 before=(OUT/'before/source-snapshot/crates/bulkparse/src/lib.rs').read_text();after=(ROOT/'crates/bulkparse/src/lib.rs').read_text()
 limits=lambda text:text[text.index('impl Default for Limits'):text.index('#[derive',text.index('impl Default for Limits'))].strip()
 unchanged=limits(before)==limits(after);assert unchanged
 report={'source_checks':checks,'development':development,'limits_unchanged':unchanged,'limits_verification':'Limits::default implementation compared byte-for-byte against the preserved previous source snapshot.','no_quality_score':True}
 write(OUT/'regression-checks.json',report)
 assert all(not c['issues'] for c in checks)
 assert all(r['unchanged'] for rs in development.values() for r in rs)
 print('All checks passed')
if __name__=='__main__':main()
