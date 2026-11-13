#!/usr/bin/env python3
"""Single-worker build control; diagnostic, not a replacement for the three-pair benchmark."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics

from benchmark_realworld import ROOT, REPORT, Worker, digest, write


def main():
    report = ROOT / 'reports/markdown-memory-2026-11-12'
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out', type=Path, default=report / 'baseline-build-control')
    out = ap.parse_args().out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = json.loads((report / 'before/source-manifest.json').read_text())
    assert all(digest(report / 'before/source-snapshot' / p) == h for p, h in source.items())
    corpus = json.loads((REPORT / 'corpus-manifest.json').read_text())
    assert all(digest(ROOT / d['path']) == d['sha256'] for d in corpus)
    binaries = {'preserved': report / 'before-office-bench',
                'rebuilt': ROOT / 'target/markdown-memory-baseline/release/examples/office_bench',
                'current': ROOT / 'target/release/examples/office_bench'}
    hashes = {k: digest(p) for k, p in binaries.items()}
    env = dict(os.environ, RAYON_NUM_THREADS='1', OMP_NUM_THREADS='1',
               OPENBLAS_NUM_THREADS='1', MKL_NUM_THREADS='1')
    workers = {k: Worker([str(p), '--stream-markdown'], out / (k + '.log'), env)
               for k, p in binaries.items()}
    runs = {k: [] for k in binaries}
    try:
        for index in range(6):
            order = list(binaries)
            order = order[index % 3:] + order[:index % 3]
            for name in order:
                rows = []
                for d in corpus:
                    row = workers[name].call(ROOT / d['path'])
                    row['output_sha256'] = hashlib.sha256(row.pop('markdown').encode()).hexdigest()
                    row['id'] = d['id']
                    rows.append(row)
                write(out / f'{name}-pass-{index}.json', rows)
                runs[name].append(rows)
                print(name, index, sum(r['elapsed_ns'] for r in rows) / 1e9, flush=True)
    finally:
        for w in workers.values():
            w.close()
    assert hashes == {k: digest(p) for k, p in binaries.items()}
    differences = []
    fields = ['ok', 'status', 'diagnostics', 'render_diagnostics', 'error', 'error_code', 'output_sha256']
    for index in range(6):
        for name in ['rebuilt', 'current']:
            for old, new in zip(runs['preserved'][index], runs[name][index]):
                changed = [k for k in fields if old.get(k) != new.get(k)]
                if changed:
                    differences.append(dict(index=index, name=name, id=old['id'], fields=changed))
    assert not differences, differences
    result = dict(method=__doc__, binary_sha256=hashes, differences=differences, versions={})
    for name, passes in runs.items():
        samples = [sum(r['elapsed_ns'] for r in rows)/1e9 for rows in passes[1:]]
        rss = re.search(r'(\d+)\s+maximum resident set size', (out / (name + '.log')).read_text())
        result['versions'][name] = dict(seconds=samples, median_seconds=statistics.median(samples),
                                        peak_mib=int(rss[1])/2**20)
    write(out / 'summary.json', result)
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
