import sys
import tempfile
import unittest
from pathlib import Path
from benchmark_parsers import bigram_recall, run
from evaluate_fixtures import compare


class BenchmarkTests(unittest.TestCase):
    def test_order_metric_detects_column_interleaving(self):
        expected = 'left alpha left beta right gamma right delta'
        interleaved = 'left alpha right gamma left beta right delta'
        self.assertEqual(compare(interleaved, expected)['token_recall_proxy'], 1)
        self.assertLess(bigram_recall(interleaved, expected), 1)

    def test_bigram_multisets_are_not_full_reading_order_scores(self):
        self.assertEqual(bigram_recall('a b c b a b d', 'a b a b c b d'), 1)

    def test_timeout_and_exit_status_are_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'output.md'
            result = run([sys.executable, '-c', 'import time; time.sleep(10)'], path, .1)
            self.assertTrue(result['timeout'])
            self.assertLess(result['exit_code'], 0)
            result = run([sys.executable, '-c', 'import sys; print("partial"); sys.exit(2)'], path)
            self.assertEqual(result['exit_code'], 2)
            self.assertEqual(path.read_text().strip(), 'partial')
            self.assertGreater(result['peak_rss_kib'], 0)


if __name__ == '__main__': unittest.main()
