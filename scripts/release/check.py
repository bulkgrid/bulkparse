"""Validate release identities before building or contacting a registry."""
import json
import os
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def check():
    workspace = tomllib.loads((ROOT / 'Cargo.toml').read_text())
    version = workspace['workspace']['package']['version']
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('This release workflow accepts stable X.Y.Z versions only')
    for folder in ('node', 'wasm'):
        package = json.loads((ROOT / f'bindings/{folder}/package.json').read_text())
        assert package['version'] == version, f'{folder} version differs from Rust'
        assert package['name'] == ('@bulkgrid/bulkparse' if folder == 'node' else '@bulkgrid/bulkparse-wasm')
        assert not package.get('private'), f'{folder} cannot be published'
    for manifest in [*ROOT.glob('crates/*/Cargo.toml'), *ROOT.glob('bindings/*/Cargo.toml')]:
        if manifest.parent.parent.name not in ('crates', 'bindings'):
            continue
        data = tomllib.loads(manifest.read_text())
        assert data['package']['version'] == {'workspace': True}, str(manifest)
        if data['package'].get('publish') is not False:
            assert (manifest.parent / 'LICENSE').read_bytes() == (ROOT / 'LICENSE').read_bytes(), f'License out of sync: {manifest}'
        for spec in data.get('dependencies', {}).values():
            if isinstance(spec, dict) and 'path' in spec and data['package'].get('publish') is not False:
                assert spec.get('version') == version, f'Missing/mismatched dependency version: {manifest}'
    assert workspace['workspace']['dependencies']['bulkparse']['version'] == version
    for folder in ('bindings/python', 'crates/bulkparse-ocr-native'):
        assert (ROOT / folder / 'THIRD_PARTY_NOTICES.md').read_bytes() == (ROOT / 'THIRD_PARTY_NOTICES.md').read_bytes(), f'Notices out of sync: {folder}'
    assert (ROOT / 'bindings/python/LICENSE').read_bytes() == (ROOT / 'LICENSE').read_bytes()
    if os.environ.get('GITHUB_REF_TYPE') == 'tag':
        assert os.environ['GITHUB_REF_NAME'] == f'v{version}', 'Tag and package versions differ'
    publishing = os.environ.get('RELEASE_PUBLISH') == 'true'
    if publishing:
        assert os.environ.get('GITHUB_REF_TYPE') == 'tag', 'Publishing requires a vX.Y.Z tag, not a branch'
        assert os.environ.get('GITHUB_REPOSITORY') == 'bulkgrid/bulkparse', 'Publishing is restricted to bulkgrid/bulkparse'
        assert os.environ.get('GITHUB_EVENT_NAME') in ('push', 'workflow_dispatch'), 'Unsupported publishing event'
    print(f'Validated release {version}')
    if os.environ.get('GITHUB_OUTPUT'):
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            output.write(f'version={version}\n')
            output.write(f'publish={str(publishing).lower()}\n')
    return version


if __name__ == '__main__':
    check()
