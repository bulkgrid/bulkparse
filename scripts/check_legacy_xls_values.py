#!/usr/bin/env python3
"""Independent value check against xlrd for a supplied recovered workbook (not a parser rule)."""
import argparse,collections,datetime,decimal,json,re
from pathlib import Path
import xlrd

def inline(items):
 return ''.join(x.get('text','') if x['type'] in ('text','code') else '\n' if x['type'] in ('line_break','soft_break','hard_break') else inline(x.get('children',x.get('inlines',[]))) for x in items)
def cell_text(cell):return '\n'.join(inline(b.get('inlines',[])) for b in cell['blocks'])
def normal(s):return s.replace('\r\n','\n').replace('\r','\n').strip()
def main():
 ap=argparse.ArgumentParser();ap.add_argument('input',type=Path);ap.add_argument('json_output',type=Path);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
 book=xlrd.open_workbook(a.input,formatting_info=True);parsed=json.loads(a.json_output.read_text());actual={};sheets=[]
 for block in parsed['document']['blocks']:
  if block['type']!='table':continue
  name=block['source']['part'].removeprefix('Workbook/');sheets.append(name)
  for row in block['rows']:
   for cell in row:
    if cell['blocks']:
     key=(name,cell['source']['row']-1,cell['source']['cell']-1);assert key not in actual;actual[key]=cell_text(cell)
 checked=collections.Counter();failures=[];expected=set();formats=collections.Counter()
 for sheet in book.sheets():
  if sheet.visibility:continue
  assert sheet.name in sheets
  for row in range(sheet.nrows):
   for col in range(sheet.ncols):
    c=sheet.cell(row,col)
    if c.ctype in (xlrd.XL_CELL_EMPTY,xlrd.XL_CELL_BLANK) or c.value=='':continue
    key=(sheet.name,row,col);expected.add(key);value=actual.get(key);ok=False
    fmt=book.format_map[book.xf_list[sheet.cell_xf_index(row,col)].format_key].format_str;formats[fmt]+=1
    if value is not None:
     if c.ctype==xlrd.XL_CELL_TEXT:ok=normal(value)==normal(c.value)
     elif c.ctype==xlrd.XL_CELL_DATE:ok=value==xlrd.xldate_as_datetime(c.value,book.datemode).date().isoformat()
     elif c.ctype==xlrd.XL_CELL_BOOLEAN:ok=value==('TRUE' if c.value else 'FALSE')
     elif c.ctype==xlrd.XL_CELL_NUMBER:
      cleaned=value.replace(',','').replace('£','').replace('$','').replace('€','').strip()
      if cleaned.startswith('(') and cleaned.endswith(')'):cleaned='-'+cleaned[1:-1]
      if cleaned=='-' and c.value==0:cleaned='0'
      try:ok=decimal.Decimal(cleaned)==decimal.Decimal(str(c.value))
      except decimal.InvalidOperation:pass
    checked[str(c.ctype)]+=1
    if not ok:failures.append(dict(sheet=sheet.name,row=row+1,column=col+1,kind=c.ctype,source=c.value,output=value,format=fmt))
 extras=sorted(set(actual)-expected)
 result=dict(input=str(a.input),reference=f'xlrd {xlrd.__version__}',biff_version=book.biff_version,codepage=book.codepage,visible_sheets=sheets,nonempty_source_cells=len(expected),checked_by_type=dict(checked),number_formats=dict(formats),mismatches=failures,extra_output_cells=extras,scope='Visible sheets, original coordinates; text normalizes outer whitespace and CRLF only. Dates checked as ISO dates; numeric values checked exactly after removing currency/grouping notation. No visual-layout or image quality claim.')
 a.out.write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='number_formats'},ensure_ascii=False))
 assert not failures and not extras
if __name__=='__main__':main()
