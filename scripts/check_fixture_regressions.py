#!/usr/bin/env python3
"""Check structural regressions in saved evaluation outputs (no fixture copies)."""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def check(evaluation, baseline):
    manifest = (evaluation / 'manifest.jsonl').read_bytes()
    assert manifest == (baseline / 'manifest.jsonl').read_bytes(), 'Input/reference manifest changed'
    results = {r['id']: r for r in map(json.loads, (evaluation / 'results.jsonl').read_text().splitlines())}
    passed = []

    def document(key):
        return json.loads(Path(results[key]['json_run']['stdout_path']).read_text())

    key = 'docling/docx/sources/docx_sdt_table_cells.docx'
    doc = document(key)
    text = json.dumps(doc, ensure_ascii=False)
    for value in ['Cover value', 'A. Reviewer', 'Draft']:
        assert value in text, f'Missing {value}'
    tables = [b for b in doc['document']['blocks'] if b['type'] == 'table']
    assert [len(r) for r in tables[1]['rows']] == [3, 3]
    passed.append({'id': key, 'check': 'content-control values and three-column table preserved'})

    key = 'docling/docx/sources/docx_list_resumed_after_table.docx'
    lists = [b for b in document(key)['document']['blocks'] if b['type'] == 'list']
    assert [(b['start'], len(b['items'])) for b in lists] == [(1, 2), (3, 2)]
    passed.append({'id': key, 'check': 'inherited numbered list resumes at three after table'})

    for suffix in ['tab', 'pipe', 'semicolon']:
        key = f'docling/csv/sources/csv-{suffix}.csv'
        rows = document(key)['document']['blocks'][0]['rows']
        assert rows and all(len(row) == 12 for row in rows)
        passed.append({'id': key, 'check': 'all twelve columns preserved'})
    for suffix in ['semicolon', 'utf16']:
        key = f'anydoc/csv/handmade-{suffix}.csv'
        assert results[key]['outcome'] == 'complete'
        assert len(document(key)['document']['blocks'][0]['rows'][0]) > 1
        passed.append({'id': key, 'check': 'previously failing dialect/encoding produces table'})
    for key, counts in [
        ('docling/docx/sources/docx_code_blocks.docx', {'code': 2}),
        ('anydoc/docx/handmade-blockstyle.docx', {'code': 5, 'quote': 2}),
    ]:
        types = [b['type'] for b in document(key)['document']['blocks']]
        for kind, count in counts.items():
            assert types.count(kind) == count
        passed.append({'id': key, 'check': 'recognized paragraph styles produce code/quote blocks'})
    key = 'anydoc/malformed/corrupt-styles--skips.docx'
    assert results[key]['outcome'] == 'partial'
    assert 'docx_styles_unreadable' in results[key]['diagnostic_codes']
    assert document(key)['document']['blocks']
    passed.append({'id': key, 'check': 'optional style damage preserves body and reports partial'})
    assert all(r['outcome'] not in {'failed', 'crashed', 'stopped'} for r in results.values() if r['group'] == 'quality')
    assert results['anydoc/abuse/deepxml--errors.docx']['error']['code'] == 'resource_limit'
    assert results['anydoc/abuse/zipbomb--errors.docx']['error']['code'] == 'resource_limit'
    return {'manifest_sha256': hashlib.sha256(manifest).hexdigest(), 'checks': passed, 'quality_conversion_failures': 0}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('evaluation', type=Path)
    parser.add_argument('--baseline', type=Path, default=ROOT / 'reports/fixture-evaluation-2026-11-10-run2')
    args = parser.parse_args()
    print(json.dumps(check(args.evaluation, args.baseline), indent=2))
