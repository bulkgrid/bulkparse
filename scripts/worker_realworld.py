#!/usr/bin/env python3
"""Persistent, benchmark-only Office adapters; no document-specific repairs."""
import argparse,importlib,importlib.metadata,io,json,logging,os,resource,shutil,subprocess,sys,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]

def cpu_seconds():
 return sum(u.ru_utime+u.ru_stime for u in (resource.getrusage(resource.RUSAGE_SELF),resource.getrusage(resource.RUSAGE_CHILDREN)))

def main():
 ap=argparse.ArgumentParser();ap.add_argument('engine');e=ap.parse_args().engine
 protocol=sys.stdout;sys.stdout=sys.stderr;logging.basicConfig(level=logging.ERROR)
 options={};versions={}
 pandoc=ROOT/'target/competitors/broad-venv/lib/python3.12/site-packages/pypandoc/files/pandoc'
 os.environ['PYPANDOC_PANDOC']=str(pandoc);os.environ['PATH']=str(pandoc.parent)+os.pathsep+os.environ['PATH']
 if e=='markitdown':
  from markitdown import MarkItDown
  parser=MarkItDown(enable_plugins=False);convert=lambda p:parser.convert(str(p)).text_content
  options={'plugins':False,'llm_client':None};versions['markitdown']=importlib.metadata.version('markitdown')
 elif e=='pandoc':
  def convert(p):return subprocess.check_output([str(pandoc),'-f',p.suffix[1:],'-t','gfm','--wrap=none',str(p)],stderr=subprocess.PIPE,text=True,timeout=90)
  versions['pandoc']=subprocess.check_output([str(pandoc),'--version'],text=True).splitlines()[0];options={'format':'gfm','wrap':'none','CLI_launch':'included'}
 elif e=='mammoth':
  import mammoth
  from markdownify import markdownify
  def convert(p):
   with p.open('rb') as f:result=mammoth.convert_to_html(f)
   return markdownify(result.value,heading_style='ATX')
  versions={n:importlib.metadata.version(n) for n in ['mammoth','markdownify']};options={'pipeline':'Mammoth default HTML -> markdownify ATX Markdown'}
 elif e=='libreoffice':
  soffice=shutil.which('soffice');assert soffice
  def convert(p):
   with tempfile.TemporaryDirectory(prefix='realworld-lo-') as td:
    tmp=Path(td);profile=tmp/'profile';(profile/'user').mkdir(parents=True)
    (profile/'user/registrymodifications.xcu').write_text('<oor:items xmlns:oor="http://openoffice.org/2001/registry"><item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop></item></oor:items>')
    subprocess.run([soffice,'-env:UserInstallation='+profile.as_uri(),'--headless','--norestore','--nologo','--nodefault','--convert-to','html','--outdir',td,str(p)],capture_output=True,check=True,timeout=90)
    html=tmp/(p.stem+'.html')
    if not html.exists():raise RuntimeError('LibreOffice did not produce HTML')
    return subprocess.check_output([str(pandoc),'-f','html','-t','gfm','--wrap=none',str(html)],stderr=subprocess.PIPE,text=True,timeout=90)
  versions={'libreoffice':subprocess.check_output([soffice,'--version'],text=True).strip(),'pandoc':subprocess.check_output([str(pandoc),'--version'],text=True).splitlines()[0]};options={'pipeline':'headless HTML export -> Pandoc GFM','subprocess_startup':'included','macro_execution':'MacroSecurityLevel 3 in isolated profile'}
 elif e=='unstructured':
  from markdownify import markdownify
  modules={'doc':('doc','partition_doc'),'docx':('docx','partition_docx'),'docm':('docx','partition_docx'),'ppt':('ppt','partition_ppt'),'pptx':('pptx','partition_pptx'),'xlsx':('xlsx','partition_xlsx'),'xlsm':('xlsx','partition_xlsx'),'xls':('xlsx','partition_xlsx'),'odt':('odt','partition_odt'),'rtf':('rtf','partition_rtf'),'epub':('epub','partition_epub'),'csv':('csv','partition_csv')}
  funcs={ext:getattr(importlib.import_module('unstructured.partition.'+mod),fn) for ext,(mod,fn) in modules.items()}
  def convert(p):
   parts=[]
   for el in funcs[p.suffix[1:]](filename=str(p)):
    text=(el.text or '').strip();category=el.category
    if category=='Table' and getattr(el.metadata,'text_as_html',None):parts.append(markdownify(el.metadata.text_as_html).strip())
    elif text:parts.append(('## ' if category=='Title' else '- ' if category=='ListItem' else '')+text)
   return '\n\n'.join(parts)
  versions={n:importlib.metadata.version(n) for n in ['unstructured','markdownify']};options={'dispatch':'declared per-format partition functions','tables':'metadata.text_as_html -> markdownify','titles':'ATX level 2','list_items':'Markdown bullets','remote_services':False}
 elif e=='docling':
  from docling.datamodel.base_models import InputFormat,DocumentStream
  from docling.document_converter import DocumentConverter
  formats=[InputFormat(x) for x in ['docx','doc','rtf','pptx','ppt','xlsx','xls','odt','ods','odp','epub','html','csv']]
  parser=DocumentConverter(allowed_formats=formats)
  def convert(p):
   rs=list(parser.convert_all([DocumentStream(name=p.name,stream=io.BytesIO(p.read_bytes()))],raises_on_error=False))
   good=[r for r in rs if r.status.value in ('success','partial_success')]
   if not good:raise RuntimeError('; '.join(str(x) for r in rs for x in r.errors) or 'No successful subdocuments')
   return ('\n\n'.join(r.document.export_to_markdown() for r in good), [{'status':r.status.value,'errors':[str(x) for x in r.errors]} for r in rs], 'partial_success' if len(good)<len(rs) or any(r.status.value=='partial_success' for r in rs) else 'returned')
  versions['docling']=importlib.metadata.version('docling');options={'formats':[f.value for f in formats],'API':'convert_all; successful default Markdown exports joined','failed_subdocuments':'status and error details retained in raw per-document records; usable outputs joined','backend_options':'installed defaults'}
 else:raise ValueError(e)
 print(json.dumps({'ready':True,'engine':e,'versions':versions,'options':options}),file=protocol,flush=True)
 for line in sys.stdin:
  q=json.loads(line);rows=[]
  for name in q['documents']:
   p=Path(name);cpu_start=cpu_seconds();start=time.perf_counter_ns()
   try:
    result=convert(p);elapsed=time.perf_counter_ns()-start;cpu_elapsed=cpu_seconds()-cpu_start
    md,diagnostics,status=result if isinstance(result,tuple) else (result,[],'returned')
    row=dict(path=name,elapsed_ns=elapsed,cpu_seconds=cpu_elapsed,ok=True,status=status,diagnostics=diagnostics,markdown=md,error=None)
   except Exception as exc:row=dict(path=name,elapsed_ns=time.perf_counter_ns()-start,cpu_seconds=cpu_seconds()-cpu_start,ok=False,status='error',markdown='',error=type(exc).__name__+': '+str(exc))
   rows.append(row)
  print(json.dumps({'documents':rows}),file=protocol,flush=True)
if __name__=='__main__':main()
