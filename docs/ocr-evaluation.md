# First real OCR integration — 2026-11-10

Historical baseline: see the later [geometry and reading-order milestone](ocr-layout.md)
for the current implementation and corrected column case. The results below are
retained unchanged for comparison.

BulkParse now has a real, optional PDF OCR path through the Rust
`bulkparse-ocr::ProcessOcr` provider and an OCR-enabled CLI. Rust owns routing,
document budgets, cancellation, process supervision and result mapping. PDFium,
RapidOCR and ONNX Runtime provide rendering and recognition in a separately
provisioned Python environment. The default engine and bindings gain no OCR
runtime dependency.

## What was evaluated

Eight original PDF cases were evaluated once with native extraction and once
with OCR. Six were newly authored before observation, with expected text written
before rasterization: prose, invoice values, Swedish text, two columns, a blank
scan and a two-page scan. Two earlier scan-only/mixed-native-scan cases were
included as regressions. The two-page case reuses the prose/invoice content; it
is a routing test, not independent recognition evidence.

All scan pages contain rasterized text, with no hidden native text layer. The
fixture source and references are bundled under `fixtures/ocr`; source scans were
rendered at 144 DPI and the backend rasterizes PDF pages at 200 DPI. Rendering at
a higher DPI cannot create missing source detail. The fixtures were visually
reviewed. Model settings were not tuned after seeing the results, and outputs
received no spelling fixes, filename conditions or document-specific replacements.

## Results

Exactness below means NFC-normalized text with whitespace collapsed, retaining
case, punctuation and accents. These are controlled examples, not a population
accuracy estimate or a comparison with competitors' OCR engines.

| Case | Actual OCR result |
| --- | --- |
| English prose | Exact expected text |
| Invoice values | Exact expected text, including identifier and decimal values |
| Two-page scan | Exact combined text, with page provenance preserved |
| Existing scan-only PDF | Exact expected text; default native extraction is empty |
| Native page followed by scan | Exact combined text; native page unchanged; OCR applied only to page 2 |
| Blank scan | Empty result with `pdf_ocr_empty`; no false claim of recovered text |
| Swedish | One character error: `Åtta` became `Atta`; 1/9 word error rate and 1/67 character error rate |
| Two columns | All words recovered, but rows interleave the columns; intended line-pair order is 80%, word error rate is 64% |

The column error rate is largely an ordering error, not missing characters. It is
retained as a known failure. The Swedish accent was not repaired through a custom
word substitution. Exact-line order also penalizes the Swedish misspelling, so
that metric should not be interpreted as evidence that its lines were reordered.

All outputs remain **partial**, including exact text matches. OCR does not
establish correct tables, paragraph hierarchy, image coverage or visual fidelity.
The initial provider contract returns plain text; recognized line boxes and
confidence scores are not yet preserved in the shared document model.

## Cost and limits

On the local Apple Silicon machine using Python 3.12, a PDF with one OCR page took
about 1.74–1.97 seconds end to end; the two-scan-page document took 3.64 seconds.
These single observations include Python startup, PDF parsing/rendering, model
loading, inference and JSON output. The backend starts once per eligible page.
They are not steady-state inference benchmarks or production sizing estimates.

The default document budget permits eight OCR attempts and 1 MiB aggregate
recognized UTF-8 output. The adapter independently enforces page timeout, input
and response limits; the renderer checks pixel dimensions before allocating the
bitmap. The models total about 30.3 MiB, in addition to the Python/native runtime.
Memory/CPU use is not hard-capped by these checks; OS resource limits remain
necessary. No cloud service or deployment was introduced.

## Verification

- Six Rust process-provider integration tests pass: Unicode transport, protocol
  errors, crashes, input/output limits, running cancellation, timeout and reaping.
- The core now has 49 passing integration tests, including a new check that empty
  OCR output emits `pdf_ocr_empty`, not `pdf_ocr_applied`. The minimal core has 16
  passing tests; all nine worker-pool tests still pass.
- Real-engine checks verify page budgets, output/pixel limits, timeout reporting,
  model-hash rejection, strict-mode rejection and skipping the backend entirely
  for native/blank pages.
- Default CLI JSON matches the previous binary exactly on 13 fixtures, and its
  dependency tree excludes `bulkparse-ocr`. Rust, Node, Python and WASM default
  paths remain independent of the backend; binding OCR entry points were not added.
- Workspace all-feature Clippy and formatting pass. An Ubuntu OCR CI job is
  configured with explicit provisioning and quality gates; remote CI has not run.

The [manifest](../benchmarks/ocr/manifest.json),
run metadata (historical artifact; not distributed),
per-case results (historical artifact; not distributed) and
integration checks (historical artifact; not distributed) and
verification record (historical artifact; not distributed) preserve
inputs, references, model hashes, settings and outputs. All converted documents
in this report are original project fixtures; no third-party scan text is bundled.

## Remaining work

The next quality work is retaining OCR geometry and applying reading-order logic,
then testing accents and multilingual recognition on a larger human-reviewed scan
set. Photos, handwriting, damaged pages, dense forms, scanned tables and RTL text
are not established capabilities. Same-page mixtures of native text and scanned
regions, and bad hidden OCR layers, are not detected by the current routing policy.
Persistent model loading and binding APIs should follow that fidelity work.

See [setup and CLI usage](../backends/rapidocr/README.md) to reproduce the optional
backend. This integration is usable experimentally; it does not make arbitrary
document conversion flawless.
