"""Ensure the hard-case suite cannot silently drop or duplicate failed documents."""
import hashlib
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class HistoricalFailures(unittest.TestCase):
    def test_selection_includes_every_recorded_failure(self):
        original = ROOT/'reports/realworld-2026-11-12/conversion'
        files = sorted(original.glob('bulkparse-round-*.json'))
        if not files:
            self.skipTest('Historical raw runs are not distributed; current corpus membership is checked separately')
        self.assertTrue(files, 'Original failure evidence must remain available')
        failed = {r['id'] for p in files for r in json.loads(p.read_text())['documents'] if not r['ok']}
        suite = json.loads((ROOT/'benchmarks/realworld-failures/manifest.json').read_text())
        self.assertEqual({d['id'] for d in suite}, failed)
        self.assertEqual(len(suite), len({d['sha256'] for d in suite}))
        full = {d['id']: d for d in json.loads((ROOT/'reports/realworld-2026-11-12/corpus-manifest.json').read_text())}
        for doc in suite:
            self.assertEqual(doc['sha256'], full[doc['id']]['sha256'])
            for failure in doc['original_failures']:
                data = (ROOT/failure['record']).read_bytes()
                self.assertEqual(hashlib.sha256(data).hexdigest(), failure['record_sha256'])
                row = next(r for r in json.loads(data)['documents'] if r['id'] == doc['id'])
                self.assertFalse(row['ok'])

    def test_expected_failures_are_not_counted_as_success(self):
        expected = json.loads((ROOT/'benchmarks/realworld-failures/expected.json').read_text())
        default = expected['conservative_default']['documents']
        recovered = expected['conservative_bounded']['documents']
        failed = {key for key,row in default.items() if not row['ok']}
        self.assertEqual(len(failed), 15)
        self.assertEqual(sum(row['ok'] for row in default.values()), 12)
        for key in failed:
            self.assertEqual(default[key]['output_sha256'], hashlib.sha256(b'').hexdigest())
            self.assertTrue(recovered[key]['ok'])
            self.assertEqual(recovered[key]['status'], 'partial')
            self.assertTrue(any(d['code'] == 'resource_limit_partial' for d in recovered[key]['diagnostics']))


    def test_default_is_full_output_for_every_historical_failure(self):
        expected = json.loads((ROOT/'benchmarks/realworld-failures/expected.json').read_text())
        large = expected['stream']
        if not (ROOT/large['source']).exists():
            self.skipTest('Historical streaming run is not distributed; regenerate it to check its recorded hash')
        data = (ROOT/large['source']).read_bytes()
        self.assertEqual(hashlib.sha256(data).hexdigest(), large['source_sha256'])
        self.assertEqual(set(large['documents']), set(expected['default']['documents']))
        for row in large['documents'].values():
            self.assertTrue(row['ok'])
            self.assertNotEqual(row['output_sha256'], hashlib.sha256(b'').hexdigest())
            self.assertFalse(any(d['code'] == 'resource_limit_partial' for d in row['diagnostics']))

    def test_current_corpus_retains_all_historical_cases(self):
        suite = json.loads((ROOT/'benchmarks/realworld-failures/manifest.json').read_text())
        full = json.loads((ROOT/'reports/mixed-200-2026-11-13/corpus-manifest.json').read_text())
        current = json.loads((ROOT/'reports/biff-legacy-2026-11-13/diagnostic/after-0.json').read_text())
        inputs = {d['id']: d for d in full}
        results = {r['id']: r for r in current}
        self.assertEqual(len(suite), 27)
        self.assertEqual(len({d['sha256'] for d in suite}), len(suite))
        for doc in suite:
            self.assertEqual(doc['sha256'], inputs[doc['id']]['sha256'])
            self.assertTrue(results[doc['id']]['ok'])
            self.assertNotEqual(results[doc['id']]['markdown_sha256'], hashlib.sha256(b'').hexdigest())


if __name__ == '__main__':
    unittest.main()
