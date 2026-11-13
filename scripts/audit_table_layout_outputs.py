#!/usr/bin/env python3
"""Review all changed predictions and validate emitted HTML table rectangles."""
import argparse,collections,hashlib,json,re
from html.parser import HTMLParser
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class Tables(HTMLParser):
 def __init__(self):super().__init__();self.stack=[];self.tables=[];self.text=[]
 def handle_data(self,data):self.text.append(data)
 def handle_starttag(self,tag,attrs):
  a=dict(attrs)
  if tag=='table':self.stack.append({'row':-1,'grid':{},'problems':[]})
  elif self.stack:
   t=self.stack[-1]
   if tag=='tr':t['row']+=1;t['col']=0
   elif tag in ('td','th'):
    r=t['row'];c=t.get('col',0)
    while (r,c) in t['grid']:c+=1
    rs=int(a.get('rowspan',1));cs=int(a.get('colspan',1))
    if not 1<=rs<=100000 or not 1<=cs<=100000 or rs*cs>100000:raise ValueError('Unbounded table span')
    for rr in range(r,r+rs):
     for cc in range(c,c+cs):
      if (rr,cc) in t['grid']:t['problems'].append('overlapping cells')
      t['grid'][rr,cc]=True
    t['col']=c+cs
 def handle_endtag(self,tag):
  if tag=='table':
   t=self.stack.pop();rows=t['row']+1;widths=[max((c+1 for rr,c in t['grid'] if rr==r),default=0) for r in range(rows)]
   if len(set(widths))>1:t['problems'].append('inconsistent row extents')
   if any(r>=rows for r,c in t['grid']):t['problems'].append('span exceeds declared rows')
   t={'rows':rows,'row_widths':widths,'problems':sorted(set(t['problems']))};self.tables.append(t)
def parse(text):
 p=Tables();p.feed(text);assert not p.stack
 return p
def tokens(text):
 text=re.sub(r'\\([\\`*_{}\[\]<>#!|~+\-=.])',r'\1',text)
 return collections.Counter(re.findall(r'\w+', ' '.join(parse(text).text)))
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--run',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
 old=json.loads((ROOT/'reports/multiformat-legacy-quality-2026-11-12/benchmark-final/bulkparse-pass-1.json').read_text())['documents'];new=json.loads((a.run/'bulkparse-pass-1.json').read_text())['documents'];old={r['path']:r for r in old}
 notes={
 'handmade-bin.rtf':'Declared cell edges now determine shared columns; valid merges no longer produce a blanket unsupported-merge diagnostic.',
 'handmade-merge.rtf':'Unequal row boundaries form a shared grid; valid vertical chains preserved; orphan continuation retained and diagnosed.',
 'text.rtf':'Physical table boundaries determine alignment instead of cell ordinal.',
 'handmade-sheet.xlsb':'Covered-cell TRUE/error content stays inside the merged anchor with original block coordinates. This fixes overlapping geometry; ambiguous covered content remains diagnosed.',
 'odf_presentation_01.odp':'Recovers list-backed table values and orders positioned title/table/footnote.',
 'text_document_03.odt':'Recovers rich cell lists, list-only cells, nested tables and inline formatting; explicit breaks retained.',
 'powerpoint_sample.pptx':'Moves table before lower footnote and preserves explicitly marked header semantics.',
 'powerpoint_with_image.pptx':'Moves caption before linked image description using declared vertical position.'}
 changed=[];geometry={}
 for r in new:
  o=old[r['path']];p=parse(r['markdown']);issues=[t for t in p.tables if t['problems']]
  geometry[r['path']]={'tables':len(p.tables),'problems':issues}
  if o['markdown']!=r['markdown']:
   missing=dict(tokens(o['markdown'])-tokens(r['markdown']))
   changed.append({'path':r['path'],'review':notes.get(Path(r['path']).name,'REVIEW REQUIRED'),'before_bytes':len(o['markdown'].encode()),'after_bytes':len(r['markdown'].encode()),'missing_token_counts':missing,'before_table_issues':[t for t in parse(o['markdown']).tables if t['problems']],'after_table_issues':issues})
 result={'changed_outputs':changed,'html_geometry':geometry,'scope':'Checks rectangular HTML table occupancy and token retention, not semantic correctness of every cell or visual fidelity.'}
 a.out.write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n')
 assert all(not r['after_table_issues'] and r['review']!='REVIEW REQUIRED' for r in changed)
 assert not any(r['problems'] for r in geometry.values())
 assert not any(r['missing_token_counts'] for r in changed)
 print(json.dumps({'changed':len(changed),'missing_tokens':{r['path']:r['missing_token_counts'] for r in changed if r['missing_token_counts']},'html_tables':sum(r['tables'] for r in geometry.values()),'geometry_issues':0},indent=2))
if __name__=='__main__':main()
