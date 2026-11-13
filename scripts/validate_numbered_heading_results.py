#!/usr/bin/env python3
"""Reconcile both reruns with their saved predictions, sources and references."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import statistics

import evaluate_independent_pdf as evaluator
from check_opendataloader_results import check
from check_pdf_quality_fix import page_characters

ROOT = Path(__file__).resolve().parents[1]
OPEN = ROOT/'reports/opendataloader-numbered-heading-2026-11-11-v2'
INDEPENDENT = ROOT/'reports/independent-numbered-heading-2026-11-11'

def read(path):
    return json.loads(path.read_text())

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    open_validation = check(OPEN)
    q = read(INDEPENDENT/'quality.json')
    identity = read(INDEPENDENT/'identity.json')
    assert identity['harness_sha256'] == sha(ROOT/'scripts/benchmark_numbered_heading_holdout.py')
    assert identity['evaluator_sha256'] == sha(ROOT/'scripts/evaluate_independent_pdf.py')
    for v, manifest in identity['source_manifests'].items():
        source = ROOT/identity['source_roots'][v]
        for name, digest in manifest.items():
            assert sha(source/name) == digest
            if v == 'current':
                assert sha(ROOT/name) == digest
    reference_path = ROOT/'target/independent-pdf-v2/references.json'
    assert sha(reference_path) == identity['reference_sha256']
    refs = read(reference_path)
    assert len(q['documents']) == 57
    counts = Counter()
    for row in q['documents']:
        raw = {}
        for v, scores in row['versions'].items():
            base = ROOT/'target/numbered-heading-fix/independent/json'/v
            path = base/(row['id']+'.json')
            assert sha(path) == scores['prediction_json_sha256']
            raw_path = base/(row['id']+'.raw.json')
            assert sha(raw_path) == scores['raw_json_sha256']
            raw[v] = read(raw_path)
            computed = evaluator.evaluate(read(path),refs[row['id']])
            table_hash = hashlib.sha256(json.dumps(computed.pop('table_structures'),sort_keys=True).encode()).hexdigest()
            assert table_hash == scores['table_structure_sha256']
            for name, value in computed.items():
                assert scores[name] == value, (row['id'],v,name)
        assert page_characters(raw['before']['document']) == page_characters(raw['current']['document'])
        assert raw['before']['pdf_inspection'] == raw['current']['pdf_inspection']
        for field in ['native_characters_preserved','inspection_unchanged','status_unchanged','table_structures_unchanged']:
            assert row['comparison'][field]
        counts['changed_json_pages'] += not row['comparison']['json_byte_identical']
        b, c = [row['versions'][v]['heading_fuzzy'] for v in ['before','current']]
        f1 = lambda s: 2*s['tp']/(2*s['tp']+s['fp']+s['fn']) if 2*s['tp']+s['fp']+s['fn'] else 1
        counts['heading_improved_pages'] += f1(c)>f1(b)
        counts['heading_regressed_pages'] += f1(c)<f1(b)
    evaluator.VERSIONS = ['before','current']
    assert evaluator.aggregate(q['documents']) == q['summary']
    previous = ROOT/'reports/opendataloader-remaining-headings-2026-11-11'
    assert read(previous/'prediction/bulkparse/evaluation.json')['documents'] == read(OPEN/'prediction/bulkparse/evaluation.json')['documents']
    for filename, n in [('timing.json',57),('opendataloader-timing.json',200)]:
        timing = read(INDEPENDENT/filename)
        assert timing['documents'] == n
        for v, runs in timing['runs'].items():
            assert len(runs)==6 and [r['warmup'] for r in runs]==[True]+[False]*5
            assert statistics.median(r['elapsed_ns'] for r in runs[1:])/1e9 == timing['summary'][v]['median_seconds']
            for i,r in enumerate(runs):
                assert len(r['documents'])==n
                assert r['version_order']==(['before','current'] if i%2==0 else ['current','before'])
                assert all(d['ok'] for d in r['documents'])
                assert r['prediction_sha256'] == runs[0]['prediction_sha256']
            if n==200:
                expected = read((previous if v=='before' else OPEN)/'runs.json')[-1]['prediction_sha256']
                assert runs[-1]['prediction_sha256'] == expected
    regression = read(OPEN/'regression.json')
    assert regression['passed']
    result = {'passed':True,'opendataloader':open_validation,'opendataloader_all_200_scores_and_predictions_unchanged':True,
              'independent_documents':57,'independent_heading_reference_regions':132,
              'independent_metrics_recomputed_from_predictions':True,
              'native_characters_inspection_and_tables_preserved_on_57_pages':True,
              'before_after_timing_parser_calls':2*6*(57+200),
              'other_preservation_checks':len(regression['pdf_cases']),
              'authored_table_checks':len(regression['authored_table_cases']),**counts}
    (INDEPENDENT/'validation.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))

if __name__ == '__main__':
    main()
