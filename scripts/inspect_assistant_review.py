#!/usr/bin/env python3
"""Read only anonymous outputs and source evidence. Inspection aids are not scores."""
import argparse,csv,html,json,re,unicodedata,string
from html.parser import HTMLParser
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];BASE=ROOT/'target/realworld/assistant-review/packets'
def norm(s):return ' '.join(unicodedata.normalize('NFKC',s).split())
def clean(s):
 s=re.sub(r'<br\s*/?>',' ',s,flags=re.I);s=re.sub(r'</?[A-Za-z][A-Za-z0-9]*(?:\s[^>]*|/?)>','',s);s=re.sub(r'\\(.)',lambda m:m[1] if m[1] in string.punctuation else m[0],s)
 return norm(html.unescape(s))
class TableParser(HTMLParser):
 def __init__(self):super().__init__();self.rows=[];self.row=None;self.cell=None;self.data=[]
 def handle_starttag(self,tag,attrs):
  if tag=='tr':self.row=[]
  if tag=='br' and self.cell:self.data.append(' ')
  if tag in ['td','th']:self.cell=tag;self.data=[]
 def handle_data(self,data):
  if self.cell:self.data.append(data)
 def handle_endtag(self,tag):
  if tag in ['td','th'] and self.cell:
   if self.row is not None:self.row.append(norm(''.join(self.data)))
   self.cell=None
  if tag=='tr' and self.row is not None:self.rows.append(self.row);self.row=None
def table_rows(path,limit=65):
 lines=[]
 with path.open() as f:
  for line in f:
   if line.strip().startswith('|'):
    cells=[clean(x) for x in re.split(r'(?<!\\)\|',line.strip().strip('|'))]
    if all(re.fullmatch(r':?-+:?',x.replace(' ','')) for x in cells):continue
    lines.append(cells)
    if len(lines)>=limit:return lines
 if lines:return lines
 parser=TableParser()
 with path.open() as f:
  while chunk:=f.read(32768):
   parser.feed(chunk)
   if len(parser.rows)>=limit:break
 return parser.rows[:limit]
def main():
 ap=argparse.ArgumentParser();ap.add_argument('document');ap.add_argument('--mode',choices=['overview','candidate','csv'],default='overview');ap.add_argument('--label');ap.add_argument('--start',type=int,default=0);ap.add_argument('--chars',type=int,default=14000);a=ap.parse_args();packet=json.loads((BASE/a.document/'packet.json').read_text());ref=packet['reference']
 if a.mode=='candidate':
  p=BASE/a.document/(a.label+'.md');text=p.read_text();text=re.sub(r'data:image/[A-Za-z0-9.+-]+;base64,[A-Za-z0-9+/=]+','[embedded image data omitted]',text);print(text[a.start:a.start+a.chars]);print('\n[normalized output characters:',len(text),'; displayed offset:',a.start,']');return
 if a.mode=='csv':
  truth=json.loads((ROOT/ref['text_path']).read_text());expected=[[norm(c) for c in row] for row in truth['records']];print(json.dumps({'source':{**truth,'records':truth['records'][:6]},'reviewed_record_count':len(expected)},ensure_ascii=False))
  for o in packet['outputs']:
   p=ROOT/o['path'];rows=table_rows(p);diff=[]
   def trimmed(row):
    row=list(row)
    while row and not row[-1]:row.pop()
    return row
   alignment={str(offset):sum(trimmed(row)==trimmed(rows[i+offset]) for i,row in enumerate(expected) if i+offset<len(rows)) for offset in range(3)}
   for i,row in enumerate(expected):
    actual=rows[i] if i<len(rows) else []
    if row!=actual:diff.append({'record':i+1,'expected':row,'observed':actual})
   print(json.dumps({'label':o['label'],'bytes':o['bytes'],'parsed_rows':len(rows),'expected_records':len(expected),'exact_prefix_rows':len(expected)-len(diff),'differences':diff[:5] if o['bytes'] else [],'difference_count':len(diff),'matching_rows_after_offset_and_empty_tail_removal':alignment,'preview':p.read_text()[:1000]},ensure_ascii=False))
  return
 print(json.dumps({'document':packet['id'],'format':packet['format'],'title':packet['source_title'],'reference_images':ref['images']},ensure_ascii=False))
 text_path=ROOT/ref['text_path'] if ref.get('text_path') else ROOT/'reports/realworld-2026-11-12/references'/a.document/'reference-text.txt'
 source=text_path.read_text();print('\nSOURCE TEXT (reference rendering, verify against page images):\n'+source[:24000])
 for o in packet['outputs']:
  text=(ROOT/o['path']).read_text();print(json.dumps({'label':o['label'],'bytes':o['bytes'],'heading_lines':re.findall(r'^#{1,6} .+',text,re.M)[:25],'table_row_lines':sum(line.lstrip().startswith('|') for line in text.splitlines()),'html_tables':text.lower().count('<table'),'preview':re.sub(r'data:image/[A-Za-z0-9.+-]+;base64,[A-Za-z0-9+/=]+','[embedded image data omitted]',text[:5000])},ensure_ascii=False))
if __name__=='__main__':main()
