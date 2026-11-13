#!/usr/bin/env python3
"""Streaming Markdown: before/after materialized output and streaming file output, defaults and bounded recovery; warm-up and five rotating sequential corpus passes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import shutil

from benchmark_realworld import ROOT, REPORT, Worker, digest, write
from benchmark_opendataloader import snapshot_sources


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    corpus = json.loads((REPORT / 'corpus-manifest.json').read_text())
    baseline = ROOT / 'reports/streaming-2026-11-12'
    old_identity = json.loads((ROOT / 'reports/realworld-fixes-2026-11-12/benchmark-final/identity.json').read_text())
    commands = {'before': [baseline / 'before-office-bench'],
                'before_bounded': [baseline / 'before-office-bench', '--partial-on-limit'],
                'default': [ROOT / 'target/release/examples/office_bench'],
                'bounded': [ROOT / 'target/release/examples/office_bench', '--partial-on-limit'],
                'stream': [ROOT / 'target/release/examples/office_bench', '--stream-markdown'],
                'stream_bounded': [ROOT / 'target/release/examples/office_bench', '--stream-markdown', '--partial-on-limit']}
    hashes = {e: digest(p[0]) for e, p in commands.items()}
    profile_identity = json.loads((ROOT / 'reports/memory-2026-11-12/per-document-after/identity.json').read_text())
    assert hashes['before'] == profile_identity['binary_sha256']
    assert all(digest(ROOT / r['path']) == r['sha256'] for r in corpus)
    source_hashes = snapshot_sources(out)
    for p in [Path(__file__).resolve(), ROOT/'scripts/benchmark_realworld.py', ROOT/'crates/bulkparse/examples/office_bench.rs', ROOT/'scripts/profile_parser_memory.py', ROOT/'scripts/validate_large_documents.py', ROOT/'bindings/node/index.cjs', ROOT/'bindings/node/index.d.ts', ROOT/'scripts/check_streaming_pdf.py']:
        name=str(p.relative_to(ROOT));source_hashes[name]=digest(p)
        dest=out/'source-snapshot'/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dest)
    write(out / 'source-manifest.json', source_hashes)
    write(out / 'corpus-manifest.json', corpus)
    env = dict(os.environ, RAYON_NUM_THREADS='1', OMP_NUM_THREADS='1',
               OPENBLAS_NUM_THREADS='1', MKL_NUM_THREADS='1')
    write(out / 'identity.json', {
        'method': __doc__, 'warmup_passes': 1, 'measured_passes': 5,
        'hardware': old_identity['hardware'],
        'hardware_source': 'Same machine as the baseline; hardware identification retained from its recorded sysctl output.',
        'commands': {e: [str(v) for v in p] for e, p in commands.items()}, 'binary_sha256': hashes,
        'before_source_snapshot': str(baseline / 'before/source-snapshot'),
        'before_source_manifest_sha256': digest(baseline / 'before/source-manifest.json'),
        'configuration': 'Original default limits; OCR disabled; one persistent process per profile; bounded explicitly enables partial_on_limit. Recovery is a second attempt with the original caps, so total work can increase. Reading and Markdown rendering included; worker setup and output serialization excluded. Streaming includes temporary spool and output-file writing; file readback for verification is excluded from time but included in lifetime RSS. Materialized profiles return Markdown in memory and do not write a file.',
    })
    workers = {e: Worker([str(v) for v in p], out / (e + '.log'), env) for e, p in commands.items()}
    runs = {e: [] for e in commands}
    try:
        for i in range(6):
            order = list(commands); order = order[i % len(commands):] + order[:i % len(commands)]
            for e in order:
                rows = []
                for d in corpus:
                    row = workers[e].call(ROOT / d['path'])
                    md = row.pop('markdown')
                    row.update(id=d['id'], format=d['format'], output_sha256=hashlib.sha256(md.encode()).hexdigest())
                    if i == 1:
                        folder = out / 'predictions' / e
                        folder.mkdir(parents=True, exist_ok=True)
                        (folder / (d['id'] + '.md')).write_text(md)
                    rows.append(row)
                write(out / f'{e}-pass-{i}.json', {'documents': rows})
                runs[e].append(rows)
                print(f'{e} pass {i}: {sum(r["ok"] for r in rows)}/112 returned, {sum(r["elapsed_ns"] for r in rows)/1e9:.3f}s', flush=True)
    finally:
        for w in workers.values():
            w.close()
    assert all(digest(ROOT / n) == h for n, h in source_hashes.items())
    assert all(digest(p[0]) == hashes[e] for e, p in commands.items())
    assert all(digest(ROOT / r['path']) == r['sha256'] for r in corpus)
    versions = {}
    for e, passes in runs.items():
        samples = passes[1:]
        documents = []
        for j, d in enumerate(corpus):
            rs = [p[j] for p in samples]
            assert all(r['id'] == d['id'] for r in rs)
            documents.append({'id': d['id'], 'format': d['format'],
                              'returned': sum(r['ok'] for r in rs),
                              'median_ms': statistics.median(r['elapsed_ns'] / 1e6 for r in rs),
                              'deterministic': len({(r['ok'], r['output_sha256']) for r in rs}) == 1})
        peaks = re.findall(r'(\d+)\s+maximum resident set size', (out / (e + '.log')).read_text())
        versions[e] = {'documents': documents,
                       'returned_all_passes': sum(d['returned'] == 5 for d in documents),
                       'partial_count': sum(r.get('status') == 'partial' for r in samples[0]),
                       'resource_recovered_count': sum(any(d['code']=='resource_limit_partial' for d in r.get('diagnostics',[])) for r in samples[0]),
                       'corpus_seconds': [sum(r['elapsed_ns'] for r in p) / 1e9 for p in samples],
                       'median_corpus_seconds': statistics.median(sum(r['elapsed_ns'] for r in p) / 1e9 for p in samples),
                       'median_document_ms': statistics.median(d['median_ms'] for d in documents),
                       'lifetime_peak_rss_bytes': int(peaks[-1]) if peaks else None}
    first = {e: {r['id']: r for r in passes[1]} for e, passes in runs.items()}
    recovered = [d['id'] for d in corpus if not first['before'][d['id']]['ok'] and first['bounded'][d['id']]['ok']]
    regressed = [d['id'] for d in corpus if first['before'][d['id']]['ok'] and not first['bounded'][d['id']]['ok']]
    common = [j for j, d in enumerate(corpus) if all(versions[e]['documents'][j]['returned'] == 5 for e in commands)]
    summary = {'versions': versions, 'recovered_documents': recovered, 'conversion_regressions': regressed,
               'previously_successful_outputs_changed': [d['id'] for d in corpus if first['before'][d['id']]['ok'] and first['before'][d['id']]['output_sha256'] != first['bounded'][d['id']]['output_sha256']],
               'common_successful_documents': len(common),
               'common_successful_median_document_ms': {e: statistics.median(versions[e]['documents'][j]['median_ms'] for j in common) for e in commands},
               'remaining_failures': [{'id': r['id'], 'format': r['format'], 'error': r.get('error')} for r in runs['bounded'][1] if not r['ok']],
               'default_conversion_regressions': [d['id'] for d in corpus if first['before'][d['id']]['ok'] and not first['default'][d['id']]['ok']],
               'default_output_changes': [d['id'] for d in corpus if first['before'][d['id']]['output_sha256'] != first['default'][d['id']]['output_sha256']],
               'verification': 'All five measured passes complete; source, executable and input hashes verified unchanged. Recovery returns partial data, not complete conversion. No quality score inferred from conversion success.'}
    summary['bounded_output_changes'] = [d['id'] for d in corpus if (first['before_bounded'][d['id']]['ok'], first['before_bounded'][d['id']].get('status'), first['before_bounded'][d['id']]['output_sha256']) != (first['bounded'][d['id']]['ok'], first['bounded'][d['id']].get('status'), first['bounded'][d['id']]['output_sha256'])]
    summary['default_diagnostic_changes'] = [d['id'] for d in corpus if first['before'][d['id']].get('diagnostics') != first['default'][d['id']].get('diagnostics')]
    summary['bounded_diagnostic_changes'] = [d['id'] for d in corpus if first['before_bounded'][d['id']].get('diagnostics') != first['bounded'][d['id']].get('diagnostics')]
    summary['stream_comparisons'] = {}
    for reference, candidate in [('before','stream'), ('before_bounded','stream_bounded')]:
        differences=[]
        for d in corpus:
            a,b=first[reference][d['id']],first[candidate][d['id']]
            keys=['ok','status','output_sha256','diagnostics','render_diagnostics','error_code']
            changed=[k for k in keys if a.get(k)!=b.get(k)]
            if changed:differences.append({'id':d['id'],'fields':changed})
        summary['stream_comparisons'][candidate]=differences
    write(out / 'summary.json', summary)
    print(json.dumps({k: v for k, v in summary.items() if k != 'versions'}, indent=2))


if __name__ == '__main__':
    main()
