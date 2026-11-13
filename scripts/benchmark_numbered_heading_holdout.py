#!/usr/bin/env python3
"""Rerun the observed DocLayNet sample with its frozen references and scoring.

This sample is now a regression set, not a new unseen generalization test.
The original evaluator, protocol, inputs and historical report remain unchanged.
"""
from bulkparse_cli import parse_command
import argparse
from collections import Counter
import datetime
import hashlib
import json
from pathlib import Path
import random
import statistics
import subprocess

import evaluate_independent_pdf as evaluation
from check_pdf_quality_fix import page_characters

ROOT = Path(__file__).resolve().parents[1]
VERSIONS = ['before', 'current']

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def read(path):
    return json.loads(path.read_text())

def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False)+'\n')

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    for name in ['before-cli','after-cli','before-worker','after-worker','out']:
        ap.add_argument('--'+name, type=Path, required=True)
    ap.add_argument('--after-source-report', type=Path, default=ROOT/'reports/opendataloader-numbered-heading-2026-11-11-v2')
    a = ap.parse_args()
    out = a.out.resolve()
    if out.exists():
        raise ValueError('Use a fresh report directory')
    spec = ROOT/'benchmarks/independent-pdf-v2'
    old_report = ROOT/'reports/independent-pdf-v2-2026-11-11'
    old = read(old_report/'quality-summary.json')
    assert sha(ROOT/'scripts/evaluate_independent_pdf.py') == old['identity']['harness_sha256']
    assert sha(spec/'PROTOCOL.md') == old['identity']['protocol_sha256']
    assert sha(ROOT/'target/independent-pdf-v2/references.json') == old['identity']['reference_sha256']
    assert sha(a.before_cli) == old['identity']['binary_sha256']['current']
    assert sha(a.before_worker) == read(old_report/'timing.json')['worker_binary_sha256']['current']
    frozen = read(spec/'freeze.json')['versions']['current']
    manifests = {'before': frozen['source_manifest'], 'current': read(a.after_source_report/'source-manifest.json')}
    source_roots = {'before': ROOT/'reports'/frozen['report']/'source-snapshot', 'current': a.after_source_report/'source-snapshot'}
    for v in VERSIONS:
        for name, h in manifests[v].items():
            assert sha(source_roots[v]/name) == h
            if v == 'current':
                assert sha(ROOT/name) == h
    manifest = read(spec/'manifest.json')
    excluded = {r['id'] for r in read(spec/'overlap-audit.json')['rows'] if r['excluded']}
    selected = [r for r in manifest['rows'] if Path(r['image']['file_name']).stem not in excluded]
    assert len(selected) == 57
    for row in selected:
        for kind, digest in row['sha256'].items():
            assert sha(ROOT/row['paths'][kind]) == digest
    refs = read(ROOT/'target/independent-pdf-v2/references.json')
    binaries = {'before': a.before_cli.resolve(), 'current': a.after_cli.resolve()}
    workers = {'before': a.before_worker.resolve(), 'current': a.after_worker.resolve()}
    scratch = ROOT/'target/numbered-heading-fix/independent'
    identity = {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                'sample_is_now_observed': True, 'documents': len(selected),
                'historical_report': str(old_report.relative_to(ROOT)),
                'harness_sha256': sha(Path(__file__)), 'evaluator_sha256': sha(ROOT/'scripts/evaluate_independent_pdf.py'),
                'reference_sha256': old['identity']['reference_sha256'],
                'sample_manifest_sha256': sha(spec/'manifest.json'),
                'source_manifests': manifests, 'source_roots': {k:str(p.relative_to(ROOT)) for k,p in source_roots.items()},
                'cli_sha256': {v:sha(p) for v,p in binaries.items()},
                'worker_sha256': {v:sha(p) for v,p in workers.items()},
                'command': __import__('sys').argv}
    write(out/'identity.json',identity)
    rows = []
    for selected_row in selected:
        id = Path(selected_row['image']['file_name']).stem
        row = {'id':id, 'source':selected_row['image'], 'versions':{}}
        parsed = {}
        for v in VERSIONS:
            parsed[v] = {}
            dest = scratch/'json'/v
            dest.mkdir(parents=True,exist_ok=True)
            for raw in [False,True]:
                command = [str(binaries[v]), parse_command(binaries[v]), str(ROOT/selected_row['paths']['pdfs']), '--to', 'json']
                if raw:
                    command.append('--pdf-normalize-unicode=false')
                run = subprocess.run(command,capture_output=True,timeout=60)
                path = dest/(id+('.raw' if raw else '')+'.json')
                path.write_bytes(run.stdout)
                path.with_suffix('.stderr').write_bytes(run.stderr)
                parsed[v][raw] = (run.returncode,json.loads(run.stdout) if run.stdout else None)
            code, value = parsed[v][False]
            scores = evaluation.evaluate(value,refs[id])
            scores.update(exit_code=code, raw_exit_code=parsed[v][True][0], status=value.get('status') if value else None,
                          prediction_json_sha256=sha(dest/(id+'.json')),raw_json_sha256=sha(dest/(id+'.raw.json')))
            row['versions'][v] = scores
        before, current = (parsed[v][True][1] for v in VERSIONS)
        b, c = (row['versions'][v] for v in VERSIONS)
        row['comparison'] = {
            'native_characters_preserved': bool(before and current and page_characters(before['document']) == page_characters(current['document'])),
            'inspection_unchanged': bool(before and current and before['pdf_inspection'] == current['pdf_inspection']),
            'status_unchanged': b['status']==c['status'] and b['exit_code']==c['exit_code'] and b['raw_exit_code']==c['raw_exit_code'],
            'table_structures_unchanged': b['table_structures']==c['table_structures'],
            'headings_unchanged': b['headings']==c['headings'],
            'normalized_text_order_unchanged': b['normalized_text_sha256']==c['normalized_text_sha256'],
            'json_byte_identical': b['prediction_json_sha256']==c['prediction_json_sha256'],
            'heading_tp_delta': c['heading_fuzzy']['tp']-b['heading_fuzzy']['tp'],
            'heading_fp_delta': c['heading_fuzzy']['fp']-b['heading_fuzzy']['fp'],
            'heading_fn_delta': c['heading_fuzzy']['fn']-b['heading_fuzzy']['fn']}
        for scores in row['versions'].values():
            tables = scores.pop('table_structures')
            scores['table_structure_sha256'] = hashlib.sha256(json.dumps(tables,sort_keys=True).encode()).hexdigest()
            scores['table_count'] = len(tables)
        rows.append(row)
    evaluation.VERSIONS = VERSIONS
    summary = evaluation.aggregate(rows)
    assert summary['before'] == old['summary']['current'], 'Fresh before scores differ from the saved study'
    historical = {r['id']:r for r in old['documents']}
    assert all(r['versions']['before']['prediction_json_sha256']==historical[r['id']]['versions']['current']['prediction_json_sha256'] for r in rows)
    categories = {cat:evaluation.aggregate([r for r in rows if r['source']['doc_category']==cat]) for cat in sorted({r['source']['doc_category'] for r in rows})}
    rng = random.Random('bulkparse-independent-v2-bootstrap')
    deltas = []
    for _ in range(2000):
        agg = evaluation.aggregate([rng.choice(rows) for _ in rows])
        deltas.append(agg['current']['heading_fuzzy']['f1']-agg['before']['heading_fuzzy']['f1'])
    quality = {'summary':summary,'categories':categories,'documents':rows,
               'heading_f1_change':{'delta':summary['current']['heading_fuzzy']['f1']-summary['before']['heading_fuzzy']['f1'],
                                    'paired_bootstrap_95_percentile':[sorted(deltas)[49],sorted(deltas)[1949]],'draws':2000},
               'fresh_before_reproduces_saved_quality_and_json':True}
    write(out/'quality.json',quality)
    print(json.dumps({'summary':summary,'changed_pages':[r['id'] for r in rows if not r['comparison']['json_byte_identical']]},indent=2),flush=True)
    processes = {}; logs = {}
    paths = sorted(str((ROOT/r['paths']['pdfs']).resolve()) for r in selected)
    open_paths = sorted(str(p.resolve()) for p in (ROOT/'target/opendataloader-bench/pdfs').glob('*.pdf'))
    assert len(open_paths) == 200
    try:
        for v in VERSIONS:
            logs[v] = open(out/(v+'-worker.stderr.log'),'w')
            processes[v] = subprocess.Popen([str(workers[v])],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=logs[v],text=True)
        for corpus, corpus_paths in [('independent',paths),('opendataloader',open_paths)]:
            runs = {v:[] for v in VERSIONS}
            for i in range(6):
                order = VERSIONS if i%2==0 else list(reversed(VERSIONS))
                for v in order:
                    dest = scratch/'markdown'/corpus/v
                    p = processes[v]
                    p.stdin.write(json.dumps({'documents':corpus_paths,'output_dir':str(dest)})+'\n');p.stdin.flush()
                    run = json.loads(p.stdout.readline())
                    run.update(round=i,warmup=i==0,version_order=order,prediction_sha256={p.name:sha(p) for p in dest.glob('*.md')})
                    assert len(run['documents'])==len(run['prediction_sha256'])==len(corpus_paths)
                    if runs[v]:
                        assert run['prediction_sha256']==runs[v][0]['prediction_sha256']
                    runs[v].append(run)
            if corpus == 'opendataloader':
                # Fresh before/after predictions must reproduce the independently
                # scored original reports, not merely produce the same totals.
                for v, report in [('before',ROOT/'reports'/frozen['report']),('current',a.after_source_report)]:
                    assert runs[v][-1]['prediction_sha256'] == read(report/'runs.json')[-1]['prediction_sha256']
            timing = {'corpus':corpus,'documents':len(corpus_paths),'runs':runs,'summary':{v:{'median_seconds':statistics.median(r['elapsed_ns'] for r in rs[1:])/1e9,
                       'measured_seconds':[r['elapsed_ns']/1e9 for r in rs[1:]],'errors':sum(not d['ok'] for r in rs for d in r['documents']),
                       'deterministic':True} for v,rs in runs.items()}}
            write(out/('timing.json' if corpus=='independent' else 'opendataloader-timing.json'),timing)
            print(corpus,json.dumps(timing['summary'],indent=2))
    finally:
        for v,p in processes.items():
            p.stdin.close();assert p.wait(timeout=15)==0;logs[v].close()

if __name__ == '__main__':
    main()
