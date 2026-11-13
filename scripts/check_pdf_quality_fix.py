#!/usr/bin/env python3
"""Check native text preservation, inspection parity and existing table fixtures.

Allows intentional heading/reading-order changes, but requires the same per-page
non-whitespace character multiset with normalization explicitly disabled. This
does not measure semantic order or heading accuracy. Historical files stay intact.
"""
from bulkparse_cli import parse_command
import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import subprocess
from compare_pdf_inspectors import Worker, sha, source_hash
from validate_pdf_tables import compact, tables

ROOT = Path(__file__).resolve().parents[1]


def convert(binary, source, raw=False):
    args = [str(binary), parse_command(binary), str(source), '--to', 'json']
    if raw:
        args.append('--pdf-normalize-unicode=false')
    result = subprocess.run(args, capture_output=True, timeout=45)
    return result.returncode, json.loads(result.stdout) if result.stdout else None


def page_characters(document):
    pages = defaultdict(Counter)

    def visit(value, page=None):
        if isinstance(value, dict):
            page = value.get('source', {}).get('page', page)
            if isinstance(value.get('text'), str):
                pages[page].update(c for c in value['text'] if not c.isspace())
            for key, child in value.items():
                if key not in ('source', 'text'):
                    visit(child, page)
        elif isinstance(value, list):
            for child in value:
                visit(child, page)
    visit(document['blocks'])
    return dict(pages)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT/'target/debug/bulkparse')
    parser.add_argument('--before-worker', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, default=ROOT/'reports/pdf-tables-corpus-2026-11-10-v2')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    if args.out.exists():
        raise ValueError('Refusing to overwrite a previous report')
    baseline = {r['id']: r for r in map(json.loads, (args.baseline/'results.jsonl').read_text().splitlines())}
    manifest = list(map(json.loads, (args.baseline/'manifest.jsonl').read_text().splitlines()))
    scratch = ROOT/'target/pdf-quality-review'
    scratch.mkdir(parents=True, exist_ok=True)
    worker = Worker(args.before_worker.resolve(), 'inspect', scratch/'before-inspection.stderr')
    records = []
    try:
        for entry in manifest:
            if entry['format'] != 'pdf':
                continue
            source = Path(entry['source_path'])
            assert sha(source) == entry['source_sha256']
            old = baseline[entry['id']]
            code, raw = convert(args.binary, source, True)
            default_code, default = convert(args.binary, source)
            before = worker.call(entry)
            inspected = subprocess.run([str(args.binary), 'inspect', str(source)], capture_output=True, timeout=45)
            row = {'id': entry['id'], 'source_sha256': entry['source_sha256']}
            if old['outcome'] == 'failed':
                row['expected_failure'] = code == default_code == old['json_run']['exit_code'] and inspected.returncode == 1 and not before['ok']
            else:
                previous = json.loads(Path(old['json_run']['stdout_path']).read_text())
                inspection = json.loads(inspected.stdout)
                row.update(
                    successful=code == default_code == old['json_run']['exit_code'],
                    raw_page_characters_preserved=page_characters(raw['document']) == page_characters(previous['document']),
                    raw_table_cells_preserved=tables(raw) == tables(previous),
                    inspection_unchanged=before['ok'] and before['result']['report'] == inspection,
                    shared_inspection=inspection == raw['pdf_inspection'] == default['pdf_inspection'],
                    heading_count=sum(b['type'] == 'heading' for b in default['document']['blocks']))
            records.append(row)
    finally:
        usage = worker.close()
    specs = json.loads((ROOT/'benchmarks/pdf-tables/fresh-spec.json').read_text())
    known = {c['id']: c['sha256'] for c in json.loads((ROOT/'benchmarks/pdf-tables/fresh-manifest.json').read_text())}
    table_cases = []
    for case in specs:
        path = ROOT/'fixtures/pdf-tables'/(case['id']+'.pdf')
        assert sha(path) == known[case['id']]
        code, result = convert(args.binary, path)
        table_cases.append({'id': case['id'], 'source_sha256': known[case['id']],
                            'passed': code in (0, 2) and compact(tables(result)) == compact(case['expected_tables'])})
    checks = ['successful', 'raw_page_characters_preserved', 'raw_table_cells_preserved', 'inspection_unchanged', 'shared_inspection']
    passed = all(r.get('expected_failure', all(r.get(k, False) for k in checks)) for r in records)
    passed = passed and all(c['passed'] for c in table_cases) and usage['exit_code'] == 0
    report = {'passed': passed, 'binary_sha256': sha(args.binary), 'before_worker_sha256': sha(args.before_worker),
              'core_source_sha256': source_hash(ROOT/'crates/bulkparse/src'), 'harness_sha256': sha(__file__),
              'scope': __doc__, 'pdf_cases': records, 'table_cases': table_cases}
    args.out.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({'passed': passed, 'pdf_cases': len(records), 'table_cases': len(table_cases),
                      'failures': [r for r in records if not r.get('expected_failure', all(r.get(k, False) for k in checks))]}, indent=2))
    raise SystemExit(0 if passed else 1)


if __name__ == '__main__':
    main()
