"""Fail closed on missing/extra release files and record downloadable checksums."""
import hashlib
from pathlib import Path
import tarfile
import json
import zipfile
from check import ROOT, check

PLATFORMS = ('linux-x64-gnu', 'linux-arm64-gnu', 'darwin-x64', 'darwin-arm64', 'win32-x64-msvc')


def verify(directory, version):
    expected = {f'bulkgrid-bulkparse-{version}.tgz', f'bulkgrid-bulkparse-wasm-{version}.tgz', f'bulkparse-{version}.tar.gz'}
    expected.update(f'bulkgrid-bulkparse-{platform}-{version}.tgz' for platform in PLATFORMS)
    expected.update(f'bulkparse-{version}-{platform}.tar.gz' for platform in PLATFORMS)
    files = {file.name for file in directory.iterdir() if file.is_file() and file.name != 'SHA256SUMS'}
    wheels = {name for name in files if name.endswith('.whl')}
    assert len(wheels) == 5, f'Expected five Python wheels, got {len(wheels)}'
    for suffix in ('manylinux_2_28_x86_64', 'manylinux_2_28_aarch64', 'macosx_11_0_x86_64', 'macosx_11_0_arm64', 'win_amd64'):
        assert any(suffix in name for name in wheels), f'Missing wheel for {suffix}'
    assert files == expected | wheels, f'Unexpected/missing files: {files ^ (expected | wheels)}'
    for name in wheels:
        assert name.startswith(f'bulkparse-{version}-cp310-abi3-'), f'Unexpected wheel identity: {name}'
        with zipfile.ZipFile(directory / name) as wheel:
            assert any('/bulkparse/' in '/' + path or path.startswith('bulkparse/') for path in wheel.namelist())
            assert any(path.endswith('/licenses/LICENSE') for path in wheel.namelist()), 'Wheel has no license'
    for name in expected:
        with tarfile.open(directory / name) as archive:
            if name.endswith('.tgz'):
                package = json.load(archive.extractfile('package/package.json'))
                assert package['version'] == version
                expected_name = name.removesuffix(f'-{version}.tgz').replace('bulkgrid-', '@bulkgrid/', 1)
                assert package['name'] == expected_name, f'Incorrect npm identity: {name}'
                assert 'package/LICENSE' in archive.getnames(), f'No license in {name}'
    lines = [f'{hashlib.sha256((directory / name).read_bytes()).hexdigest()}  {name}' for name in sorted(files)]
    (directory / 'SHA256SUMS').write_text('\n'.join(lines) + '\n')
    print(f'Validated and checksummed {len(files)} release artifacts')


if __name__ == '__main__':
    verify(ROOT / 'dist/release', check())
