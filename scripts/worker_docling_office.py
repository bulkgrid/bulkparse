#!/usr/bin/env python3
"""Persistent local Docling Office converter, with no PDF/OCR/model pipeline."""
import io,json,logging,os,sys,time
from pathlib import Path
os.environ['HF_HUB_OFFLINE']='1';os.environ['TRANSFORMERS_OFFLINE']='1'
logging.basicConfig(level=logging.ERROR)
from docling.datamodel.base_models import InputFormat,DocumentStream
from docling.document_converter import DocumentConverter
converter=DocumentConverter(allowed_formats=[InputFormat.XLSX,InputFormat.PPTX])
print(json.dumps({'ready':True}),flush=True)
for line in sys.stdin:
    q=json.loads(line);rows=[];batch=time.perf_counter_ns()
    for filename in q['documents']:
        path=Path(filename);start=time.perf_counter_ns()
        try:
            data=path.read_bytes();r=converter.convert(DocumentStream(name=path.name,stream=io.BytesIO(data)),raises_on_error=False)
            ok=r.status.value in ('success','partial_success');md=r.document.export_to_markdown() if ok else ''
            ns=time.perf_counter_ns()-start
            row={'path':filename,'elapsed_ns':ns,'ok':ok,'status':r.status.value,'markdown':md,'diagnostics':[str(e) for e in r.errors]}
            if not ok:row['error']=r.status.value
        except Exception as e:row={'path':filename,'elapsed_ns':time.perf_counter_ns()-start,'ok':False,'error':f'{type(e).__name__}: {e}','markdown':''}
        rows.append(row)
    print(json.dumps({'wall_ns':time.perf_counter_ns()-batch,'documents':rows}),flush=True)
