#!/usr/bin/env python3
"""Replay accounting, hashes and upstream scores for the same-machine comparison."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import sys
from benchmark_same_machine import ROOT, ENGINES, validate_run, read
from benchmark_opendataloader import sha256, write_json


def check(out):
    identity = read(out/'identity.json')
    assert identity['protocol']['ocr'] is False
    assert read(out/'worker-exits.json') == {e:0 for e in ENGINES}
    source = read(out/'source-manifest.json')
    for name,digest in source.items():
        assert sha256(out/'source-snapshot'/name) == digest
    upstream = ROOT/'target/opendataloader-bench'
    corpus = read(out/'corpus-manifest.json')
    for name,digest in corpus['sha256'].items():
        assert sha256(upstream/name) == digest
    paths = [upstream/'pdfs'/(name+'.pdf') for name in corpus['document_ids']]
    assert len(paths) == 200 and len(set(paths)) == 200
    options = read(out/'docling-effective-options.json')['options']
    assert options['do_ocr'] is False and options['do_table_structure'] is True
    assert options['table_structure_options']['mode'] == 'accurate'
    assert options['table_structure_options']['do_cell_matching'] is True
    assert options['accelerator_options']['device'] == 'cpu'
    assert options['accelerator_options']['num_threads'] == 4
    assert options['enable_remote_services'] is False
    assert options['do_code_enrichment'] is False and options['do_formula_enrichment'] is False
    sys.path.insert(0,str(upstream/'src'))
    from evaluator import _evaluate_single_document, _aggregate_document_scores
    runs = read(out/'runs.json')
    comparison = read(out/'comparison.json')
    reports = []
    for row in comparison:
        engine = row['engine']; rs = runs[engine]
        assert len(rs) == 6 and [r['warmup'] for r in rs] == [True]+[False]*5
        for index,r in enumerate(rs):
            assert r['index'] == index
            assert r['engine_order'] == ENGINES[index%3:]+ENGINES[:index%3]
            assert validate_run(r,paths,out/'prediction'/engine/f'round-{index}'/'markdown') == r['prediction_sha256']
            assert r['options'] == rs[0]['options']
        assert row['median_seconds'] == statistics.median(r['elapsed_ns'] for r in rs[1:])/1e9
        assert row['measured_seconds'] == [r['elapsed_ns']/1e9 for r in rs[1:]]
        assert row['deterministic'] == all(r['prediction_sha256'] == rs[0]['prediction_sha256'] for r in rs)
        assert row['quality_rounds'] == ([5] if row['deterministic'] else list(range(1,6)))
        assert row['failures_all_runs'] == sum(not d['ok'] for r in rs for d in r['documents'])
        assert row['failed_documents_last_pass'] == [d['document_id'] for d in rs[-1]['documents'] if not d['ok']]
        metrics = []
        for index in row['quality_rounds']:
            d = out/'prediction'/engine/f'round-{index}'
            saved = read(d/'evaluation.json')
            scored = [_evaluate_single_document(p.stem,upstream/'ground-truth/markdown'/(p.stem+'.md'),d/'markdown'/(p.stem+'.md')) for p in paths]
            assert [s.to_json() for s in scored] == saved['documents']
            assert _aggregate_document_scores(scored) == saved['metrics']
            metrics.append(saved['metrics']['score'])
        assert row['metrics'] == {k:statistics.mean(m[k] for m in metrics) for k in metrics[0]}
        reports.append({'engine':engine,'documents':200,'runs':6,'attempts':1200,'all_scores_recomputed':True,
                        'deterministic':row['deterministic'],'failures_all_runs':row['failures_all_runs']})
    assert {r['engine'] for r in comparison} == set(ENGINES)
    reference = ROOT/identity.get('bulkparse_reference_report','reports/opendataloader-table-boundaries-2026-11-11-v3')
    prior = read(reference/'runs.json')[-1]['prediction_sha256']
    assert runs['bulkparse'][-1]['prediction_sha256'] == prior
    result = {'passed':True,'engine_checks':reports,'bulkparse_matches_previous_200_predictions':True,
              'docling_ocr_disabled_and_accurate_tables_enabled':True,'source_and_input_hashes_verified':True}
    write_json(out/'validation.json',result)
    print(json.dumps(result,indent=2))


if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('report',type=Path)
    check(p.parse_args().report.resolve())
