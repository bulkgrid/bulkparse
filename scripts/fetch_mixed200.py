#!/usr/bin/env python3
"""Restore hash-pinned public inputs. Changed downloads fail; nothing is substituted."""
import argparse
import json
from pathlib import Path
import subprocess

from benchmark_support import ROOT, REPORT, digest


def restore(manifest):
    docs = json.loads(manifest.read_text())
    for doc in docs:
        path = ROOT/doc['path']
        path.parent.mkdir(parents=True, exist_ok=True)
        if path.exists():
            assert digest(path) == doc['sha256'], f'Changed existing input: {path}'
            continue
        temporary = path.with_suffix('.download')
        try:
            subprocess.run(['curl', '-sS', '-L', '--fail', '--max-time', '90',
                            '--max-filesize', '15728640', doc['url'], '-o', str(temporary)], check=True)
            assert digest(temporary) == doc['sha256'], f'Source changed: {doc["url"]}'
            temporary.rename(path)
        finally:
            temporary.unlink(missing_ok=True)
        print(doc['id'], flush=True)
    print(f'All {len(docs)} input hashes verified')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=REPORT/'corpus-manifest.json')
    restore(parser.parse_args().manifest)
