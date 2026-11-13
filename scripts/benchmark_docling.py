#!/usr/bin/env python3
"""Optional pinned Docling adapter. Requires a separately provisioned environment.

Network/model downloads are disabled; supply predownloaded artifacts explicitly.
"""
import argparse
from importlib.metadata import version
import os
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path)
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--tables', action='store_true')
    args = parser.parse_args()
    for package, expected in [('docling', '2.150.0'), ('docling-parse', '7.23.0')]:
        actual = version(package)
        if actual != expected: parser.error(f'{package} must be {expected}, found {actual}')
    if not args.artifacts.is_dir(): parser.error('provide predownloaded Docling artifacts')
    os.environ['HF_HUB_OFFLINE'] = '1'
    os.environ['TRANSFORMERS_OFFLINE'] = '1'
    from docling.datamodel.base_models import InputFormat
    from docling.datamodel.accelerator_options import AcceleratorOptions, AcceleratorDevice
    from docling.datamodel.pipeline_options import PdfPipelineOptions
    from docling.document_converter import DocumentConverter, PdfFormatOption
    options = PdfPipelineOptions(artifacts_path=args.artifacts.resolve(), do_ocr=False,
                                 do_table_structure=args.tables, enable_remote_services=False,
                                 accelerator_options=AcceleratorOptions(num_threads=4, device=AcceleratorDevice.CPU))
    converter = DocumentConverter(allowed_formats=[InputFormat.PDF],
                                  format_options={InputFormat.PDF: PdfFormatOption(pipeline_options=options)})
    result = converter.convert(args.input)
    if result.status.value != 'success':
        raise RuntimeError(f'Docling conversion status: {result.status.value}')
    sys.stdout.write(result.document.export_to_markdown())


if __name__ == '__main__': main()
