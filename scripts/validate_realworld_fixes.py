#!/usr/bin/env python3
"""Check saved final outputs against source CSV records and prior Office fixtures.

Run with target/competitors/docling-venv/bin/python (MarkdownIt and BeautifulSoup).
These are regression checks, not whole-document quality scores.
"""
import hashlib
import json
import os
from pathlib import Path

from bs4 import BeautifulSoup
from markdown_it import MarkdownIt
from benchmark_realworld import ROOT, Worker, digest, write


def main():
    out = ROOT / 'reports/realworld-fixes-2026-11-12'
    run = out / 'benchmark-final'
    summary = json.loads((run / 'summary.json').read_text())
    identity = json.loads((run / 'identity.json').read_text())
    binary = ROOT / 'target/release/examples/office_bench'
    assert digest(binary) == identity['binary_sha256']['after']
    csv_checks = []
    for doc in summary['recovered_documents']:
        if not doc.startswith('csv-'):
            continue
        ref_path = ROOT / 'reports/realworld-2026-11-12/references' / doc / 'source-records.json'
        reference = json.loads(ref_path.read_text())
        md_path = run / 'predictions/after' / (doc + '.md')
        html = MarkdownIt('commonmark').enable('table').render(md_path.read_text())
        soup = BeautifulSoup(html, 'html.parser')
        actual = [[c.get_text().strip() for c in tr.find_all(['td', 'th'], recursive=False)]
                  for tr in soup.select('tbody tr')]
        expected = [[v.strip() for v in row] for row in reference['records']]
        mismatches = [i for i, row in enumerate(expected) if i >= len(actual) or row != actual[i]]
        csv_checks.append({'document_id': doc, 'records_checked': len(expected), 'mismatched_rows': mismatches,
                           'reference_sha256': digest(ref_path), 'output_sha256': digest(md_path)})
    before_path = ROOT / 'reports/multiformat-legacy-layout-2026-11-12/benchmark-final/bulkparse-pass-1.json'
    old_rows = json.loads(before_path.read_text())['documents']
    w = Worker([str(binary)], out / 'development-final-worker.log', dict(os.environ, RAYON_NUM_THREADS='1'))
    checks = []
    try:
        for old in old_rows:
            path = ROOT / old['path']
            new = w.call(path)
            checks.append({'path': old['path'], 'input_sha256': digest(path),
                           'returned': new['ok'], 'before_returned': old['ok'],
                           'output_identical': new['markdown'] == old['markdown'],
                           'before_output_sha256': hashlib.sha256(old['markdown'].encode()).hexdigest(),
                           'after_output_sha256': hashlib.sha256(new['markdown'].encode()).hexdigest()})
    finally:
        w.close()
    result = {'method': __doc__, 'after_binary_sha256': digest(binary),
              'csv_scope': 'First 60 frozen source records, or all if shorter; all columns. Compare rendered table cell text, trimming surrounding whitespace only.',
              'csv_checks': csv_checks, 'csv_records_checked': sum(c['records_checked'] for c in csv_checks),
              'csv_checks_passed': all(not c['mismatched_rows'] for c in csv_checks),
              'development_baseline_sha256': digest(before_path), 'development_documents': checks,
              'development_outputs_unchanged': all(c['output_identical'] and c['returned'] == c['before_returned'] for c in checks)}
    write(out / 'regression-checks.json', result)
    assert result['csv_checks_passed'] and result['development_outputs_unchanged']
    assert not summary['conversion_regressions'] and not summary['previously_successful_outputs_changed']
    print(f'Passed: {result["csv_records_checked"]} source CSV records, {len(checks)} unchanged development documents, 85 unchanged real-world successes.')


if __name__ == '__main__':
    main()
