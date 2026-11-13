#!/usr/bin/env python3
"""Benchmark-only persistent Docling JSONL worker; no network or result cache."""
import argparse
import json
import os
from pathlib import Path
import sys
import time
from importlib.metadata import version


def emit(value):
    print(json.dumps(value,ensure_ascii=False),flush=True)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--artifacts',type=Path,required=True)
    args=p.parse_args()
    for name,v in [('docling','2.150.0'),('docling-parse','7.23.0')]:
        if version(name)!=v:raise RuntimeError(f'{name} must be {v}')
    if not args.artifacts.is_dir():raise RuntimeError('local model directory required')
    os.environ['HF_HUB_OFFLINE']='1';os.environ['TRANSFORMERS_OFFLINE']='1'
    from docling.datamodel.base_models import InputFormat
    from docling.datamodel.accelerator_options import AcceleratorOptions,AcceleratorDevice
    from docling.datamodel.pipeline_options import PdfPipelineOptions
    from docling.document_converter import DocumentConverter,PdfFormatOption
    options=PdfPipelineOptions(artifacts_path=args.artifacts.resolve(),do_ocr=False,
        do_table_structure=False,enable_remote_services=False,
        accelerator_options=AcceleratorOptions(num_threads=4,device=AcceleratorDevice.CPU))
    converter=DocumentConverter(allowed_formats=[InputFormat.PDF],
        format_options={InputFormat.PDF:PdfFormatOption(pipeline_options=options)})
    converter.initialize_pipeline(InputFormat.PDF)
    emit({'ready':True,'pid':os.getpid()})
    for line in sys.stdin:
        request=json.loads(line);started=time.perf_counter()
        try:
            source=Path(request['input'])
            if source.stat().st_size>33_554_432:raise ValueError('input limit')
            result=converter.convert(source)
            if result.status.value!='success':raise RuntimeError(f'Docling status: {result.status.value}')
            response={'ok':True,'status':result.status.value,'markdown':result.document.export_to_markdown()}
        except Exception as error:
            response={'ok':False,'error':f'{type(error).__name__}: {error}','markdown':''}
        response.update(id=request['id'],service_ms=(time.perf_counter()-started)*1000)
        emit(response)


if __name__=='__main__':main()
