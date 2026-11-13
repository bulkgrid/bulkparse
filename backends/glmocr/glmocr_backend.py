#!/usr/bin/env python3
"""Optional PDFium -> GLM-OCR SDK server -> BulkParse semantic-region adapter.

Only explicitly routed pages leave this process. No model downloads, implicit
cloud endpoints, credentials in config, or retries. The Rust parent enforces the
overall deadline and process lifetime. The server owns layout and inference.
"""
import argparse
import base64
from contextlib import redirect_stdout
from html.parser import HTMLParser
import io
import json
import math
import os
from pathlib import Path
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

MAX_REGIONS = 1024
MAX_CELLS = 16384


class BudgetError(ValueError):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("GLM endpoint redirects are not allowed")


def endpoint(value):
    url = urllib.parse.urlsplit(value)
    if url.scheme not in {"http", "https"} or not url.hostname or url.username or url.password or url.fragment:
        raise ValueError("configure an explicit HTTP(S) GLM-OCR SDK endpoint without credentials or fragments")
    return value


def integer(value, low, high, name):
    if type(value) is not int or not low <= value <= high:
        raise ValueError(f"invalid {name}")
    return value


class TableParser(HTMLParser):
    """Small non-executing HTML table reader; reject structure we cannot retain."""
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.rows, self.headers = [], []
        self.row = self.cell = None
        self.depth = self.count = 0
        self.closed = False
        self.in_head = False

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "table":
            if self.depth or self.closed:
                raise ValueError("nested or multiple OCR tables are unsupported")
            self.depth = 1
        elif tag == "thead":
            if self.depth != 1 or self.row is not None or self.in_head:
                raise ValueError("invalid OCR table header")
            self.in_head = True
        elif tag == "tr":
            if self.depth != 1 or self.row is not None:
                raise ValueError("invalid OCR table row")
            self.row, self.row_headers = [], []
        elif tag in {"td", "th"}:
            if self.row is None or self.cell is not None:
                raise ValueError("invalid OCR table cell")
            def span(name):
                value = attrs.get(name, "1")
                if not isinstance(value, str) or not value.isascii() or not value.isdigit():
                    raise ValueError("invalid OCR table span")
                return integer(int(value), 1, 256, "OCR table span")
            self.cell = {"text": "", "col_span": span("colspan"), "row_span": span("rowspan")}
            self.row_headers.append(tag == "th" or self.in_head)
            self.count += self.cell["col_span"] * self.cell["row_span"]
            if self.count > MAX_CELLS:
                raise BudgetError("OCR table cell budget")
        elif tag == "br" and self.cell is not None:
            self.cell["text"] += "\n"
        elif tag not in {"thead", "tbody", "tfoot", "b", "strong", "i", "em", "span", "p", "sup", "sub", "br"}:
            raise ValueError("unsupported markup in OCR table")

    def handle_endtag(self, tag):
        if tag == "thead":
            if self.row is not None or not self.in_head:
                raise ValueError("invalid OCR table header end")
            self.in_head = False
        elif tag in {"td", "th"}:
            if self.cell is None:
                raise ValueError("unmatched OCR table cell")
            self.cell["text"] = self.cell["text"].strip()
            self.row.append(self.cell)
            self.cell = None
        elif tag == "tr":
            if self.row is None or self.cell is not None:
                raise ValueError("unclosed OCR table cell")
            self.rows.append(self.row)
            self.headers.append(bool(self.row_headers) and all(self.row_headers))
            self.row = None
            if len(self.rows) > 256:
                raise BudgetError("OCR table row budget")
        elif tag == "table":
            if self.depth != 1 or self.row is not None or self.in_head:
                raise ValueError("unclosed OCR table row")
            self.depth, self.closed = 0, True

    def handle_data(self, data):
        if self.cell is not None:
            self.cell["text"] += data
        elif data.strip():
            raise ValueError("text outside OCR table cells")


def unfence(text):
    # Remove a complete outer code fence only; never repair truncated markup.
    lines = text.strip().splitlines()
    if len(lines) >= 3 and lines[0] in {"```", "```html", "```markdown", "```latex"} and lines[-1] == "```":
        return "\n".join(lines[1:-1]).strip()
    return text.strip()


def table_content(text):
    text = unfence(text)
    if text.lower().startswith("<table"):
        parser = TableParser()
        parser.feed(text)
        parser.close()
        if not parser.closed or parser.depth or not parser.rows:
            raise ValueError("incomplete OCR HTML table")
        headers = 0
        for is_header in parser.headers:
            if not is_header:
                break
            headers += 1
        return {"type": "table", "rows": parser.rows, "header_rows": headers}
    # GFM tables have an explicit separator and no merged cells.
    def cells(line):
        line = line.strip()
        if line.startswith("|"):
            line = line[1:]
        if line.endswith("|") and not line.endswith(r"\|"):
            line = line[:-1]
        return [s.replace(r"\|", "|").strip() for s in re.split(r"(?<!\\)\|", line)]
    lines = text.splitlines()
    if len(lines) < 2 or "|" not in lines[0] or not all(re.fullmatch(r":?-{3,}:?", c) for c in cells(lines[1])):
        raise ValueError("expected an HTML or Markdown OCR table")
    raw = [cells(line) for i, line in enumerate(lines) if i != 1]
    if len(raw) > 256 or len(raw[0]) > 256 or sum(map(len, raw)) > MAX_CELLS:
        raise BudgetError("OCR table cell budget")
    if any(len(row) != len(raw[0]) for row in raw) or len(cells(lines[1])) != len(raw[0]):
        raise ValueError("ragged OCR Markdown table")
    return {"type": "table", "header_rows": 1,
            "rows": [[{"text": v, "col_span": 1, "row_span": 1} for v in row] for row in raw]}


def normalize_result(result, image_size, limit):
    """Official SDK json_result: page lists, reading-order index, 0..1000 boxes."""
    if not isinstance(result, dict) or result.get("error"):
        raise ValueError("GLM service returned an error")
    pages = result.get("json_result")
    if not isinstance(pages, list) or len(pages) != 1 or not isinstance(pages[0], list):
        raise ValueError("expected exactly one GLM json_result page")
    if len(pages[0]) > MAX_REGIONS:
        raise BudgetError("OCR region budget")
    regions, orders, total = [], set(), 0
    for item in pages[0]:
        if not isinstance(item, dict):
            raise ValueError("invalid GLM region")
        order = integer(item.get("index"), 0, 2**32 - 1, "region order")
        if order in orders:
            raise ValueError("duplicate GLM region order")
        orders.add(order)
        label, text, box = item.get("label"), item.get("content"), item.get("bbox_2d")
        if not isinstance(label, str) or not label or len(label.encode()) > 64 or any(ord(c) < 32 for c in label):
            raise ValueError("invalid GLM label")
        if not isinstance(text, str):
            raise ValueError("invalid GLM region content")
        if not isinstance(box, list) or len(box) != 4 or any(type(v) not in (int, float) or not math.isfinite(v) or not 0 <= v <= 1000 for v in box):
            raise ValueError("invalid GLM normalized box")
        if box[0] >= box[2] or box[1] >= box[3]:
            raise ValueError("degenerate GLM region box")
        total += len(text.encode()) + 1
        if total > limit:
            raise BudgetError("OCR output byte budget")
        if label == "table":
            content = table_content(text)
        elif label in {"formula", "equation", "display_formula"}:
            tex = unfence(text)
            for left, right in (("$$", "$$"), (r"\[", r"\]"), (r"\(", r"\)"), ("$", "$")):
                if tex.startswith(left) and tex.endswith(right) and len(tex) >= len(left) + len(right):
                    tex = tex[len(left):-len(right)].strip()
                    break
            if not tex:
                raise ValueError("empty GLM formula")
            content = {"type": "formula", "tex": tex}
        elif label in {"title", "doc_title", "paragraph_title"}:
            content = {"type": "heading", "text": text, "level": 1 if label in {"title", "doc_title"} else 2}
        elif label in {"figure", "image", "chart"}:
            content = {"type": "figure", "description": text}
        else:
            # Header/footer/caption/reference/unknown labels survive as evidence.
            content = {"type": "text", "text": text}
        # No confidence is published by the documented SDK contract.
        regions.append({"order": order, "label": label,
                        "bbox": [v * image_size[i % 2] / 1000 for i, v in enumerate(box)],
                        "content": content})
    return sorted(regions, key=lambda r: r["order"])


def recognize_image(png, image_size, settings, limit):
    data = json.dumps({"images": ["data:image/png;base64," + base64.b64encode(png).decode("ascii")]}).encode()
    headers = {"Content-Type": "application/json", "Accept": "application/json"}
    key_env = settings.get("api_key_env")
    if key_env:
        key = os.environ.get(key_env)
        if not key:
            raise ValueError("configured GLM API key environment variable is missing")
        headers["Authorization"] = "Bearer " + key
    request = urllib.request.Request(endpoint(settings["endpoint"]), data=data, headers=headers, method="POST")
    opener = urllib.request.build_opener(NoRedirect())
    try:
        with opener.open(request, timeout=settings["request_timeout_seconds"]) as response:
            raw = response.read(settings["max_response_bytes"] + 1)
    except urllib.error.HTTPError as error:
        raise ValueError(f"GLM service HTTP {error.code}") from None
    except (urllib.error.URLError, TimeoutError, OSError):
        raise ValueError("GLM service unavailable or timed out") from None
    if len(raw) > settings["max_response_bytes"]:
        raise BudgetError("GLM response byte budget")
    return normalize_result(json.loads(raw), image_size, limit)


def render_page(path, page_number, dpi, max_pixels):
    import pypdfium2 as pdfium
    doc = pdfium.PdfDocument(str(path))
    try:
        if not 1 <= page_number <= len(doc):
            raise ValueError("page outside document")
        page = doc[page_number - 1]
        try:
            width, height = page.get_size()
            if not all(math.isfinite(v) and v > 0 for v in (width, height)):
                raise ValueError("invalid PDF page dimensions")
            scale = min(dpi / 72, math.sqrt(max_pixels / (width * height)))
            # Account for renderer rounding before allocation.
            while math.ceil(width * scale) * math.ceil(height * scale) > max_pixels:
                scale *= 0.99
            bitmap = page.render(scale=scale, rev_byteorder=True, limit_image_cache=True)
            try:
                if bitmap.width * bitmap.height > max_pixels:
                    raise BudgetError("render pixel budget")
                image = bitmap.to_pil()
                try:
                    data = io.BytesIO()
                    image.save(data, format="PNG")
                    return data.getvalue(), [bitmap.width, bitmap.height]
                finally:
                    image.close()
            finally:
                bitmap.close()
        finally:
            page.close()
    finally:
        doc.close()


def load_settings(path):
    if path.stat().st_size > 16384:
        raise ValueError("GLM settings too large")
    settings = json.loads(path.read_text())
    if settings.get("version") != 1 or set(settings) - {"version", "endpoint", "api_key_env", "request_timeout_seconds", "max_response_bytes"}:
        raise ValueError("invalid GLM settings")
    endpoint(settings["endpoint"])
    integer(settings["request_timeout_seconds"], 1, 240, "request timeout")
    integer(settings["max_response_bytes"], 1024, 16 * 1024 * 1024, "response budget")
    if settings.get("api_key_env") and not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", settings["api_key_env"]):
        raise ValueError("invalid credential environment variable name")
    return settings


def run_page(args, settings):
    if args.input.stat().st_size > args.max_input_bytes:
        raise BudgetError("PDF input byte budget")
    png, size = render_page(args.input, args.page, args.dpi, args.max_pixels)
    regions = recognize_image(png, size, settings, args.max_output_bytes)
    return {"version": 3, "page": args.page, "image_size": size, "regions": regions}


def reply(args, settings):
    try:
        # Native dependencies must never pollute the JSON protocol.
        with redirect_stdout(sys.stderr):
            return run_page(args, settings)
    except Exception as error:
        code = "resource_limit" if isinstance(error, BudgetError) else "backend_error"
        # Do not echo provider bodies, document content, URLs or credentials.
        return {"version": 3, "page": args.page, "error": {"code": code, "message": type(error).__name__ + ": GLM page processing failed"}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", type=Path, required=True, help="explicit endpoint settings, not local weights")
    parser.add_argument("--input", type=Path)
    parser.add_argument("--page", type=int)
    parser.add_argument("--dpi", type=int, default=200)
    parser.add_argument("--max-pixels", type=int, default=8_000_000)
    parser.add_argument("--max-input-bytes", type=int, default=32 * 1024 * 1024)
    parser.add_argument("--max-output-bytes", type=int, default=1024 * 1024)
    parser.add_argument("--worker", action="store_true")
    args = parser.parse_args()
    settings = load_settings(args.models)
    integer(args.dpi, 72, 600, "DPI")
    integer(args.max_pixels, 1, 100_000_000, "pixel budget")
    if args.worker:
        while True:
            raw = sys.stdin.buffer.readline(16385)
            if not raw:
                break
            if len(raw) > 16384 or not raw.endswith(b"\n"):
                raise ValueError("invalid worker frame")
            request = json.loads(raw)
            if request.get("version") != 1:
                raise ValueError("invalid worker version")
            args.input = Path(request["input"])
            args.page = integer(request["page"], 1, 2**32 - 1, "page")
            args.max_output_bytes = integer(request["max_output_bytes"], 1, 16 * 1024 * 1024, "text budget")
            print(json.dumps({"id": request["id"], "response": reply(args, settings)}, allow_nan=False), flush=True)
    else:
        if args.input is None or args.page is None:
            parser.error("--input and --page are required without --worker")
        print(json.dumps(reply(args, settings), allow_nan=False))


if __name__ == "__main__":
    main()
