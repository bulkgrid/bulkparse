import unittest
from pathlib import Path
from bulkparse import parse, parse_file, capabilities, inspect_pdf, BulkParseError


class ConversionTests(unittest.TestCase):
    def test_image_assets_and_file_export(self):
        import base64
        import tempfile
        from bulkparse import parse_file_to_markdown
        raw = br"{\rtf1 Text {\pict\pngblip 89504e470d0a1a0a}}"
        result = parse(raw, filename="image.rtf")
        original = bytes.fromhex("89504e470d0a1a0a")
        self.assertEqual(base64.b64decode(result["document"]["assets"][0]["data"]), original)
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            input = folder / "input.rtf"
            output = folder / "output.md"
            input.write_bytes(raw)
            summary = parse_file_to_markdown(input, output, export_images=True)
            self.assertEqual((folder / summary["images"][0]["path"]).read_bytes(), original)
            self.assertNotIn(summary["images"][0]["path"], output.read_text())
            self.assertEqual(output.read_text(), parse_file(input)["markdown"])

    def test_contract_and_file(self):
        result = parse(b"# Hello BulkParse", filename="x.md")
        self.assertEqual(result["document"]["blocks"][0]["type"], "heading")
        self.assertEqual(result["status"], "complete")
        root = Path(__file__).resolve().parents[3]
        self.assertIn("# Hello BulkParse", parse_file(root / "fixtures/hello.docx")["markdown"])
        self.assertEqual(len(capabilities()), 18)

    def test_csv_options(self):
        result = parse("name;value\n名称;😀\n".encode("utf-16-le"), filename="x.csv", csv_encoding="utf16_le", csv_delimiter=";")
        self.assertEqual(len(result["document"]["blocks"][0]["rows"][0]), 2)
        self.assertIn("😀", result["markdown"])
        with self.assertRaises(BulkParseError) as error:
            parse(b"a,b;c\nx,y;z", filename="x.csv")
        self.assertEqual(error.exception.code, "malformed")

    def test_pdf_inspection(self):
        root = Path(__file__).resolve().parents[3]
        data = (root / "fixtures/inspection/header-over-scan.pdf").read_bytes()
        report = inspect_pdf(data)
        self.assertEqual(report["pages_needing_ocr"], [1])
        self.assertEqual(report["pages"][0]["ocr_status"], "not_requested")
        self.assertEqual(parse(data, filename="x.pdf")["pdf_inspection"], report)
        with self.assertRaises(BulkParseError) as error:
            inspect_pdf(data, limits={"max_nodes": 1})
        self.assertEqual(error.exception.code, "resource_limit")

    def test_errors(self):
        for options, code in [
            ({}, "unsupported"),
            ({"filename": "x.txt", "limits": {"max_input_bytes": 2}}, "resource_limit"),
            ({"filename": "x.txt", "strcit": True}, "malformed"),
        ]:
            with self.assertRaises(BulkParseError) as error:
                parse(b"abc", **options)
            self.assertEqual(error.exception.code, code)


if __name__ == "__main__":
    unittest.main()
