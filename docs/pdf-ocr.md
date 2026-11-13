# Optional PDF OCR

Current structured extension: [semantic-region OCR](../backends/glmocr/README.md)
adds an optional GLM-OCR SDK service adapter and process protocol version 3.
Providers may return typed headings, tables, formulas and figure regions through
`PdfOcrOutput::Regions`. These become the existing table/math document structures
with optional `source.pdf_region` evidence. Document schema is now **0.7**.
The text/line interfaces below remain supported; their table and paragraph
limitations describe the RapidOCR path. No Firecrawl account is required. Live
GLM recognition quality remains unverified.

BulkParse's default PDF path stays local and uses native text. It does not install OCR engines, render page bitmaps, download models or contact a service. The Rust API accepts a caller-supplied `PdfOcrProvider`. A real optional [ProcessOcr backend](../backends/rapidocr/README.md) now connects PDFium, RapidOCR and ONNX Runtime through a supervised process; an OCR-enabled CLI can use it with `--ocr-config`. Node, Python and WASM expose page-level `pdf_ocr_needed` diagnostics; provider registration is currently a Rust API capability, not a JSON option or a binding callback.

```rust
use bulkparse::{CancellationToken, Error, Parser, PdfOcrBudget,
                PdfOcrProvider, PdfOcrRequest};

fn convert_with_provider(
    pdf: &[u8], provider: &dyn PdfOcrProvider,
) -> Result<bulkparse::ParseResult, Error> {
    Parser::default().parse_with_pdf_ocr(
        pdf, Some("upload.pdf"), &CancellationToken::default(), provider,
        PdfOcrBudget { max_pages: 4, max_output_bytes: 1_048_576 },
    )
}
```

A provider implements `recognize(&self, request: PdfOcrRequest<'_>) -> Result<String, Error>`. The request includes the original document bytes, a one-based page number, remaining output-byte budget, and cancellation token. It returns **plain text in logical Unicode order**. Rendering and inference belong to the provider; it can use a local engine or an explicitly configured service according to the host application's policy.

The built-in [PDF inspector](pdf-inspection.md) now selects pages from native evidence: images or vector content without native text, broken text layers, large images with sparse native text, and repeated text-like vector outlines with sparse native text. It reports page types and routing reasons in schema 0.3. Usable native pages and structurally blank pages avoid OCR. Mixed-content candidates currently use whole-page OCR; region-level fusion is not implemented. Available native text is retained when OCR fails or returns no usable text.

The page budget counts attempted calls, including failures. The byte budget is aggregate across accepted provider outputs. Over-budget output fails with `resource_limit`; provider errors otherwise become page diagnostics and preserve text already extracted on other pages. Cancellation is checked before and after callbacks. Strict mode continues to reject PDFs because OCR accuracy, layout and visual coverage are unverified.

The callback is synchronous and cannot be forcibly interrupted by the core. Providers must enforce their own rendering pixel limits, timeouts and memory budgets and cooperate with cancellation. Bulkgrid should supervise expensive providers in bounded workers/processes. The parser's checks do not constitute an OS resource sandbox.

The original interface tests use deterministic provider doubles for routing, budgets, failures and cancellation. The later [real-engine evaluation](ocr-evaluation.md) covers eight original cases and explicit failure/budget checks. Empty OCR output now produces `pdf_ocr_empty`, with no implication that the page is known to be blank. Production scan accuracy is still unestablished. The later [geometry and reading-order milestone](ocr-layout.md) fixes the authored column case while retaining the Swedish accent error.

## Structured provider output (schema 0.2)

Existing providers can keep implementing `recognize`. Its default adapter returns
`PdfOcrOutput::Text`; no geometry is fabricated. Providers that have line boxes can
override `recognize_page` to return `PdfOcrOutput::Lines { image_size, lines }`.
Each `PdfOcrLine` contains plain `text`, `quad: [[f64; 2]; 4]` and `confidence`.
The process provider implements both entry points; its legacy `recognize` returns
plain engine-order text, while the parser uses structured output and layout logic.

Coordinates are rendered-page pixels, with origin at the top left, x increasing
right and y increasing down. `image_size` is `[width, height]`; it is not a PDF
MediaBox or a point-space coordinate system. Vertices follow the quadrilateral
perimeter (the RapidOCR adapter supplies top-left, top-right, bottom-right,
bottom-left). The engine confidence is preserved in [0, 1]; it is not a calibrated
probability that a line is correct. Rust rejects nonfinite/out-of-bounds points,
nonconvex or degenerate polygons, invalid confidence and embedded line breaks.
There may be at most 4096 nonempty lines per page. The text budget includes one
newline between lines; geometry has a separately bounded transport allowance.

The parser emits one paragraph block per recognized line, in inferred order,
with `source.page` and `source.ocr` containing `image_size`, `quad`, `confidence`.
It does not yet infer paragraph grouping or table cells. The optional evidence is
boxed so ordinary source locations only carry an optional pointer. JSON schema,
Node and WASM type declarations include the field; Python uses the shared JSON
dictionary. `Source`, `Diagnostic` and `Coverage` retain `PartialEq` but no longer
implement `Eq` because evidence contains floating-point numbers.

The geometry milestone introduced document schema version `0.2`; the current inspection milestone uses `0.3`. Non-OCR fields and
content remain unchanged. Clients that require an exact schema version should
accept the new version before updating. Node/Python/WASM OCR invocation APIs are
still outside this milestone. Geometry-capable results remain `partial` and emit
`pdf_ocr_layout_inferred`; strict mode still rejects them.

## Reusing supervised model sessions

New bundled backend configurations enable `reuse_process` and recycle the worker
at `max_pages_per_worker` (128 by default). A `ProcessOcr` is safe to share across
calls: one request runs at a time, and waiting callers retain their own deadline
and cancellation checks. Reuse the provider across documents; constructing a new
provider for every page prevents reuse. Dropping it terminates its worker.
Failed requests discard the worker, allowing the next request to start fresh.

This remains a supervised Python/ONNX backend, not an in-process Rust inference
engine. The process boundary preserves hard timeout and cancellation enforcement.
Old JSON configurations default to one-shot execution. Model hashes are checked
on worker initialization; model replacements require provider restart. No document
or output cache is introduced. See the before/after measurements (historical artifact; not distributed).

Rust callers constructing `Config` with a struct literal must supply the new
`reuse_process` and `max_pages_per_worker` fields. JSON callers may omit them
for the compatible one-shot defaults.
