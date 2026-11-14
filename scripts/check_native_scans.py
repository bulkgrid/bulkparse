#!/usr/bin/env python3
"""Run the hash-pinned scanned-layout development set through the native binding."""
import argparse
import hashlib
import json
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'bindings/python/python'))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--config', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    from bulkparse import OcrParser
    manifest = json.loads((ROOT / 'target/scan-parity/manifest.json').read_text())
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with OcrParser(json.loads(args.config.read_text())) as parser, args.out.open('x') as output:
        for case in manifest:
            path = ROOT / 'target/scan-parity/inputs' / (case['id'] + '.png')
            assert hashlib.sha256(path.read_bytes()).hexdigest() == case['scan_sha256']
            start = time.perf_counter()
            result = parser.parse_file(path, budget={'max_pages': 2000, 'max_output_bytes': 33554432})
            row = {'path': str(path), 'seconds': time.perf_counter() - start, 'result': result}
            output.write(json.dumps(row, ensure_ascii=False) + '\n')
            output.flush()
            print(case['id'], round(row['seconds'], 3), flush=True)
            if any(d['code'] in ('image_ocr_failed', 'image_ocr_budget_exhausted') for d in result['diagnostics']):
                raise RuntimeError('OCR did not complete')


if __name__ == '__main__':
    main()
