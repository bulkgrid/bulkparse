#!/usr/bin/env python3
"""Check every changed legacy output and all emitted HTML table rectangles."""
import argparse,difflib,json,re
from pathlib import Path
from audit_table_layout_outputs import parse
ROOT=Path(__file__).resolve().parents[1]
def content(text):
 text=re.sub(r'\\([\\`*_{}\[\]<>#!|~+\-=.])',r'\1',text)
 return ''.join(re.findall(r'\w',''.join(parse(text).text)))
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--run',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
 base=ROOT/'reports/multiformat-tables-layout-2026-11-12/benchmark-final'
 old={r['path']:r for r in json.loads((base/'bulkparse-pass-1.json').read_text())['documents']};new=json.loads((a.run/'bulkparse-pass-1.json').read_text())['documents']
 notes={
 'anydoc-main/tests/fixtures/doc/text.doc':'Character-style NotBold override clears inherited bold; physical table edges/merge flags now produce one structured multirow table.',
 'docling-main/tests/data/doc/sources/greek_heading_styles.doc':'Ignoring non-character table-style payloads restores legitimate paragraph heading properties.',
 'docling-main/tests/data/doc/sources/legacy_sample.doc':'Recovered style metadata restores headings, italic caption, quote and bold table cells; table rows retain declared header/boundary information.',
 'anydoc-main/tests/fixtures/ppt/handmade-multimaster.ppt':'Resolves each slide to its active master, preserving distinct bold/bullet and italic/unbulleted defaults.',
 'anydoc-main/tests/fixtures/ppt/pres.ppt':'Paragraph style depths and bullet flags recover nested bullets.',
 'docling-main/tests/data/ppt/sources/legacy_sample.ppt':'Local text-run emphasis and bullet structure recovered; original table cells still flatten to paragraphs.',
 'anydoc-main/tests/fixtures/rtf/handmade-cocoa.rtf':'Escaped physical line endings create paragraphs; raw newlines stay insignificant.',
 'anydoc-main/tests/fixtures/rtf/text.rtf':'Label-local pard/plain controls no longer replace the parent heading role.'}
 reviews=[];tables=[];patch=[]
 for n in new:
  o=old[n['path']];p=parse(n['markdown']);tables.extend({'path':n['path'],**t} for t in p.tables)
  if o['markdown']!=n['markdown']:
   reviews.append({'path':n['path'],'review':notes.get(n['path'],'REVIEW REQUIRED'),'alphanumeric_content_order_unchanged':content(o['markdown'])==content(n['markdown']),'before_bytes':len(o['markdown'].encode()),'after_bytes':len(n['markdown'].encode())})
   patch.extend(difflib.unified_diff(o['markdown'].splitlines(True),n['markdown'].splitlines(True),fromfile='before/'+n['path'],tofile='after/'+n['path']))
 result={'scope':'Every changed prediction reviewed; normalized alphanumeric content/order excludes markup, punctuation and whitespace. HTML checks validate rectangular occupancy, not semantic cell correctness.','changed_outputs':reviews,'html_tables':tables}
 a.out.write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n');a.out.with_suffix('.diff').write_text(''.join(patch))
 assert all(r['alphanumeric_content_order_unchanged'] and r['review']!='REVIEW REQUIRED' for r in reviews)
 assert not any(t['problems'] for t in tables)
 print(json.dumps({'changed':len(reviews),'alphanumeric_content_order_unchanged':True,'html_tables':len(tables),'geometry_problems':0}))
if __name__=='__main__':main()
