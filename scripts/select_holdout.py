#!/usr/bin/env python3
"""Select external inputs before observing parser outputs; download pinned bytes.

Uses a previously fetched GitHub recursive tree. Raw third-party documents and
references stay in target/ and are not redistributed with BulkParse.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
from hashlib import sha256
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SEED = 'bulkparse-independent-v1-2026-11-10'


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--tree', type=Path, required=True)
    args = p.parse_args()
    tree = json.loads(args.tree.read_text())
    assert not tree.get('truncated'), 'complete tree required'
    revision = tree['sha']
    required = ['tests/pdfs/scotus-transcript-p1.pdf',
                'tests/pdfs/nics-background-checks-2015-11.pdf',
                'tests/pdfs/nics-background-checks-2015-11-rotated.pdf',
                'tests/pdfs/federal-register-2020-17221.pdf']
    candidates = [r['path'] for r in tree['tree'] if r['type'] == 'blob'
                  and r['path'].endswith('.pdf') and r.get('size', 0) <= 2_000_000
                  and not any(s in r['path'] for s in
                              ['from-oss-fuzz', 'password', 'malformed', '/empty.pdf'])
                  and r['path'] not in required]
    selected = required + sorted(candidates, key=lambda s: sha256((SEED+s).encode()).hexdigest())[:8]
    assert len(selected) == 12
    spec = {'repository': 'jsvine/pdfplumber', 'revision': revision, 'seed': SEED,
            'selection': 'Four preselected transcript/table/rotation/columns cases plus eight seeded path-hash selections from PDFs <=2 MB, excluding known malformed/password/empty/fuzz paths.',
            'paths': selected, 'tree_sha256': sha256(args.tree.read_bytes()).hexdigest(),
            'reference_policy': 'Upstream saved full-page text for transcript; Apple PDFKit differential references for others. PDFKit text is silver, not human gold. No output-derived reference from evaluated engines.'}
    plan = ROOT/'benchmarks/holdout/selection.json'
    if plan.exists():
        assert json.loads(plan.read_text()) == spec, 'selection must not change after results'
    else:
        plan.write_text(json.dumps(spec, indent=2)+'\n')
    dest = ROOT/'target/independent-inputs/external'
    dest.mkdir(parents=True, exist_ok=True)
    jobs = selected + ['tests/comparisons/scotus-transcript-p1.txt', 'LICENSE.txt']
    def fetch(path):
        output = dest/path
        output.parent.mkdir(parents=True, exist_ok=True)
        url = f'https://raw.githubusercontent.com/jsvine/pdfplumber/{revision}/{path}'
        subprocess.run(['curl','-fsSL','--connect-timeout','10','--max-time','90',url,'-o',str(output)], check=True)
        return {'path':path,'url':url,'sha256':sha256(output.read_bytes()).hexdigest()}
    with ThreadPoolExecutor(max_workers=4) as pool:
        downloads = list(pool.map(fetch, jobs))
    previous = {json.loads(s)['source_sha256'] for s in
                (ROOT/'reports/positioned-pdf-2026-11-10/manifest.jsonl').read_text().splitlines()}
    assert not previous.intersection(r['sha256'] for r in downloads if r['path'].endswith('.pdf')), 'overlap with development inputs'
    (ROOT/'benchmarks/holdout/downloads.json').write_text(json.dumps(downloads,indent=2)+'\n')
    print(json.dumps(spec,indent=2))


if __name__ == '__main__': main()
