"""Exercise publication gates with missing, mislabeled and complete artifacts."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('artifacts', Path(__file__).with_name('check-artifacts.py'))
artifacts = importlib.util.module_from_spec(spec)
spec.loader.exec_module(artifacts)


def archive(file, contents):
    with tarfile.open(file, 'w:gz') as tar:
        for name, content in contents.items():
            data = content.encode()
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.version = '0.1.0'
        for name in ('bulkparse', 'bulkparse-wasm', *(f'bulkparse-{key}' for key in artifacts.PLATFORMS)):
            archive(self.directory / f'bulkgrid-{name}-{self.version}.tgz', {
                'package/package.json': json.dumps({'name': f'@bulkgrid/{name}', 'version': self.version}),
                'package/LICENSE': 'test license',
            })
        for key in artifacts.PLATFORMS:
            archive(self.directory / f'bulkparse-{self.version}-{key}.tar.gz', {'bulkparse': 'test binary'})
        archive(self.directory / f'bulkparse-{self.version}.tar.gz', {'PKG-INFO': 'test metadata'})
        for platform in ('manylinux_2_28_x86_64', 'manylinux_2_28_aarch64', 'macosx_11_0_x86_64', 'macosx_11_0_arm64', 'win_amd64'):
            with zipfile.ZipFile(self.directory / f'bulkparse-{self.version}-cp310-abi3-{platform}.whl', 'w') as wheel:
                wheel.writestr('bulkparse/__init__.py', '')
                wheel.writestr(f'bulkparse-{self.version}.dist-info/licenses/LICENSE', 'test license')

    def test_complete_set_has_reproducible_checksums(self):
        artifacts.verify(self.directory, self.version)
        checksums = (self.directory / 'SHA256SUMS').read_text()
        self.assertEqual(len(checksums.splitlines()), 18)
        for line in checksums.splitlines():
            digest, name = line.split('  ')
            self.assertEqual(digest, hashlib.sha256((self.directory / name).read_bytes()).hexdigest())
        artifacts.verify(self.directory, self.version)
        self.assertEqual(checksums, (self.directory / 'SHA256SUMS').read_text())

    def test_missing_platform_cannot_publish(self):
        next(self.directory.glob('*win_amd64.whl')).unlink()
        with self.assertRaisesRegex(AssertionError, 'five Python wheels'):
            artifacts.verify(self.directory, self.version)

    def test_mislabeled_npm_package_cannot_publish(self):
        archive(self.directory / f'bulkgrid-bulkparse-{self.version}.tgz', {
            'package/package.json': json.dumps({'name': '@bulkgrid/bulkparse-wasm', 'version': self.version}),
            'package/LICENSE': 'test license',
        })
        with self.assertRaisesRegex(AssertionError, 'Incorrect npm identity'):
            artifacts.verify(self.directory, self.version)

    def test_extra_file_cannot_leak_into_release(self):
        (self.directory / 'debug.log').write_text('not a release artifact')
        with self.assertRaisesRegex(AssertionError, 'Unexpected/missing files'):
            artifacts.verify(self.directory, self.version)

    def test_tag_version_must_match_manifests(self):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('check.py'))],
                                env={**os.environ, 'GITHUB_REF_TYPE': 'tag', 'GITHUB_REF_NAME': 'v999.0.0'},
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Tag and package versions differ', result.stderr)

    def release_gate(self, **overrides):
        output = self.directory / 'github-output'
        version = json.loads((artifacts.ROOT / 'bindings/node/package.json').read_text())['version']
        env = {**os.environ, 'GITHUB_REF_TYPE': 'tag', 'GITHUB_REF_NAME': f'v{version}',
               'GITHUB_REPOSITORY': 'bulkgrid/bulkparse', 'GITHUB_EVENT_NAME': 'workflow_dispatch',
               'RELEASE_PUBLISH': 'true', 'GITHUB_OUTPUT': str(output), **overrides}
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('check.py'))],
                                env=env, capture_output=True, text=True)
        return result, output.read_text() if output.exists() else ''

    def test_manual_publication_requires_tag(self):
        result, output = self.release_gate(GITHUB_REF_TYPE='branch', GITHUB_REF_NAME='main')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Publishing requires a vX.Y.Z tag', result.stderr)
        self.assertNotIn('publish=true', output)

    def test_fork_cannot_publish(self):
        result, output = self.release_gate(GITHUB_REPOSITORY='another-owner/bulkparse')
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('publish=true', output)

    def test_manual_dry_run_never_enables_publishing(self):
        result, output = self.release_gate(RELEASE_PUBLISH='false', GITHUB_REF_TYPE='branch', GITHUB_REF_NAME='main')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('publish=false', output)

    def test_tagged_manual_release_enables_publishing(self):
        result, output = self.release_gate()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('publish=true', output)

    def test_tag_push_enables_publishing(self):
        result, output = self.release_gate(GITHUB_EVENT_NAME='push')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('publish=true', output)


if __name__ == '__main__':
    unittest.main()
