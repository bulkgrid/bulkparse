#!/usr/bin/env python3
"""Quality gates for the authored OCR corpus, including explicit known gaps."""
from collections import Counter
import json
from pathlib import Path
import sys

root=Path(__file__).resolve().parents[1]
report=Path(sys.argv[1])
rows=json.loads((report/'summary.json').read_text())
ocr={r['id']:r for r in rows if r['profile']=='ocr'}
assert len(ocr)==8
for name,row in ocr.items():
    codes={d['code'] for d in row['diagnostics']}
    assert row['status']=='partial'
    assert 'pdf_ocr_error' not in codes and 'pdf_ocr_budget' not in codes
    if name=='ocr-blank':
        assert 'pdf_ocr_empty' in codes and 'pdf_ocr_applied' not in codes
    else: assert 'pdf_ocr_applied' in codes
    if name != 'ocr-swedish':
        assert row['quality']['exact_after_whitespace'],name
assert ocr['ocr-swedish']['quality']['word_error_rate'] <= 1/9 + 1e-8
assert ocr['ocr-swedish']['quality']['character_error_rate'] <= 1/67 + 1e-8
actual=(report/'ocr-columns-ocr.txt').read_text().split()
expected=(root/'fixtures/ocr/ocr-columns.txt').read_text().split()
assert Counter(actual)==Counter(expected),'column scan lost or changed words'
assert ocr['ocr-columns']['quality']['exact_line_pair_order'] == 1.0
print('OCR quality gates pass; column order is exact on the authored fixture; the Swedish-character gap remains explicitly measured.')
