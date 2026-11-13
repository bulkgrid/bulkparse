#!/usr/bin/env python3
"""Explicitly download fingerprinted benchmark assets. Never used in production."""
import argparse
import hashlib
import json
import io
from pathlib import Path
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--data', type=Path, default=ROOT / 'target/ocr-alternatives/data')
    ap.add_argument('--paddle', action='store_true', help='Also provision official Paddle inference models')
    args = ap.parse_args()
    for asset in json.loads((ROOT / 'benchmarks/ocr-alternatives/assets.json').read_text()):
        path = args.data / asset['path']
        path.parent.mkdir(parents=True, exist_ok=True)
        if not path.exists():
            with urllib.request.urlopen(asset['url'], timeout=180) as response:
                blob = response.read()
            assert hashlib.sha256(blob).hexdigest() == asset['sha256'], asset['path']
            path.write_bytes(blob)
        assert hashlib.sha256(path.read_bytes()).hexdigest() == asset['sha256'], asset['path']
        print(asset['path'], 'verified', flush=True)
    if args.paddle:
        cache = ROOT / 'target/ocr-alternatives/paddle-cache'
        for model in json.loads((ROOT / 'benchmarks/ocr-alternatives/paddle-assets.json').read_text()):
            expected = model['files']
            if not all((cache / p).is_file() for p in expected):
                with urllib.request.urlopen(model['url'], timeout=180) as response:
                    blob = response.read()
                with tarfile.open(fileobj=io.BytesIO(blob)) as archive:
                    for path, digest in expected.items():
                        # Write only the manifest's known filenames; never extract archive paths or links.
                        members = [m for m in archive.getmembers() if m.isfile() and Path(m.name).name == Path(path).name]
                        assert len(members) == 1, path
                        content = archive.extractfile(members[0]).read()
                        assert hashlib.sha256(content).hexdigest() == digest, path
                        target = cache / path
                        target.parent.mkdir(parents=True, exist_ok=True)
                        target.write_bytes(content)
            for path, digest in expected.items():
                assert hashlib.sha256((cache / path).read_bytes()).hexdigest() == digest, path
            print(model['model'], 'verified', flush=True)

if __name__ == '__main__':
    main()
