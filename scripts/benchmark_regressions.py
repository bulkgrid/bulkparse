#!/usr/bin/env python3
"""Compare two office_bench builds on identical inputs, including streaming and JSON.

Run before changing the parser to preserve a baseline executable. Output changes
fail the check and require review; timings and memory are measurements, not gates.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics

from benchmark_support import ROOT, REPORT, DocumentWorker, digest, write

MODES = {'default': [], 'stream': ['--stream-markdown'], 'json': ['--verify-json']}
FIELDS = ('ok', 'status', 'output_sha256', 'diagnostics', 'render_diagnostics',
          'error_code', 'image_count', 'image_bytes', 'image_assets')


def differences(before, after):
    changed = [key for key in FIELDS if before.get(key) != after.get(key)]
    # JSON mode verifies the complete structured document, not only Markdown
    # and diagnostics. Serialization time is deliberately excluded.
    for key in ('bytes', 'sha256', 'valid'):
        if before.get('json_check', {}).get(key) != after.get('json_check', {}).get(key):
            changed.append(f'json_check.{key}')
    return changed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline-binary', type=Path, required=True)
    parser.add_argument('--candidate-binary', type=Path, default=ROOT/'target/release/examples/office_bench')
    parser.add_argument('--manifest', type=Path, default=REPORT/'corpus-manifest.json')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--modes', nargs='+', choices=MODES, default=list(MODES))
    parser.add_argument('--passes', type=int, default=1)
    parser.add_argument('--warmup', action='store_true')
    parser.add_argument('--reverse-order', action='store_true', help='Reverse the alternating build order; without warm-up this runs the baseline first')
    args = parser.parse_args()
    if args.passes < 1:
        parser.error('--passes must be positive')
    corpus = json.loads(args.manifest.read_text())
    assert corpus and len(corpus) == len({row['id'] for row in corpus})
    assert all(digest(ROOT/row['path']) == row['sha256'] for row in corpus)
    binaries = {'baseline': args.baseline_binary.resolve(), 'candidate': args.candidate_binary.resolve()}
    hashes = {name: digest(path) for name, path in binaries.items()}
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    write(out/'manifest.json', corpus)
    write(out/'identity.json', dict(binary_sha256=hashes, binaries={k:str(v) for k,v in binaries.items()},
          manifest_sha256=digest(args.manifest), modes=args.modes, passes=args.passes,
          warmup=int(args.warmup), reverse_order=args.reverse_order,
          environment={key: '1' for key in ('RAYON_NUM_THREADS', 'OMP_NUM_THREADS', 'OPENBLAS_NUM_THREADS', 'MKL_NUM_THREADS')},
          memory='macOS worker lifetime peak RSS, includes transport and warmup'))
    changed, summaries = [], {}
    for mode in args.modes:
        samples = {name: [] for name in binaries}
        workers = {name: DocumentWorker([str(binary), *MODES[mode]], out/f'{mode}-{name}.log',
                   dict(os.environ, RAYON_NUM_THREADS='1', OMP_NUM_THREADS='1',
                        OPENBLAS_NUM_THREADS='1', MKL_NUM_THREADS='1')) for name, binary in binaries.items()}
        try:
            for index in range(0 if args.warmup else 1, args.passes+1):
                runs = {}
                order = list(workers)
                if bool(index % 2) != args.reverse_order:
                    order.reverse()
                for name in order:
                    rows = []
                    for doc in corpus:
                        row = workers[name].call(ROOT/doc['path'])
                        assert row['path'] == str(ROOT/doc['path'])
                        row['output_sha256'] = hashlib.sha256(row.pop('markdown').encode()).hexdigest()
                        row.update(id=doc['id'], format=doc.get('format', 'xlsx'))
                        if mode == 'json' and row['ok']:
                            assert row.get('json_check', {}).get('valid'), (name, doc['id'], 'invalid JSON')
                        rows.append(row)
                    runs[name] = rows
                    write(out/f'{mode}-{name}-{index}.json', {'documents': rows})
                    if index:
                        samples[name].append(rows)
                for before, after in zip(runs['baseline'], runs['candidate']):
                    fields = differences(before, after)
                    if fields:
                        changed.append(dict(mode=mode, round=index, id=after['id'], fields=fields))
                print(f'{mode}: pass {index}, {len(corpus)} documents compared', flush=True)
        finally:
            for worker in workers.values():
                worker.close()
        summaries[mode] = {}
        for name, runs in samples.items():
            peaks = re.findall(r'(\d+)\s+maximum resident set size', (out/f'{mode}-{name}.log').read_text())
            summaries[mode][name] = dict(
                median_corpus_seconds=statistics.median(sum(r['elapsed_ns'] for r in run)/1e9 for run in runs),
                peak_rss_bytes=max(map(int, peaks)) if peaks else None,
                returned_each_pass=[sum(r['ok'] for r in run) for run in runs])
    assert hashes == {name:digest(path) for name,path in binaries.items()}
    assert all(digest(ROOT/row['path']) == row['sha256'] for row in corpus)
    write(out/'summary.json', dict(profiles=summaries, differences=changed,
          note='Equality checks detect changes, not correctness. Expected failures remain failures. Review intentional changes.'))
    assert not changed, f'{len(changed)} output changes; see {out}/summary.json'


if __name__ == '__main__':
    main()
