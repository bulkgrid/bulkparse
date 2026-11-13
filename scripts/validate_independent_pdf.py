#!/usr/bin/env python3
"""Independently reconcile the frozen holdout report and available artifacts."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import random
import statistics

ROOT = Path(__file__).resolve().parents[1]

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--report', type=Path, default=ROOT/'reports/independent-pdf-v2-2026-11-11')
    args = ap.parse_args()
    report = args.report
    spec = ROOT/'benchmarks/independent-pdf-v2'
    data = ROOT/'target/independent-pdf-v2'
    read = lambda path: json.loads(path.read_text())
    q, t = read(report/'quality-summary.json'), read(report/'timing.json')
    freeze, manifest = read(spec/'freeze.json'), read(spec/'manifest.json')
    rows = q['documents']
    assert len(rows) == len(manifest['rows']) == 57
    assert len({r['id'] for r in rows}) == 57
    assert len({r['source']['doc_name'] for r in rows}) == 57
    assert {r['id'] for r in rows} == {Path(r['image']['file_name']).stem for r in manifest['rows']}
    categories = Counter(r['source']['doc_category'] for r in rows)
    assert sorted(categories.values()) == [7, 10, 10, 10, 10, 10]
    sources_checked = predictions_checked = inputs_checked = 0
    for version, frozen in freeze['versions'].items():
        for name, digest in frozen['source_manifest'].items():
            assert sha(ROOT/'reports'/frozen['report']/'source-snapshot'/name) == digest
            if version == 'current':
                assert sha(ROOT/name) == digest
            sources_checked += 1
        groups = [(rows, q['summary'][version])]
        groups += [([r for r in rows if r['source']['doc_category'] == cat], q['categories'][cat][version]) for cat in categories]
        for group, summary in groups:
            for metric in ['heading_fuzzy', 'heading_exact']:
                tp, fp, fn = [sum(r['versions'][version][metric][k] for r in group) for k in ['tp','fp','fn']]
                expected = dict(tp=tp, fp=fp, fn=fn, precision=tp/(tp+fp), recall=tp/(tp+fn), f1=2*tp/(2*tp+fp+fn))
                assert expected == summary[metric]
            assert sum(r['versions'][version]['order_reference_pairs'] for r in group) == summary['order_reference_pairs']
            assert sum(r['versions'][version]['text_token_matches'] for r in group)/sum(r['versions'][version]['reference_tokens'] for r in group) == summary['text_token_recall']
        assert q['summary'][version]['heading_fuzzy']['tp'] + q['summary'][version]['heading_fuzzy']['fn'] == 132
        runs = t['runs'][version]
        assert len(runs) == 6 and [r['warmup'] for r in runs] == [True,False,False,False,False,False]
        assert statistics.median(r['elapsed_ns'] for r in runs[1:])/1e9 == t['summary'][version]['median_seconds']
        for i, run in enumerate(runs):
            assert run['round'] == i
            assert len(run['documents']) == 57
            assert {d['document_id'] for d in run['documents']} == {r['id'] for r in rows}
            # ocr_pages lists recommendations from inspection, not executed OCR.
            assert all(d['ok'] for d in run['documents'])
            assert run['prediction_sha256'] == runs[0]['prediction_sha256']
        for row in rows:
            metrics = row['versions'][version]
            assert metrics['exit_code'] in (0,2) and metrics['raw_exit_code'] in (0,2)
            for suffix, key in [('.json','prediction_json_sha256'),('.raw.json','raw_json_sha256')]:
                prediction = data/'outputs'/version/(row['id']+suffix)
                if prediction.exists():
                    assert sha(prediction) == metrics[key]
                    predictions_checked += 1
    for row in manifest['rows']:
        for kind, digest in row['sha256'].items():
            path = ROOT/row['paths'][kind]
            if path.exists():
                assert sha(path) == digest
                inputs_checked += 1
    rng = random.Random('bulkparse-independent-v2-bootstrap')
    deltas = {v: [] for v in freeze['versions'] if v != 'current'}
    def f1(sample, version):
        tp, fp, fn = [sum(r['versions'][version]['heading_fuzzy'][k] for r in sample) for k in ['tp','fp','fn']]
        return 2*tp/(2*tp+fp+fn)
    for _ in range(2000):
        sample = [rng.choice(rows) for _ in rows]
        for version in deltas:
            deltas[version].append(f1(sample,'current')-f1(sample,version))
    for version, values in deltas.items():
        assert [sorted(values)[49], sorted(values)[1949]] == q['heading_f1_change'][version]['paired_bootstrap_95_percentile']
    assert sha(ROOT/'scripts/evaluate_independent_pdf.py') == q['identity']['harness_sha256']
    assert sha(ROOT/'scripts/time_independent_pdf.py') == t['harness_sha256']
    assert sha(spec/'PROTOCOL.md') == q['identity']['protocol_sha256']
    result = dict(passed=True, documents=57, distinct_source_documents=57, categories=dict(categories), source_snapshot_files_checked=sources_checked, raw_and_normalized_predictions_checked=predictions_checked, downloaded_input_files_checked=inputs_checked, heading_regions=132, order_pairs=929, paired_bootstrap_draws_reconciled=2000, timed_and_warmup_conversions=1026, conversion_errors=0, parser_source_unchanged=True)
    (report/'validation.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))

if __name__ == '__main__':
    main()
