#!/usr/bin/env python3
"""Compare complete JSON output and CLI errors without buffering large outputs in RAM."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

from benchmark_support import ROOT, REPORT, digest, write


def signature(binary, path, recovery):
    command = [str(binary), 'parse', str(path), '--to', 'json']
    if recovery:
        command.append('--partial-on-limit')
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        result = subprocess.run(command, stdout=stdout, stderr=stderr,
                                env=dict(os.environ, RAYON_NUM_THREADS='1'), timeout=90)
        assert result.returncode in (0, 1, 2), (path, result.returncode)
        size = stdout.tell()
        stdout.seek(0)
        stderr.seek(0)
        return dict(exit_code=result.returncode, output_bytes=size,
                    stdout_sha256=hashlib.file_digest(stdout, 'sha256').hexdigest(),
                    stderr_sha256=hashlib.file_digest(stderr, 'sha256').hexdigest())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline-binary', type=Path, required=True, help='BulkParse CLI executable')
    parser.add_argument('--candidate-binary', type=Path, default=ROOT/'target/release/bulkparse')
    parser.add_argument('--manifest', type=Path, default=REPORT/'corpus-manifest.json')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--include-recovery', action='store_true')
    args = parser.parse_args()
    if args.out.exists():
        parser.error('Use a fresh output file')
    docs = json.loads(args.manifest.read_text())
    assert docs and len(docs) == len({d['id'] for d in docs})
    binaries = {'baseline':args.baseline_binary.resolve(), 'candidate':args.candidate_binary.resolve()}
    hashes = {name:digest(path) for name,path in binaries.items()}
    rows = []
    for recovery in ([False, True] if args.include_recovery else [False]):
        for doc in docs:
            path = ROOT/doc['path']
            assert digest(path) == doc['sha256']
            results = {name:signature(binary, path, recovery) for name,binary in binaries.items()}
            assert digest(path) == doc['sha256'], 'Input changed during check'
            rows.append(dict(id=doc['id'], recovery=recovery, results=results,
                             unchanged=results['baseline'] == results['candidate']))
    assert hashes == {name:digest(path) for name,path in binaries.items()}
    changes = [dict(id=r['id'], recovery=r['recovery']) for r in rows if not r['unchanged']]
    args.out.parent.mkdir(parents=True, exist_ok=True)
    write(args.out, dict(binary_sha256=hashes, manifest_sha256=digest(args.manifest),
                        documents=len(docs), rows=rows, differences=changes))
    assert not changes, changes
    print(f'{len(rows)} complete JSON/error comparisons matched')


if __name__ == '__main__':
    main()
