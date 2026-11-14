# Rust OCR provider

The CLI, Node binding and Python binding use this provider for native OCR. It does
not launch Python or download models. Rust handles decoding, preprocessing,
geometry and structure; ONNX Runtime executes models and PDFium renders PDFs.
Those two native libraries are not pure Rust. Sessions are reused and calls to one
provider are serialized.

## Setup

```sh
node scripts/provision_native_ocr.mjs --output /absolute/ocr
cargo build --locked --release -p bulkparse-cli --features ocr
target/release/bulkparse parse input.pdf --ocr-config /absolute/ocr/config.json
```

The pinned setup supports Linux x64/ARM64 and macOS ARM64. Other native platforms
can provision compatible libraries explicitly. Setup is a build/deployment step;
requests never download artifacts. SHA-256 identities and upstream URLs are
recorded in `sources.json`.

Configuration uses absolute paths:

```json
{
  "models": "/opt/bulkparse/models.json",
  "onnxruntime_library": "/opt/bulkparse/lib/libonnxruntime.so",
  "pdfium_library": "/opt/bulkparse/lib/libpdfium.so",
  "threads": 1,
  "dpi": 200,
  "max_pixels": 8000000,
  "document_timeout_ms": 120000
}
```

The manifest contains `models.det`, `models.rec` and optional `models.cls`, each
with `path` and `sha256`. Optional `dictionary` uses the same shape; otherwise
recognition vocabulary comes from the verified model's embedded metadata.
Optional `layout` contains `detector`, `table` and `dictionary` artifacts.
The supplied layout is PP-DocLayout-plus-L with SLANet-plus table structure.
Detection uses the PP-OCRv6 BGR normalization contract (mean/std 0.5), not
the older generic detector defaults. Unsupported model families need their own
preprocessing; matching ONNX tensor dimensions alone is insufficient.
The orientation adapter requires PP-LCNet-compatible preprocessing; the older
48×192 RapidOCR classifier is **not** interchangeable. Model hashes are verified.

PNG, JPEG, GIF, WebP, TIFF and PDF are supported, including image frames,
transparency and EXIF orientation. Ruled-grid reconstruction and model table
structure preserve merged cells where supported; invalid grids retain OCR lines.
Page/document deadlines are cooperative. Bulkgrid's service additionally enforces
a hard deadline using a supervised Node child around this native provider.

```sh
cargo test -p bulkparse-ocr-native
cargo build -p bulkparse-ocr-native --release --example parse
target/release/examples/parse /absolute/ocr/config.json input.png input.pdf
```

See [binding APIs](../../docs/ocr-bindings.md) and
[validation results](../../docs/ocr-bindings-validation.md). WASM exposes the
portable OCR evidence API, not this native inference runtime. Python remains a
supported caller language and a benchmark dependency; no Python worker is needed
by the Node service or CLI.
