#!/usr/bin/env python3
"""Verify a new mixed-corpus run without requiring deleted historical reports."""
import argparse
import json
from pathlib import Path

from benchmark_support import ROOT, REPORT, digest


def check(run):
    manifest = json.loads((run/'manifest.json').read_text())
    published = json.loads((REPORT/'corpus-manifest.json').read_text())
    assert [(r['id'], r['sha256']) for r in manifest] == [(r['id'], r['sha256']) for r in published], 'Not the frozen 200-document corpus'
    assert len(manifest) == len({r['id'] for r in manifest}) == len({r['sha256'] for r in manifest}) == 200
    assert all(digest(ROOT/r['path']) == r['sha256'] for r in manifest)
    hard = json.loads((ROOT/'benchmarks/realworld-failures/manifest.json').read_text())
    assert {r['id'] for r in hard} <= {r['id'] for r in manifest}
    identity = json.loads((run/'identity.json').read_text())
    assert not identity.get('smoke') and identity['measured_passes'] > 0
    files = []
    for engine in identity['commands']:
        for index in range(0 if identity['warmup'] else 1, identity['measured_passes']+1):
            path = run/f'{engine}-round-{index}.json'
            rows = json.loads(path.read_text())['documents']
            assert [r['id'] for r in rows] == [r['id'] for r in manifest], path
            folder = run/'prediction'/engine/f'round-{index}'
            assert {p.name for p in folder.glob('*.md')} == {r['id']+'.md' for r in manifest}
            for row in rows:
                output = folder/(row['id']+'.md')
                assert output.stat().st_size == row['output_bytes'] and digest(output) == row['output_sha256'], output
                assert row['elapsed_ns'] >= 0
                assert row['ok'] or row['output_bytes'] == 0, 'Failure retained stale output'
            files.append(path.name)
    assert json.loads((run/'verification.json').read_text())['complete']
    return {'documents':len(manifest), 'historical_cases':len(hard), 'round_files':len(files)}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run', type=Path)
    print(json.dumps(check(parser.parse_args().run), indent=2))
