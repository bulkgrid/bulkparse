#!/usr/bin/env python3
"""Contract and real HTTP/process tests, using authored responses, never a model score."""
import base64
from contextlib import contextmanager
import importlib.util
import io
import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "backends/glmocr"))
import glmocr_backend as backend
import configure


def response():
    return {"json_result": [[
        {"index": 2, "label": "formula", "content": r"$$E=mc^2$$", "bbox_2d": [50, 700, 900, 800]},
        {"index": 0, "label": "title", "content": "Financial results", "bbox_2d": [50, 50, 900, 150]},
        {"index": 1, "label": "table", "content": "<table><tr><th>Quarter</th><th colspan='2'>Revenue</th></tr><tr><td rowspan='2'>Q1</td><td>10</td><td>20</td></tr><tr><td>30</td><td>40</td></tr></table>", "bbox_2d": [50, 200, 900, 650]},
    ]]}


@contextmanager
def service(mode="ok"):
    requests = []
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass
        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            requests.append({"path": self.path, "request": request})
            if mode == "redirect":
                self.send_response(307); self.send_header("Location", "/elsewhere"); self.end_headers(); return
            self.send_response(503 if mode == "error" else 200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            if mode == "oversize":
                self.wfile.write(b"x" * 4096)
            elif mode == "bad-json":
                self.wfile.write(b"not-json")
            else:
                self.wfile.write(json.dumps(response()).encode())
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/glmocr/parse", requests
    finally:
        server.shutdown(); server.server_close(); thread.join()


class ContractTests(unittest.TestCase):
    def test_transport_serializes_only_image_and_bounds_response_without_network(self):
        class Reply(io.BytesIO):
            pass
        class Opener:
            def open(inner, request, timeout):
                self.assertEqual(request.method, "POST")
                self.assertEqual(timeout, 2)
                self.assertEqual(json.loads(request.data), {"images": ["data:image/png;base64," + base64.b64encode(b"pixels").decode()]})
                return Reply(json.dumps(response()).encode())
        settings = {"endpoint": "http://localhost:5002/glmocr/parse", "request_timeout_seconds": 2, "max_response_bytes": 2048}
        with patch.object(backend.urllib.request, "build_opener", return_value=Opener()):
            self.assertEqual(len(backend.recognize_image(b"pixels", [600, 800], settings, 10000)), 3)
            settings["max_response_bytes"] = 10
            with self.assertRaises(backend.BudgetError): backend.recognize_image(b"pixels", [600, 800], settings, 10000)

    def test_normalization_retains_order_cells_math_and_pixel_boxes(self):
        regions = backend.normalize_result(response(), [600, 800], 10000)
        self.assertEqual([r["order"] for r in regions], [0, 1, 2])
        self.assertEqual(regions[0]["bbox"], [30, 40, 540, 120])
        self.assertNotIn("confidence", regions[0])
        self.assertEqual(regions[1]["content"]["rows"][0][1]["col_span"], 2)
        self.assertEqual(regions[1]["content"]["rows"][1][0]["row_span"], 2)
        self.assertEqual(regions[2]["content"], {"type": "formula", "tex": "E=mc^2"})

    def test_markdown_table_and_entities(self):
        table = backend.table_content("| Item | Value |\n| --- | ---: |\n| a \\| b | 12 |")
        self.assertEqual(table["rows"][1][0]["text"], "a | b")
        table = backend.table_content("| Item | Value |\n| --- | --- |\n| a ||")
        self.assertEqual(table["rows"][1][1]["text"], "")
        table = backend.table_content("<table><tr><td>&lt;script&gt; &amp; 5</td></tr></table>")
        self.assertEqual(table["rows"][0][0]["text"], "<script> & 5")
        table = backend.table_content("<table><thead><tr><td>Label</td></tr></thead><tbody><tr><td>Value</td></tr></tbody></table>")
        self.assertEqual(table["header_rows"], 1)

    def test_malformed_tables_do_not_become_plausible_cells(self):
        for text in ("not a table", "<table><tr><td>x", "<table><tr><td><table></table></td></tr></table>",
                     "<table><tr><td rowspan='999999'>x</td></tr></table>",
                     "| a | b |\n| --- | --- |\n| 1 |"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                backend.table_content(text)

    def test_bad_boxes_duplicate_orders_and_page_counts_are_rejected(self):
        for value in (None, [0, 0, 1001, 200], [0, 0, float("nan"), 200], [0, 0, 0, 100]):
            data = response(); data["json_result"][0][0]["bbox_2d"] = value
            with self.assertRaises(ValueError): backend.normalize_result(data, [600, 800], 10000)
        data = response(); data["json_result"][0][0]["index"] = 0
        with self.assertRaises(ValueError): backend.normalize_result(data, [600, 800], 10000)
        for data in ({"error": "failure"}, {"json_result": []}, {"json_result": [[], []]}):
            with self.assertRaises(ValueError): backend.normalize_result(data, [600, 800], 10000)

    def test_content_and_region_budgets(self):
        with self.assertRaises(backend.BudgetError): backend.normalize_result(response(), [600, 800], 10)
        data = {"json_result": [response()["json_result"][0] * 400]}
        with self.assertRaises(backend.BudgetError): backend.normalize_result(data, [600, 800], 100000)

    @unittest.skipIf(os.environ.get("BULKPARSE_SKIP_HTTP_TESTS"), "HTTP tests explicitly disabled: sandbox forbids socket binding")
    def test_real_http_request_and_failure_paths(self):
        for mode in ("ok", "error", "redirect", "bad-json", "oversize"):
            with self.subTest(mode=mode), service(mode) as (url, calls):
                settings = {"endpoint": url, "request_timeout_seconds": 2, "max_response_bytes": 2048}
                if mode == "ok":
                    self.assertEqual(len(backend.recognize_image(b"png-test", [600, 800], settings, 10000)), 3)
                else:
                    with self.assertRaises(ValueError): backend.recognize_image(b"png-test", [600, 800], settings, 10000)
                self.assertEqual(len(calls), 1)
                self.assertEqual(calls[0]["path"], "/glmocr/parse")
                self.assertEqual(base64.b64decode(calls[0]["request"]["images"][0].split(",")[1]), b"png-test")

    def test_configuration_is_explicit_and_never_overwrites(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = configure.configure(tmp, "http://127.0.0.1:5002/glmocr/parse")
            self.assertTrue(path.is_file())
            with self.assertRaises(ValueError): configure.configure(tmp, "http://localhost:5002/glmocr/parse")
        for url in ("file:///tmp/file", "https://key:secret@example.com/parse", "https://host/parse#fragment"):
            with self.assertRaises(ValueError): backend.endpoint(url)


@unittest.skipUnless(importlib.util.find_spec("pypdfium2") and importlib.util.find_spec("PIL"), "requires optional renderer")
class RenderingAndProcessTests(unittest.TestCase):
    @unittest.skipUnless((ROOT / "target/debug/examples/ocr_bench").exists(), "build ocr_bench for Rust/process replay")
    def test_rust_process_rendering_and_assembly_with_authored_response(self):
        for malformed in (False, True):
            with self.subTest(malformed=malformed), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                config_path = configure.configure(root, "http://127.0.0.1:5002/glmocr/parse")
                fixture = root / "response.json"
                data = response()
                if malformed:
                    data["json_result"][0][0]["bbox_2d"] = [-1, 20, 300, 400]
                fixture.write_text(json.dumps(data))
                config = json.loads(config_path.read_text())
                config["args"] = [str(ROOT / "crates/bulkparse-ocr/tests/replay_glm_backend.py"), "--replay-fixture", str(fixture)]
                config_path.write_text(json.dumps(config))
                inputs = [{"id": name, "path": str(ROOT / "fixtures/holdout" / name)} for name in ("prose.pdf", "native-and-scan.pdf")]
                run = subprocess.run([str(ROOT / "target/debug/examples/ocr_bench"), str(config_path)],
                                     input="".join(json.dumps(r) + "\n" for r in inputs), text=True, capture_output=True, timeout=25, check=True)
                results = [json.loads(line) for line in run.stdout.splitlines()]
                self.assertEqual(len(fixture.with_suffix(".calls").read_text().splitlines()), 1)
                scan = results[2]["result"]
                self.assertTrue(any(b["source"].get("page") == 1 for b in scan["document"]["blocks"]))
                blocks = [b for b in scan["document"]["blocks"] if b["source"].get("page") == 2]
                if malformed:
                    self.assertEqual(scan["pdf_inspection"]["pages"][1]["ocr_status"], "failed")
                    self.assertEqual(blocks, [])
                else:
                    self.assertEqual(scan["pdf_inspection"]["pages"][1]["ocr_status"], "applied")
                    self.assertEqual([b["type"] for b in blocks], ["heading", "table", "paragraph"])
                    self.assertEqual(blocks[-1]["inlines"][0], {"type": "math", "tex": "E=mc^2", "display": True})
                    self.assertEqual(blocks[1]["rows"][2][0]["source"]["cell"], 2)

    def test_rendering_respects_pixel_budget(self):
        from PIL import Image
        png, size = backend.render_page(ROOT / "fixtures/holdout/scan-only.pdf", 1, 200, 200000)
        self.assertLessEqual(size[0] * size[1], 200000)
        with Image.open(io.BytesIO(png)) as image:
            self.assertEqual(list(image.size), size)

    @unittest.skipIf(os.environ.get("BULKPARSE_SKIP_HTTP_TESTS"), "HTTP tests explicitly disabled: sandbox forbids socket binding")
    def test_python_worker_real_rendering_and_wire_envelope(self):
        with tempfile.TemporaryDirectory() as tmp, service() as (url, calls):
            config = json.loads(configure.configure(tmp, url).read_text())
            request = {"version": 1, "id": 7, "input": str(ROOT / "fixtures/holdout/scan-only.pdf"), "page": 1, "max_output_bytes": 10000}
            run = subprocess.run([sys.executable, *config["args"], "--models", config["models"], "--worker"],
                                 input=json.dumps(request) + "\n", text=True, capture_output=True, timeout=20, check=True)
            result = json.loads(run.stdout)
            self.assertEqual(result["id"], 7)
            self.assertEqual(result["response"]["version"], 3)
            self.assertEqual(len(result["response"]["regions"]), 3)
            self.assertEqual(len(calls), 1)
            self.assertTrue(base64.b64decode(calls[0]["request"]["images"][0].split(",")[1]).startswith(b"\x89PNG"))

    @unittest.skipUnless((ROOT / "target/debug/examples/ocr_bench").exists(), "build ocr_bench for full Rust-to-HTTP test")
    @unittest.skipIf(os.environ.get("BULKPARSE_SKIP_HTTP_TESTS"), "HTTP tests explicitly disabled: sandbox forbids socket binding")
    def test_rust_provider_to_http_and_native_bypass(self):
        with tempfile.TemporaryDirectory() as tmp, service() as (url, calls):
            config = configure.configure(tmp, url)
            inputs = [{"id": name, "path": str(ROOT / "fixtures/holdout" / name)} for name in ("prose.pdf", "native-and-scan.pdf")]
            run = subprocess.run([str(ROOT / "target/debug/examples/ocr_bench"), str(config)],
                                 input="".join(json.dumps(r) + "\n" for r in inputs), text=True, capture_output=True, timeout=25, check=True)
            results = [json.loads(line) for line in run.stdout.splitlines()]
            self.assertEqual(results[0], {"ready": True})
            self.assertEqual(len(calls), 1, "only the scanned page should reach the service")
            scan = results[2]["result"]
            self.assertEqual(scan["pdf_inspection"]["pages"][1]["ocr_status"], "applied")
            blocks = [b for b in scan["document"]["blocks"] if b["source"].get("page") == 2]
            self.assertEqual([b["type"] for b in blocks], ["heading", "table", "paragraph"])
            self.assertEqual(blocks[-1]["inlines"][0], {"type": "math", "tex": "E=mc^2", "display": True})
            self.assertEqual(blocks[1]["rows"][2][0]["source"]["cell"], 2)


if __name__ == "__main__":
    unittest.main()
