"""Publish already-tested artifacts. Existing versions are never overwritten."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import urllib.error
import urllib.request

from check import ROOT, check


def registry(url):
    try:
        request = urllib.request.Request(url, headers={'User-Agent': 'bulkparse-release'})
        with urllib.request.urlopen(request, timeout=60) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise


def publish_npm(directory):
    files = sorted(directory.glob('*.tgz'))
    if len(files) != 7:
        raise ValueError(f'Expected five native packages, Node and WASM; got {len(files)}')
    # Native packages must exist before the public wrapper can be installed.
    native, wrappers = [], []
    for file in files:
        with tarfile.open(file) as archive:
            package = json.load(archive.extractfile('package/package.json'))
        (wrappers if package['name'] in ('@bulkgrid/bulkparse', '@bulkgrid/bulkparse-wasm') else native).append((file, package))
    for file, package in native + wrappers:
        name, version = package['name'], package['version']
        existing = registry(f'https://registry.npmjs.org/{name.replace("/", "%2f")}/{version}')
        if existing:
            integrity = 'sha512-' + base64.b64encode(hashlib.sha512(file.read_bytes()).digest()).decode()
            if existing['dist'].get('integrity') != integrity:
                raise ValueError(f'{name}@{version} exists with different bytes; do not reuse a released version')
            print(f'{name}@{version} already published with identical contents', flush=True)
            continue
        subprocess.run(['npm', 'publish', str(file.resolve()), '--access', 'public'], check=True)


def publish_crates():
    version = check()
    for name in ('bulkparse', 'bulkparse-ocr', 'bulkparse-ocr-native', 'bulkparse-pool', 'bulkparse-cli'):
        if registry(f'https://crates.io/api/v1/crates/{name}/{version}'):
            print(f'{name}@{version} already published; skipping immutable version', flush=True)
            continue
        subprocess.run(['cargo', 'publish', '--locked', '-p', name], cwd=ROOT, check=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('registry', choices=['npm', 'crates'])
    parser.add_argument('--artifacts', type=Path, default=ROOT / 'dist/release')
    args = parser.parse_args()
    if args.registry == 'npm':
        publish_npm(args.artifacts)
    else:
        publish_crates()
