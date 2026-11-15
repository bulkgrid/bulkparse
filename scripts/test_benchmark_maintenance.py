"""Guard against silent corpus shrinkage and misleading regenerated reports."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import check_mixed200
from benchmark_regressions import differences
from report_mixed200 import render
from benchmark_support import commands


class MaintenanceTests(unittest.TestCase):
    def fixture(self, root):
        published = root/'published'
        published.mkdir()
        hard = root/'benchmarks/realworld-failures'
        hard.mkdir(parents=True)
        run = root/'run'
        run.mkdir()
        predictions = run/'prediction/bulkparse/round-1'
        predictions.mkdir(parents=True)
        manifest, rows = [], []
        for i in range(200):
            data = f'input {i}'.encode()
            path = root/f'{i}.txt'
            path.write_bytes(data)
            manifest.append(dict(id=str(i), path=str(path), sha256=hashlib.sha256(data).hexdigest(), format='txt'))
            text = b'output' if i else b''
            (predictions/f'{i}.md').write_bytes(text)
            rows.append(dict(id=str(i), format='txt', ok=bool(i), elapsed_ns=1_000_000,
                             output_bytes=len(text), output_sha256=hashlib.sha256(text).hexdigest()))
        for path in [published/'corpus-manifest.json', run/'manifest.json']:
            path.write_text(json.dumps(manifest))
        (hard/'manifest.json').write_text(json.dumps(manifest[:27]))
        (run/'identity.json').write_text(json.dumps(dict(commands={'bulkparse':['worker']}, warmup=0, measured_passes=1)))
        (run/'bulkparse-round-1.json').write_text(json.dumps({'documents':rows}))
        (run/'resources.json').write_text(json.dumps({'bulkparse':{'lifetime_peak_rss_bytes':2**20}}))
        (run/'verification.json').write_text(json.dumps({'complete':True}))
        return run, published

    def test_complete_run_and_report_keep_failure_in_denominator(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run, published = self.fixture(root)
            with patch.object(check_mixed200, 'ROOT', root), patch.object(check_mixed200, 'REPORT', published):
                self.assertEqual(check_mixed200.check(run)['documents'], 200)
                report = render(run)
                self.assertIn('199/200', report)
                self.assertIn('| txt | 99.5%<br>1.00 ms |', report)
                self.assertIn('Coverage is not quality', report)

    def test_reordered_rows_and_changed_prediction_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run, published = self.fixture(root)
            with patch.object(check_mixed200, 'ROOT', root), patch.object(check_mixed200, 'REPORT', published):
                path = run/'bulkparse-round-1.json'
                original = path.read_text()
                rows = json.loads(original)['documents']
                path.write_text(json.dumps({'documents':list(reversed(rows))}))
                with self.assertRaises(AssertionError):
                    check_mixed200.check(run)
                path.write_text(original)
                (run/'prediction/bulkparse/round-1/1.md').write_text('changed')
                with self.assertRaises(AssertionError):
                    check_mixed200.check(run)

    def test_output_and_diagnostics_changes_fail_but_timing_does_not(self):
        before = dict(ok=True, status='complete', output_sha256='same', diagnostics=[], elapsed_ns=10)
        after = dict(before, elapsed_ns=20)
        self.assertEqual(differences(before, after), [])
        after.update(output_sha256='different', diagnostics=[{'code':'missing'}])
        self.assertEqual(differences(before, after), ['output_sha256', 'diagnostics'])

    def test_structured_hash_changes_fail_but_serialization_timing_does_not(self):
        before = {'json_check': dict(bytes=10, sha256='a', valid=True, serialization_and_validation_ns=1)}
        after = {'json_check': dict(before['json_check'], serialization_and_validation_ns=2)}
        self.assertEqual(differences(before, after), [])
        after['json_check']['sha256'] = 'b'
        self.assertEqual(differences(before, after), ['json_check.sha256'])

    def test_format_selection_cannot_silently_omit_documents(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run, published = self.fixture(root)
            path = published/'corpus-manifest.json'
            manifest = json.loads(path.read_text())
            for row in manifest[100:]:
                row['format'] = 'md'
            path.write_text(json.dumps(manifest))
            (run/'manifest.json').write_text(json.dumps(manifest[:100]))
            path = run/'bulkparse-round-1.json'
            rows = json.loads(path.read_text())['documents'][:100]
            path.write_text(json.dumps({'documents': rows}))
            for i in range(100, 200):
                (run/f'prediction/bulkparse/round-1/{i}.md').unlink()
            path = run/'identity.json'
            identity = json.loads(path.read_text())
            identity['selected_formats'] = ['txt']
            path.write_text(json.dumps(identity))
            with patch.object(check_mixed200, 'ROOT', root), patch.object(check_mixed200, 'REPORT', published):
                self.assertEqual(check_mixed200.check(run)['documents'], 100)
                (run/'manifest.json').write_text(json.dumps(manifest[:99]))
                with self.assertRaises(AssertionError):
                    check_mixed200.check(run)

    def test_comparator_commands_use_current_adapter(self):
        for name, command in commands().items():
            if name not in ('bulkparse', 'anydoc'):
                self.assertTrue(command[1].endswith('/scripts/worker_mixed200.py'))


if __name__ == '__main__':
    unittest.main()
