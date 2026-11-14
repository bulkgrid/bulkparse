# Built-in PDF inspection and OCR routing

BulkParse inspects every PDF page using its native Rust content walker. Conversion loads the PDF once and uses the same decoded page evidence for inspection, layout reconstruction and selective OCR routing. The inspection-only API skips layout reconstruction, rendering and inference. It still parses the PDF and decodes native text; it is not a constant-time file-header check.

The design was informed by the supplied `legacy/pdf-inspector-main` source, particularly its separation of native extraction, page evidence, text quality and OCR routing. This is a BulkParse implementation on the existing `lopdf` dependency; it does not depend on the downloaded repository or copy its classifier, English-frequency heuristics or OCR runtime. No new dependencies or models were added.

## Use it

```sh
cargo run -p bulkparse-cli -- inspect document.pdf
# JSON from stdin; exit 0 when inspected, 2 if any page needs manual review,
# 1 on a document-level error. An OCR recommendation alone is not an error.
bulkparse inspect - < document.pdf

# The same inspection automatically controls the existing opt-in OCR path.
cargo run -p bulkparse-cli --features ocr -- parse document.pdf \
  --ocr-config target/ocr/profiles/multi-small/config.json --to json
```

```rust
let inspection = bulkparse::Parser::default().inspect_pdf(&bytes)?;
println!("OCR candidates: {:?}", inspection.pages_needing_ocr);
```

```js
// Node: asynchronous libuv task. WASM: synchronous after init().
const inspection = await inspectPdf(bytes);
```

```python
from bulkparse import inspect_pdf
inspection = inspect_pdf(pdf_bytes)
```

The inspection functions accept the usual options/limits. Format selection, strict conversion, CSV options and Unicode output normalization do not change inspection; it always inspects a PDF's original mapped text. Rust also exposes `inspect_pdf_with_cancel`. Normal conversion returns the same report as `pdf_inspection`, with actual OCR outcomes filled in when a provider is used. Bindings expose inspection, not new OCR provider registration APIs.

## Evidence and routing

Each page contains:

- `kind`: `text`, `image`, `mixed`, `vector`, `blank` or `unknown`. An image classification does not prove the image contains text or came from a scanner.
- `route`: `native`, `ocr`, `skip` or `review`, with stable machine-readable `reasons`.
- Native and suspicious character counts, text operations, invisible text operations, painted image count, approximate image coverage and painted path count.
- `ocr_status`: `not_needed`, `not_requested`, `applied`, `empty`, `failed` or `budget_exhausted`.

`pages_needing_ocr` remains the native-evidence recommendation after OCR succeeds. Consult `ocr_status` for what actually happened. Document `kind` summarizes nonblank pages; any unknown page makes the summary unknown. Every page is inspected, without sampling or invented confidence probabilities.

| Evidence | Route |
| --- | --- |
| Usable native text, including short multilingual text | Native |
| Painted images or paths with no native text | OCR candidate |
| Undecodable font runs or a substantially corrupt Unicode layer | OCR candidate |
| At least half the page covered by images and fewer than 100 non-whitespace native characters | OCR candidate, including scans with a small native header |
| Sparse native text plus repeated, similarly sized filled curves aligned into text-like runs | OCR candidate (`outlined_text_with_sparse_native_text`) |
| No text, painted images, painted paths or uncertain graphics | Skip |
| Extraction failure or uninterpretable graphics without known text/image/path content | Review |

Text-quality checks count replacement characters, non-whitespace controls and private-use Unicode. OCR is recommended if all characters are suspicious, or at least three are suspicious and comprise at least 20% of the text. A single damaged glyph in otherwise readable prose remains a warning. There are no document-name/phrase markers, dictionaries or English letter-frequency rules. Invisible text is reported but is not automatically considered corrupt.

Image coverage uses a fixed 32×32 grid in the inherited CropBox/MediaBox intersection. It counts only fully covered cells in the union of painted image rectangles, so repeated logos cannot inflate area and tiled scans can combine. Image placement, Form matrices and axis-aligned Form bounds are respected. Image resolution and unused resources do not determine coverage. Reversed rectangles are normalized. Invalid optional geometry does not discard native text.

Nonrectangular clipping, arbitrary rotations/shears, graphics-state effects, patterns, shadings and annotations make coverage uncertain (`null`). Their visual effects are not reconstructed. Native text extraction retains its existing geometry/visibility limitations. The coverage grid can underestimate narrow strips and boundary slivers.

Filled curved paths also supply outline bounds. With fewer than 100 native characters, the inspector looks for repeated similarly sized, horizontally aligned shapes: at least three qualifying runs and at least 60 shapes (also at least four times the native character count). Each run needs at least 12 non-overprinted shapes and sufficient horizontal extent. Ordinary rectangular rules, off-page shapes, and a large path count alone do not satisfy this rule. This catches some text converted to outlines without rendering. It can still miss text combined into large paths, or confuse sufficiently regular artwork with text. The sparse-text gate also leaves mixed pages with substantial native text outside this rule.

## OCR behavior and limits

OCR is still opt-in. Inspection alone never starts Python, PDFium, ONNX Runtime, network access or model downloads. Page budgets count attempted calls, including failed calls. Existing cancellation and input/text/node/page/provider limits remain in force. This is not an OS memory sandbox, and PDF loading cannot be interrupted at every allocation.

The provider receives the selected **whole page**. Successful, usable OCR replaces that page's native blocks instead of appending duplicates. Failure, empty output, exhausted page budget or predominantly invalid Unicode OCR retains the native blocks. Hard resource-limit and cancellation errors still stop conversion. Native extraction plus accepted OCR consumes the document text budget conservatively, even if OCR later replaces native blocks. Plain-text and geometry-aware providers both work.

This does not implement regional OCR, native/OCR geometric fusion, a learned layout model, table recognition from scans or semantic validation of OCR output. A large photograph with a short caption can trigger OCR unnecessarily; a scan with a long but inaccurate printable text layer can be missed. Photos and diagrams may contain no text. These are explicit heuristics, not a guarantee of complete document understanding. All PDF conversion results remain partial.

## Schema compatibility

Output schema is **0.3**. `ParseResult`/binding conversion JSON adds optional `pdf_inspection`, omitted for non-PDF inputs. Existing document blocks and OCR geometry fields remain available. Exact-version clients must accept 0.3. Generated JSON Schema and Node/WASM declarations describe the new report; Python returns the same JSON dictionary.

## Validation

- 17 new Rust integration tests cover routing, text corruption, mixed content, tiled/repeated images, Form transformations/clipping, hidden text, annotations, reversed bounds, limits, cancellation, failures and native fallback.
- All 80 PDFs in the existing hash-pinned corpus pass: 79 retain identical native blocks and inspection/conversion route parity; the encrypted input retains its expected failure. This is a regression check, **not** a classifier accuracy benchmark. This historical check is recorded in the original experiment; use the [current regression guide](../benchmarks/README.md#regression-checks) for maintained commands.
- 13 conversion cases and 3 inspection cases match across rebuilt Node, Python, release WASM and CLI. Node/Python/WASM smoke and inspection checks pass.
- The existing eight authored real-OCR cases meet their previous quality gates. The Swedish recognition error remains. See `reports/pdf-inspection-ocr-final-2026-11-10`.
- An additional authored scan with native header `Monthly archive` routes to OCR and recovers the header and scanned body exactly after whitespace normalization, with the header occurring once. The fixture generator is `crates/bulkparse/examples/make_inspection_fixture.rs`; the OCR result and check are in `reports/pdf-inspection-2026-11-10`.

Run inspection unit tests with `cargo test --locked -p bulkparse` and cross-binding checks with `node scripts/check_binding_parity.mjs --pdf-corpus` after building the bindings. Current comparison and regression commands are in the [benchmark guide](../benchmarks/README.md).

A direct local comparison is now available: [BulkParse vs Firecrawl PDF Inspector](pdf-inspector-comparison.md). Inspection speeds are similar; conversion throughput and quality tradeoffs are reported separately.

The outlined-text and table-alignment study (historical artifact; not distributed) adds one routing integration test with positive and negative cases. Across 304 preservation inputs, the only routing change is the reviewed infographic; this does not estimate population-level OCR detection accuracy.
