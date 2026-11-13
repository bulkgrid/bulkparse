#!/usr/bin/env python3
"""Before/after structured JSON and CLI-error equality, independent of Markdown checks."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

from benchmark_realworld import ROOT, REPORT, digest, write

OUT = ROOT / 'reports/xlsx-output-streaming-2026-11-13'


def main():
    docs = json.loads((REPORT / 'corpus-manifest.json').read_text())
    docs += json.loads((OUT / 'stress-manifest.json').read_text())
    binaries = {'before': OUT / 'before-bulkparse', 'after': ROOT / 'target/release/bulkparse'}
    hashes = {k: digest(p) for k,p in binaries.items()}
    env = dict(os.environ, RAYON_NUM_THREADS='1')
    rows = []
    for recovery in [False, True]:
        for doc in docs:
            path = ROOT / doc['path']
            assert digest(path) == doc['sha256']
            results = {}
            for name, binary in binaries.items():
                args = [str(binary), 'parse', str(path), '--to', 'json']
                if recovery: args.append('--partial-on-limit')
                r = subprocess.run(args, capture_output=True, env=env, timeout=90)
                assert r.returncode in [0, 1, 2], (name, doc['id'], r.returncode)
                results[name] = dict(exit_code=r.returncode, stdout_sha256=hashlib.sha256(r.stdout).hexdigest(),
                                     stderr_sha256=hashlib.sha256(r.stderr).hexdigest(), output_bytes=len(r.stdout))
            rows.append(dict(id=doc['id'], recovery=recovery, results=results, unchanged=results['before']==results['after']))
        print('recovery', recovery, 'checked', len(docs), flush=True)
    assert hashes == {k: digest(p) for k,p in binaries.items()}
    differences = [dict(id=r['id'], recovery=r['recovery']) for r in rows if not r['unchanged']]
    write(OUT / 'structured-json-checks.json', dict(method=__doc__, documents=len(docs), binary_sha256=hashes,
                                                   rows=rows, differences=differences))
    assert not differences, differences


if __name__ == '__main__':
    main()
