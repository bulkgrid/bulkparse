"""Run with the recorded Docling Python environment after timed conversions."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]/'scripts'))
from check_realworld_csv_content import audit, reference


class SourceContentAudit(unittest.TestCase):
    def test_duplicate_rows_require_duplicate_output(self):
        result = audit('| A | B |\n| --- | --- |\n| x | y |\n', [('A','B'), ('x','y'), ('x','y')])
        self.assertTrue(result['auditable'])
        self.assertEqual(result['matched_source_rows'], 2)
        self.assertEqual(result['missing_source_rows'], 1)

    def test_escaping_and_html_match_visible_cells(self):
        result = audit('<table><tr><th>A</th><th>B</th></tr><tr><td><b>東京</b></td><td>x &amp; y</td></tr></table>', [('A','B'), ('東京','x & y')])
        self.assertEqual(result['matched_source_rows'], 2)
        self.assertEqual(result['missing_source_rows'], 0)

    def test_non_table_text_is_unscored(self):
        result = audit('A B\nx y', [('A','B'), ('x','y')])
        self.assertFalse(result['auditable'])
        self.assertIsNone(result['matched_source_rows'])

    def test_inline_boundaries_do_not_insert_spaces(self):
        result = audit('<table><tr><td><b>A</b>B<br>C</td><td><p>x</p><p>y</p></td></tr></table>', [('AB C','x y')])
        self.assertEqual(result['matched_source_rows'], 1)

    def test_nested_table_rows_are_not_counted_twice(self):
        result = audit('<table><tr><td><table><tr><td>x</td><td>y</td></tr></table></td></tr></table>', [('x','y'), ('x','y')])
        self.assertEqual(result['matched_source_rows'], 1)
        self.assertEqual(result['missing_source_rows'], 1)

    def test_utf16_tabs_and_quoted_newlines(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)/'source.csv'
            path.write_bytes('A\tB\r\nx\t"line one\nline two"\r\ny\tz\r\n'.encode('utf-16'))
            rows, metadata = reference(path)
            self.assertEqual(metadata['encoding'], 'utf-16')
            self.assertEqual(metadata['delimiter'], '\t')
            self.assertEqual(rows, [('A','B'), ('x','line one line two'), ('y','z')])

    def test_legacy_encoding_is_not_silently_ground_truth(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)/'source.csv'
            path.write_bytes(b'A,B\nx,\x93quoted\x94\ny,z\n')
            _, metadata = reference(path)
            self.assertFalse(metadata['encoding_verified'])


if __name__ == '__main__':
    unittest.main()
