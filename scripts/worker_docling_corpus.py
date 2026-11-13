#!/usr/bin/env python3
"""Persistent, sequential Docling corpus adapter with OCR off and tables on."""
import argparse
from importlib.metadata import version
import json
import logging
import os
from pathlib import Path
import sys
import time


def emit(value):
    print(json.dumps(value, ensure_ascii=False), flush=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--artifacts', type=Path, required=True)
    args = ap.parse_args()
    for package, expected in [('docling', '2.150.0'), ('docling-parse', '7.23.0')]:
        if version(package) != expected:
            raise RuntimeError(f'{package} must be {expected}')
    os.environ['HF_HUB_OFFLINE'] = '1'
    os.environ['TRANSFORMERS_OFFLINE'] = '1'
    logging.basicConfig(level=logging.WARNING)
    from docling.datamodel.base_models import InputFormat
    from docling.datamodel.accelerator_options import AcceleratorOptions, AcceleratorDevice
    from docling.datamodel.pipeline_options import PdfPipelineOptions, TableStructureOptions, TableFormerMode
    from docling.document_converter import DocumentConverter, PdfFormatOption
    options = PdfPipelineOptions(
        artifacts_path=args.artifacts.resolve(), do_ocr=False,
        do_table_structure=True,
        table_structure_options=TableStructureOptions(mode=TableFormerMode.ACCURATE, do_cell_matching=True),
        enable_remote_services=False,
        do_code_enrichment=False, do_formula_enrichment=False,
        do_picture_classification=False, do_picture_description=False,
        accelerator_options=AcceleratorOptions(num_threads=4, device=AcceleratorDevice.CPU))
    pdf_format = PdfFormatOption(pipeline_options=options)
    converter = DocumentConverter(allowed_formats=[InputFormat.PDF],
                                  format_options={InputFormat.PDF: pdf_format})
    converter.initialize_pipeline(InputFormat.PDF)
    effective = options.model_dump(mode='json', serialize_as_any=True)
    effective['table_structure_engine'] = type(options.table_structure_options).__name__
    effective['layout_engine'] = type(options.layout_options).__name__
    emit({'ready': True, 'options': effective, 'backend': pdf_format.backend.__name__,
          'pipeline': pdf_format.pipeline_cls.__name__})
    for index, line in enumerate(sys.stdin):
        request = json.loads(line)
        destination = Path(request['output_dir'])
        destination.mkdir(parents=True, exist_ok=True)
        documents = []
        started = time.perf_counter_ns()
        for path in map(Path, request['documents']):
            doc_started = time.perf_counter_ns()
            status = None
            diagnostics = []
            pages = None
            tables = None
            try:
                result = converter.convert(path, raises_on_error=False)
                status = result.status.value
                ok = status in ('success', 'partial_success')
                diagnostics = [str(error) for error in result.errors]
                error = None if ok else f'Docling status: {status}'
                markdown = result.document.export_to_markdown() if ok else ''
                pages = len(result.pages)
                tables = len(result.document.tables) if ok else 0
            except Exception as exc:
                ok, error, markdown = False, f'{type(exc).__name__}: {exc}', ''
            (destination / (path.stem + '.md')).write_text(markdown, encoding='utf-8')
            documents.append({'document_id': path.stem, 'ok': ok, 'status': status,
                              'error': error, 'diagnostics': diagnostics,
                              'pages': pages, 'tables': tables,
                              'elapsed_ns': time.perf_counter_ns() - doc_started})
            if len(documents) % 10 == 0:
                print(f'Pass {index}: {len(documents)}/{len(request["documents"])} documents', file=sys.stderr, flush=True)
        elapsed = time.perf_counter_ns() - started
        emit({'elapsed_ns': elapsed, 'documents': documents, 'options': effective})


if __name__ == '__main__':
    main()
