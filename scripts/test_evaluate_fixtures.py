"""Tests the evaluator's process isolation and comparison caveats, not the parser."""
import argparse
from pathlib import Path
import tempfile
import unittest

from evaluate_fixtures import compare, run_cli


class EvaluationTests(unittest.TestCase):
    def test_timeout_is_not_reported_as_a_parser_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / 'slow'
            binary.write_text('#!/bin/sh\nif [ "$1" = "--help" ]; then printf "  parse  Parse a document\\n"; exit 0; fi\nsleep 20\n')
            binary.chmod(0o700)
            args = argparse.Namespace(cpu_seconds=2, output_mib=1, rss_mib=128, timeout=0.1)
            result = run_cli(binary, root / 'unused', root / 'json', args)
            self.assertEqual(result['harness_stop'], 'wall_timeout')
            self.assertLess(result['wall_ms_including_watchdog'], 3000)
            self.assertLess(result['exit_code'], 0)

    def test_partial_exit_and_output_are_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / 'partial'
            binary.write_text('#!/bin/sh\nif [ "$1" = "--help" ]; then printf "  parse  Parse a document\\n"; exit 0; fi\nprintf "partial output"\nexit 2\n')
            binary.chmod(0o700)
            args = argparse.Namespace(cpu_seconds=2, output_mib=1, rss_mib=128, timeout=2)
            result = run_cli(binary, root / 'unused', root / 'markdown', args)
            self.assertEqual(result['exit_code'], 2)
            self.assertIsNone(result['harness_stop'])
            self.assertEqual(Path(result['stdout_path']).read_text(), 'partial output')
            self.assertGreater(result['peak_rss_kib'], 0)

    def test_reference_errors_are_not_scored_as_missing_document_text(self):
        self.assertEqual(compare('', 'ERROR: all pages need OCR'), {'reference_is_error': True})

    def test_word_overlap_cannot_detect_table_structure_loss(self):
        result = compare('a;b\nx;y', '| a | b |\n|---|---|\n|x|y|')
        self.assertEqual(result['token_recall_proxy'], 1)
        self.assertFalse(result['normalized_exact'])


if __name__ == '__main__':
    unittest.main()
