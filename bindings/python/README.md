# BulkParse for Python

Requires Python 3.10+ and Rust. This package is not published yet. From this folder:

```sh
python3 -m venv .venv
source .venv/bin/activate
python -m pip install maturin
maturin develop --release
python -m unittest discover -s tests
```

On Windows activate the virtual environment using `.venv\Scripts\activate`.

```python
from bulkparse import parse, parse_file, BulkParseError

result = parse(b"# Hello", filename="hello.md")
print(result["markdown"])
document = parse_file("../../fixtures/hello.docx", strict=True)
print(document["document"]["blocks"])
```

Calls are synchronous and release the GIL during Rust conversion. Thread pools and queue limits belong to the application. Inputs are copied into Rust memory. `BulkParseError.code` carries stable error categories (`resource_limit`, `unsupported`, `malformed`, `incomplete`, etc.). The result is a dictionary with the same schema used by Node/WASM.

`capabilities()` reports enabled formats. Options mirror the Rust core; see the root README for extraction limitations and budgets. Native wheels and installation from PyPI are planned, not currently available.


CSV supports automatic comma/semicolon/tab/pipe detection and BOM-aware UTF-8/UTF-16 decoding. Without a BOM or explicit encoding, valid UTF-8 is preserved; otherwise legacy Windows-1252 decoding is attempted and reported with a `csv_encoding_inferred` diagnostic. Encoding inference is ambiguous, particularly for mixed-encoding files. Set `csv_encoding` to `utf8` for strict UTF-8, `windows1252` for a known legacy export, or `utf16_le` / `utf16_be` for BOM-less UTF-16. Explicit encodings must agree with any BOM. Undefined Windows-1252 bytes and binary controls are rejected. For delimiter ambiguity, pass `csv_delimiter` (one ASCII character); headers remain explicit through `csv_has_headers`.

PDFs support conservative ruled-table reconstruction. Arabic presentation forms and alphabetic ligatures expand by default. Pass `pdf_normalize_unicode: false` (Python: `pdf_normalize_unicode=False`) to retain mapped forms. `ActualText` is always preserved. See the root `docs/pdf-tables.md` for limits and validation.

PDF inspection is available without an OCR runtime: `inspect_pdf(pdf_bytes)`. Conversion results also include `pdf_inspection`. The shared output schema is now 0.8. See [inspection and routing](../../docs/pdf-inspection.md).

The standard build enables BulkParse-owned Office/EPUB readers. See [format support and limits](../../docs/office-formats.md).

For resource-limited spreadsheets and CSV files, opt in to `partial_on_limit` (Python: `partial_on_limit=True`). Successful normal conversions are unchanged; supported limit failures can return an explicitly partial prefix within the original per-attempt budgets. This is not complete extraction and strict mode never accepts the recovery result. Inspect `status` and `resource_limit_partial` diagnostics. See [bounded large-document handling](../../docs/large-documents.md). Rebuild the binding to use this option.

Embedded image bytes are available in `document.assets` (base64 JSON). See [image retention, export and limitations](../../docs/images.md).
