#!/usr/bin/env python3
"""Deterministic scanned-layout development check; annotations never enter parsers."""
import argparse, collections, hashlib, json, re, sys, time, unicodedata
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'target/scan-parity'
def tokens(text): return re.findall(r'[^\W_]+',unicodedata.normalize('NFKC',text).casefold())
def text_values(value):
 if isinstance(value,dict):
  if isinstance(value.get('text'),str):yield value['text']
  for k,v in value.items():
   if k not in ('text','source'):yield from text_values(v)
 elif isinstance(value,list):
  for v in value:yield from text_values(v)
def prepare():
 import pypdfium2 as pdfium
 OUT.mkdir(exist_ok=True);(OUT/'inputs').mkdir(exist_ok=True)
 manifest=json.loads((ROOT/'benchmarks/icdar2013-tables/manifest.json').read_text())['rows']
 references=json.loads((ROOT/'benchmarks/icdar2013-tables/references.json').read_text())
 candidates=[]
 for row in manifest:
  if row['excluded']:continue
  for page in sorted({r['page'] for r in references[row['id']]}):
   refs=[r for r in references[row['id']] if r['page']==page]
   candidates.append({'id':row['id']+'-'+str(page),'source':row['pdf'],'page':page,'tables':len(refs),'reference':' '.join(c['text'] for r in refs for c in r['cells']),'reference_scope':'table cells','kind':'ICDAR'})
 selected=sorted(candidates,key=lambda r:hashlib.sha256(('scan-parity-v1:'+r['id']).encode()).hexdigest())[:8]
 refs=json.loads((ROOT/'target/independent-pdf-v2/references.json').read_text())
 names=sorted(refs,key=lambda x:hashlib.sha256(('scan-parity-v1:'+x).encode()).hexdigest())[:6]
 for name in names:
  selected.append({'id':name,'source':f'target/independent-pdf-v2/pdfs/{name}.pdf','page':1,'tables':None,'reference':refs[name]['text'],'reference_scope':'page cells','kind':'DocLayNet'})
 for row in selected:
  path=ROOT/row['source']; doc=pdfium.PdfDocument(path);page=doc[row['page']-1]
  bitmap=page.render(scale=200/72,rev_byteorder=True)
  bitmap.to_pil().convert('RGB').save(OUT/'inputs'/(row['id']+'.png'))
  bitmap.close();page.close();doc.close()
  row['source_sha256']=hashlib.sha256(path.read_bytes()).hexdigest()
  row['scan_sha256']=hashlib.sha256((OUT/'inputs'/(row['id']+'.png')).read_bytes()).hexdigest()
 (OUT/'manifest.json').write_text(json.dumps(selected,indent=2));print('prepared',len(selected))
def main():
 parser=argparse.ArgumentParser();parser.add_argument('engine',choices=['prepare','bulkparse','docling']);parser.add_argument('--label');a=parser.parse_args()
 if a.engine=='prepare':return prepare()
 label=a.label or a.engine;dest=OUT/label;dest.mkdir(parents=True,exist_ok=True)
 rows=json.loads((OUT/'manifest.json').read_text());results=[]
 if a.engine=='bulkparse':
  sys.path.insert(0,str(ROOT/'bindings/python/python'));from bulkparse import OcrParser
  engine=OcrParser(json.loads((ROOT/'target/ocr/profiles/multi-small/config.json').read_text()))
 else:
  sys.path.append(str(ROOT/'target/ocr/venv/lib/python3.12/site-packages'))
  import torch;torch.set_num_threads(1)
  from docling.document_converter import DocumentConverter,ImageFormatOption
  from docling.datamodel.base_models import InputFormat
  from docling.datamodel.pipeline_options import PdfPipelineOptions,RapidOcrOptions
  from rapidocr import OCRVersion,LangRec,ModelType
  from rapidocr.utils.download_file import DownloadFile
  def no_download(*args,**kwargs):raise RuntimeError('downloads disabled')
  DownloadFile.run=staticmethod(no_download)
  models=json.loads((ROOT/'target/ocr/profiles/multi-small/models.json').read_text())['models']
  opts=PdfPipelineOptions();opts.do_ocr=True;opts.do_table_structure=True;opts.table_structure_options.do_cell_matching=True;opts.document_timeout=120
  opts.ocr_options=RapidOcrOptions(force_full_page_ocr=False,backend='onnxruntime',det_model_path=models['det']['path'],cls_model_path=models['cls']['path'],rec_model_path=models['rec']['path'],rapidocr_params={'Rec.ocr_version':OCRVersion('PP-OCRv6'),'Rec.lang_type':LangRec('ch'),'Rec.model_type':ModelType('small')})
  engine=DocumentConverter(format_options={InputFormat.IMAGE:ImageFormatOption(pipeline_options=opts)})
 for row in rows:
  start=time.perf_counter();out={'id':row['id'],'kind':row['kind'],'expected_tables':row['tables']}
  try:
   if a.engine=='bulkparse':
    result=engine.parse_file(OUT/'inputs'/(row['id']+'.png'),budget={'max_pages':2000,'max_output_bytes':33554432})
    text=' '.join(text_values(result['document']['blocks']));markdown=result['markdown'];tables=sum(b['type']=='table' for b in result['document']['blocks'])
   else:
    document=engine.convert(OUT/'inputs'/(row['id']+'.png')).document
    result=document.export_to_dict();markdown=document.export_to_markdown();text=markdown;tables=len(document.tables)
   gold=collections.Counter(tokens(row['reference']));pred=collections.Counter(tokens(text));recall=sum((gold&pred).values())/max(1,sum(gold.values()))
   (dest/(row['id']+'.json')).write_text(json.dumps(result,ensure_ascii=False));(dest/(row['id']+'.md')).write_text(markdown)
   out.update({'nonempty':bool(markdown.strip()),'tables':tables,'reference_token_recall':recall,'seconds':time.perf_counter()-start})
  except Exception as error:out['error']=str(error)[:1000]
  results.append(out);print(json.dumps(out),flush=True);(dest/'summary.json').write_text(json.dumps(results,indent=2))
 if a.engine=='bulkparse':engine.close()
if __name__=='__main__':main()
