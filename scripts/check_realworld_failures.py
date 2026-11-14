#!/usr/bin/env python3
"""Permanent real-world regression suite: all 27 original BulkParse failures."""
import argparse
import hashlib
import json
import os
from pathlib import Path

from benchmark_support import ROOT, DocumentWorker as Worker, digest, write

SUITE = ROOT / 'benchmarks/realworld-failures'
MODES = {
    'default': [], 'stream': ['--stream-markdown'],
    'conservative_default': ['--conservative-limits'],
    'conservative_bounded': ['--conservative-limits', '--partial-on-limit'],
    'conservative_stream': ['--conservative-limits', '--stream-markdown'],
    'conservative_stream_bounded': ['--conservative-limits', '--stream-markdown', '--partial-on-limit'],
}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--binary', type=Path, default=ROOT/'target/release/examples/office_bench')
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--modes', nargs='+', choices=list(MODES), default=list(MODES))
    ap.add_argument('--legacy-diagnostics', action='store_true', help='Compare schema 0.7 diagnostics with frozen pre-category expectations; ignore only category/evidence additions')
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    corpus = json.loads((SUITE/'manifest.json').read_text())
    expected = json.loads((SUITE/'expected.json').read_text())
    assert len(corpus) == len({d['sha256'] for d in corpus}) == 27
    for d in corpus:
        assert digest(ROOT/d['path']) == d['sha256'], d['id']
    binary = args.binary.resolve()
    binary_hash = digest(binary)
    rows = []
    for mode in args.modes:
        flags = MODES[mode]
        reference = expected[mode]
        worker = Worker([str(binary), *flags], args.out/(mode+'.log'), dict(os.environ, RAYON_NUM_THREADS='1'))
        try:
            for d in corpus:
                row = worker.call(ROOT/d['path'])
                row['output_sha256'] = hashlib.sha256(row.pop('markdown').encode()).hexdigest()
                want = reference['documents'][d['id']]
                compared = dict(row)
                if args.legacy_diagnostics:
                    for field in ['diagnostics', 'render_diagnostics']:
                        if field in compared:
                            compared[field] = [{k:v for k,v in d.items() if k not in ('category', 'evidence')} for d in compared[field]]
                changed = [k for k,v in want.items() if compared.get(k) != v]
                rows.append(dict(id=d['id'], mode=mode, ok=row['ok'], status=row.get('status'),
                                 changed=changed, output_sha256=row['output_sha256'], error=row.get('error')))
        finally:
            worker.close()
        print(mode, 'checked 27 documents', flush=True)
    assert digest(binary) == binary_hash
    differences = [r for r in rows if r['changed']]
    write(args.out/'results.json', dict(method=__doc__, manifest_sha256=digest(SUITE/'manifest.json'),
        expectations_sha256=digest(SUITE/'expected.json'), binary_sha256=binary_hash,
        documents=27, checks=len(rows), rows=rows, differences=differences,
        legacy_diagnostic_projection=args.legacy_diagnostics,
        interpretation='Expected failures are explicit regression expectations, not successful conversions. Intentional extraction improvements require reviewed expectation updates.'))
    assert not differences, differences
    print(f'All {len(rows)} outcome/content/diagnostic checks match the frozen expectations.')


if __name__ == '__main__':
    main()
