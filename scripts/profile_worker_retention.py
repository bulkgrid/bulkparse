#!/usr/bin/env python3
"""Diagnostic post-response RSS trace; subprocess sampling overhead is not benchmark time."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--binary', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--passes', type=int, default=2)
    ap.add_argument('--stream', action='store_true')
    ap.add_argument('--no-recovery', action='store_true')
    a = ap.parse_args()
    corpus = json.loads((ROOT / 'reports/realworld-2026-11-12/corpus-manifest.json').read_text())
    command = [str(a.binary.resolve())] + ([] if a.no_recovery else ['--partial-on-limit']) + (['--stream-markdown'] if a.stream else [])
    p = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
                         env=dict(os.environ, RAYON_NUM_THREADS='1'), cwd=ROOT)
    rows = []
    try:
        for run in range(a.passes):
            for doc in corpus:
                p.stdin.write(json.dumps({'documents': [str(ROOT / doc['path'])]}) + '\n')
                p.stdin.flush()
                reply = json.loads(p.stdout.readline())['documents'][0]
                rss = int(subprocess.check_output(['ps', '-o', 'rss=', '-p', str(p.pid)], text=True)) * 1024
                rows.append(dict(run=run, id=doc['id'], rss_bytes=rss, ok=reply['ok']))
    finally:
        p.stdin.close()
        p.wait(timeout=30)
    a.out.write_text(json.dumps(dict(method=__doc__, command=command,
                                    binary_sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(),
                                    rows=rows), indent=2) + '\n')
    previous = 0
    for row in rows:
        if row['rss_bytes'] > previous + 8 * 2**20:
            print(row['run'], row['id'], round(previous / 2**20, 1), '->', round(row['rss_bytes'] / 2**20, 1))
        previous = row['rss_bytes']


if __name__ == '__main__':
    main()
