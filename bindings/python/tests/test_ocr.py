import tempfile
import unittest
from pathlib import Path
from bulkparse import parse_with_ocr, OcrParser, BulkParseError

EVIDENCE = {'page_count': 2, 'pages': [{'page': p, 'output': {'type': 'text', 'value': f'Frame {p}'}} for p in [1, 2]]}
IMAGE = b'II\x2a\0'

class OcrTests(unittest.TestCase):
    def test_portable_evidence(self):
        result = parse_with_ocr(IMAGE, EVIDENCE)
        self.assertEqual(result['document']['format'], 'image')
        self.assertEqual(len(result['coverage']), 2)
        self.assertIn('Frame 2', result['markdown'])
        result = parse_with_ocr(IMAGE, EVIDENCE, budget={'max_pages': 1})
        self.assertEqual(result['coverage'][1]['status'], 'partial')
        with self.assertRaises(BulkParseError):
            parse_with_ocr(IMAGE, EVIDENCE, budget={'max_output_bytes': 1})
        with self.assertRaises(BulkParseError):
            parse_with_ocr(IMAGE, EVIDENCE, strict=True, budget={'max_pages': 1})

    def test_native_configuration_and_close(self):
        with self.assertRaises(BulkParseError):
            OcrParser({'executable': 'python3'})
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory) / 'models.json'
            models.write_text('{}')
            with OcrParser({'models': str(models), 'onnxruntime_library': str(models)}) as parser:
                first = parser.parse(b'Reusable Rust parser', filename='input.txt')
                self.assertIn('Reusable Rust parser', first['markdown'])
                source = Path(directory) / 'input.txt'
                source.write_bytes(b'Reusable Rust parser')
                self.assertEqual(parser.parse_file(source)['markdown'], first['markdown'])
            with self.assertRaises(BulkParseError): parser.parse(IMAGE)

    def test_merged_evidence_and_invalid_spans(self):
        import json
        evidence = json.loads((Path(__file__).resolve().parents[3] / 'fixtures/ocr/evidence/merged-table.json').read_text())
        result = parse_with_ocr(IMAGE, evidence)
        self.assertEqual(result['document']['blocks'][0]['rows'][0][0]['col_span'], 2)
        self.assertIn('125', result['markdown'])
        evidence['pages'][0]['output']['value']['regions'][0]['content']['rows'][0][0]['row_span'] = 9
        with self.assertRaises(BulkParseError): parse_with_ocr(IMAGE, evidence)
