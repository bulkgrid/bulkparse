#!/usr/bin/env python3
"""Use all declared Office/EPUB backends in the installed Docling version."""
import io,json,logging,os,sys,time
from pathlib import Path
os.environ['HF_HUB_OFFLINE']='1';os.environ['TRANSFORMERS_OFFLINE']='1'
logging.basicConfig(level=logging.ERROR)
from docling.datamodel.base_models import InputFormat,DocumentStream
from docling.document_converter import DocumentConverter
formats=[InputFormat(x) for x in ['docx','doc','rtf','pptx','ppt','xlsx','xls','odt','ods','odp','epub','html']]
converter=DocumentConverter(allowed_formats=formats)
print(json.dumps({'ready':True,'formats':[f.value for f in formats], 'conversion':'convert_all; default Markdown exports joined with blank lines', 'legacy_conversion':'Docling default LibreOffice subprocess, included in per-document timing'}),flush=True)
for line in sys.stdin:
    q=json.loads(line); rows=[]; batch=time.perf_counter_ns()
    for filename in q['documents']:
        path=Path(filename); start=time.perf_counter_ns()
        try:
            data=path.read_bytes();results=list(converter.convert_all([DocumentStream(name=path.name,stream=io.BytesIO(data))],raises_on_error=False))
            returned=[r for r in results if r.status.value in ('success','partial_success')]
            ok=bool(returned)
            status='partial_success' if ok and any(r.status.value!='success' for r in results) else ('success' if ok else 'failure')
            md='\n\n'.join(r.document.export_to_markdown() for r in returned)
            row=dict(path=filename,elapsed_ns=time.perf_counter_ns()-start,ok=ok,status=status,markdown=md,error=None if ok else status,diagnostics=[str(x) for r in results for x in r.errors],subdocuments=len(results))
        except Exception as exc:row=dict(path=filename,elapsed_ns=time.perf_counter_ns()-start,ok=False,error=f'{type(exc).__name__}: {exc}',markdown='')
        rows.append(row)
    print(json.dumps(dict(elapsed_ns=time.perf_counter_ns()-batch,documents=rows)),flush=True)
