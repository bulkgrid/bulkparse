#!/usr/bin/env python3
"""Paired BulkParse before/after check; warm-up then five alternating corpus passes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics

from benchmark_realworld import ROOT, REPORT, Worker, digest, write
from benchmark_opendataloader import snapshot_sources


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    corpus = json.loads((REPORT / 'corpus-manifest.json').read_text())
    baseline = REPORT / 'conversion'
    old_identity = json.loads((baseline / 'identity.json').read_text())
    commands = {'before': baseline / 'baseline-office-bench',
                'after': ROOT / 'target/release/examples/office_bench'}
    hashes = {e: digest(p) for e, p in commands.items()}
    assert hashes['before'] == old_identity['binary_sha256']['bulkparse']
    assert all(digest(ROOT / r['path']) == r['sha256'] for r in corpus)
    source_hashes = snapshot_sources(out)
    write(out / 'source-manifest.json', source_hashes)
    write(out / 'corpus-manifest.json', corpus)
    env = dict(os.environ, RAYON_NUM_THREADS='1', OMP_NUM_THREADS='1',
               OPENBLAS_NUM_THREADS='1', MKL_NUM_THREADS='1')
    write(out / 'identity.json', {
        'method': __doc__, 'warmup_passes': 1, 'measured_passes': 5,
        'hardware': old_identity['hardware'],
        'hardware_source': 'Same machine as the baseline; hardware identification retained from its recorded sysctl output.',
        'commands': {e: str(p) for e, p in commands.items()}, 'binary_sha256': hashes,
        'before_source_snapshot': str(baseline / 'source-snapshot'),
        'before_source_manifest_sha256': digest(baseline / 'source-manifest.json'),
        'configuration': 'Original default limits; OCR disabled; one persistent process per version. Reading and Markdown rendering included; worker setup and output serialization excluded.',
    })
    workers = {e: Worker([str(p)], out / (e + '.log'), env) for e, p in commands.items()}
    runs = {e: [] for e in commands}
    try:
        for i in range(6):
            for e in (['before', 'after'] if i % 2 == 0 else ['after', 'before']):
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
    assert all(digest(p) == hashes[e] for e, p in commands.items())
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
                       'corpus_seconds': [sum(r['elapsed_ns'] for r in p) / 1e9 for p in samples],
                       'median_corpus_seconds': statistics.median(sum(r['elapsed_ns'] for r in p) / 1e9 for p in samples),
                       'median_document_ms': statistics.median(d['median_ms'] for d in documents),
                       'lifetime_peak_rss_bytes': int(peaks[-1]) if peaks else None}
    first = {e: {r['id']: r for r in passes[1]} for e, passes in runs.items()}
    recovered = [d['id'] for d in corpus if not first['before'][d['id']]['ok'] and first['after'][d['id']]['ok']]
    regressed = [d['id'] for d in corpus if first['before'][d['id']]['ok'] and not first['after'][d['id']]['ok']]
    common = [j for j, d in enumerate(corpus) if all(versions[e]['documents'][j]['returned'] == 5 for e in commands)]
    summary = {'versions': versions, 'recovered_documents': recovered, 'conversion_regressions': regressed,
               'previously_successful_outputs_changed': [d['id'] for d in corpus if first['before'][d['id']]['ok'] and first['before'][d['id']]['output_sha256'] != first['after'][d['id']]['output_sha256']],
               'common_successful_documents': len(common),
               'common_successful_median_document_ms': {e: statistics.median(versions[e]['documents'][j]['median_ms'] for j in common) for e in commands},
               'remaining_failures': [{'id': r['id'], 'format': r['format'], 'error': r.get('error')} for r in runs['after'][1] if not r['ok']],
               'verification': 'All five measured passes complete; source, executable and input hashes verified unchanged during this paired run. No quality score inferred from conversion success.'}
    write(out / 'summary.json', summary)
    print(json.dumps({k: v for k, v in summary.items() if k != 'versions'}, indent=2))


if __name__ == '__main__':
    main()
