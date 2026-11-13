#!/usr/bin/env python3
"""Two additional structured-worker pairs to investigate the first RSS difference."""
import hashlib
import json
import os
import re
import statistics

from benchmark_realworld import ROOT, Worker, digest, write

OUT = ROOT / 'reports/xlsx-output-streaming-2026-11-13'


def main():
    out = OUT / 'structured-repetitions'
    out.mkdir(exist_ok=False)
    corpus = json.loads((OUT/'benchmark-final/corpus-manifest.json').read_text())
    identity = json.loads((OUT/'benchmark-final/identity.json').read_text())
    commands = {name: identity['commands'][name] for name in ['before', 'default']}
    assert all(digest(ROOT/d['path']) == d['sha256'] for d in corpus)
    assert all(digest(ROOT/name) == h for name,h in json.loads((OUT/'benchmark-final/source-manifest.json').read_text()).items())
    env = dict(os.environ, RAYON_NUM_THREADS='1', OMP_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1', MKL_NUM_THREADS='1')
    rows = []
    fields = ['ok', 'status', 'output_sha256', 'diagnostics', 'render_diagnostics', 'error_code', 'error']
    for repetition in [2, 3]:
        workers = {name: Worker(command, out/f'{name}-{repetition}.log', env) for name,command in commands.items()}
        passes = {name: [] for name in commands}
        try:
            for run in range(6):
                order = list(commands)
                if (run+repetition)%2: order.reverse()
                for name in order:
                    results = []
                    for doc in corpus:
                        r = workers[name].call(ROOT/doc['path'])
                        r['output_sha256'] = hashlib.sha256(r.pop('markdown').encode()).hexdigest()
                        r['id'] = doc['id']
                        results.append(r)
                    passes[name].append(results)
                assert all(all(x.get(k)==y.get(k) for k in fields) for x,y in zip(passes['before'][-1],passes['default'][-1]))
            print('pair', repetition, 'matched', flush=True)
        finally:
            for worker in workers.values(): worker.close()
        write(out/f'pair-{repetition}.json', passes)
        for name in commands:
            peak = re.search(r'(\d+)\s+maximum resident set size', (out/f'{name}-{repetition}.log').read_text())
            seconds = [sum(r['elapsed_ns'] for r in run)/1e9 for run in passes[name][1:]]
            rows.append(dict(version=name, repetition=repetition, corpus_seconds=seconds,
                             median_corpus_seconds=statistics.median(seconds), lifetime_peak_rss_bytes=int(peak[1])))
    assert all(digest(__import__('pathlib').Path(cmd[0])) == identity['binary_sha256'][name] for name,cmd in commands.items())
    write(out/'summary.json', dict(method=__doc__, script_sha256=digest(__import__('pathlib').Path(__file__)), results=rows, differences=[]))


if __name__ == '__main__':
    main()
