import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import subprocess
from evaluate_advanced_pdf import check_result, evaluate, sha256, tex_normalize


class ReferenceTests(unittest.TestCase):
    def test_formula_checks_require_math_structure(self):
        result = {"document": {"blocks": [{"type": "paragraph", "source": {"page": 1},
                   "inlines": [{"type": "text", "text": "E=mc^2"}]}]}}
        check = {"type": "formula", "tex": "E = mc^{2}"}
        self.assertFalse(check_result(result, check))
        result["document"]["blocks"][0]["inlines"] = [{"type": "math", "tex": "E=mc^2", "display": True}]
        self.assertTrue(check_result(result, check))
        self.assertNotEqual(tex_normalize("x^2"), tex_normalize("x^3"))

    def test_order_requires_every_anchor_in_sequence(self):
        result = {"document": {"blocks": [{"type": "paragraph", "source": {}, "inlines": [{"type": "text", "text": "right left end"}]}]}}
        self.assertFalse(check_result(result, {"type": "order", "anchors": ["left", "right"]}))
        self.assertTrue(check_result(result, {"type": "order", "anchors": ["right", "left", "end"]}))

    def test_failed_and_missing_inputs_remain_in_denominator(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            doc, config, driver = (root / name for name in ("input.pdf", "config.json", "driver"))
            for p in (doc, config, driver): p.write_text("test")
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({"version": 1, "cases": [
                {"id": name, "path": "input.pdf", "sha256": sha256(doc),
                 "checks": [{"type": "text", "contains": "expected"}]} for name in ("ok", "failed", "missing")]}))
            result = {"status": "partial", "diagnostics": [], "document": {"blocks": [
                {"type": "paragraph", "source": {}, "inlines": [{"type": "text", "text": "expected"}]}]}}
            stdout = json.dumps({"id": "ok", "result": result}) + "\n" + json.dumps({"id": "failed", "error": "failed"}) + "\n"
            with patch("evaluate_advanced_pdf.subprocess.run", return_value=subprocess.CompletedProcess([], 1, stdout, "driver failed")):
                report = evaluate(manifest, config, driver, root / "report")
            self.assertEqual((report["checks_passed"], report["checks_total"]), (1, 3))
            self.assertEqual(report["driver_exit"], 1)
            self.assertTrue(report["cases"][2]["error"])
            # A changed input is rejected before invoking any provider.
            doc.write_text("changed")
            with patch("evaluate_advanced_pdf.subprocess.run") as run:
                with self.assertRaises(ValueError): evaluate(manifest, config, driver, root / "report2")
                run.assert_not_called()


if __name__ == "__main__": unittest.main()
