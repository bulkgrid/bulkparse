"""Behavior checks for benchmark accounting and the native worker protocol."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from benchmark_opendataloader import ROOT, validate_corpus


class CorpusValidation(unittest.TestCase):
    def test_incomplete_corpus_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(ValueError, "exactly 200"):
                validate_corpus(Path(temporary))

    def test_lfs_pointers_cannot_be_benchmarked_as_pdfs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "pdfs").mkdir()
            (root / "ground-truth/markdown").mkdir(parents=True)
            for i in range(200):
                (root / "pdfs" / f"{i:03}.pdf").write_bytes(b"version https://git-lfs.github.com/spec/v1\n")
                (root / "ground-truth/markdown" / f"{i:03}.md").write_text("reference")
            with self.assertRaisesRegex(ValueError, "possibly an LFS pointer"):
                validate_corpus(root)

    def test_failure_overwrites_previous_prediction_and_retains_document(self):
        binary = Path(os.environ.get("BULKPARSE_BENCH_WORKER", ROOT / "target/opendataloader-build/release/bulkparse-opendataloader-bench"))
        pdf = ROOT / "target/opendataloader-bench/pdfs/01030000000001.pdf"
        if not binary.is_file() or not pdf.is_file():
            self.skipTest("Build worker and download corpus first")
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "markdown"
            requests = [{"documents": [str(pdf)], "output_dir": str(output)},
                        {"documents": [str(Path(temporary) / pdf.name)], "output_dir": str(output)}]
            process = subprocess.run([str(binary)], input="".join(json.dumps(r) + "\n" for r in requests),
                                     capture_output=True, text=True, timeout=30, check=True)
            runs = [json.loads(line) for line in process.stdout.splitlines()]
            self.assertEqual(len(runs), 2)
            self.assertTrue(runs[0]["documents"][0]["ok"])
            self.assertEqual(len(runs[1]["documents"]), 1)
            failure = runs[1]["documents"][0]
            self.assertEqual(failure["document_id"], pdf.stem)
            self.assertFalse(failure["ok"])
            self.assertTrue(failure["error"])
            self.assertEqual((output / (pdf.stem + ".md")).read_text(), "")


if __name__ == "__main__":
    unittest.main()
