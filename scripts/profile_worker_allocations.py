#!/usr/bin/env python3
"""Instrumented allocation diagnostics; not speed or RSS benchmark results."""
import argparse
import json
from pathlib import Path
import subprocess

from benchmark_realworld import ROOT, REPORT, digest, write


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--binary', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--passes', type=int, default=3)
    ap.add_argument('--structured', action='store_true', help='Profile the full structured model and Markdown instead of the writer')
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    corpus = json.loads((REPORT / 'corpus-manifest.json').read_text())
    assert all(digest(ROOT / d['path']) == d['sha256'] for d in corpus)
    binary = args.binary.resolve()
    sha = digest(binary)
    paths = ''.join(str(ROOT / d['path']) + '\n' for d in corpus) * args.passes
    with (args.out / 'documents.jsonl').open('w') as f:
        subprocess.run([str(binary)] + (['--structured'] if args.structured else []), input=paths, text=True, stdout=f, check=True)
    assert digest(binary) == sha
    rows = [json.loads(line) for line in (args.out / 'documents.jsonl').read_text().splitlines()]
    assert len(rows) == args.passes * len(corpus)
    for index, row in enumerate(rows):
        doc = corpus[index % len(corpus)]
        assert row['path'] == str(ROOT / doc['path'])
        row.update(id=doc['id'], run=index // len(corpus))
    summary = dict(method=__doc__, binary_sha256=sha, passes=args.passes, structured=args.structured,
                   scope='Rust requested live bytes/peaks and macOS allocator statistics. Realloc transient overlap and non-Rust allocations are not counted by the Rust counters. Allocator reserved bytes are not RSS. Includes diagnostic snapshots and benchmark-like Markdown readback/serialization; excludes OCR.',
                   rows=rows)
    if args.structured:
        summary['scope'] = 'Rust requested live bytes/peaks and macOS allocator statistics, including full model, Markdown and protocol-like serialization. Excludes transient realloc overlap, non-Rust allocations and OCR. Instrumented diagnostics, not speed or RSS measurements.'
    write(args.out / 'summary.json', summary)
    for run in range(args.passes):
        part = rows[run*len(corpus):(run+1)*len(corpus)]
        print(run, 'end live/reserved:', part[-1]['released']['live'], part[-1]['released']['system'],
              'largest parser peak:', max(r['parsed']['peak'] for r in part), flush=True)


if __name__ == '__main__':
    main()
