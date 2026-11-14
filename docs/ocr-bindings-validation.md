# Rust OCR migration validation

The CLI, Node and Python OCR bindings use the Rust native provider. Bulkgrid's
file-converter service has been migrated to TypeScript and the Node binding; its
production Docker image contains no Python interpreter or Python worker. Python
bindings and historical benchmark tooling remain available. WASM continues to
accept validated OCR evidence; it does not run the native inference libraries.

## Coverage and compatibility

- **200/200 mixed documents** returned nonempty Markdown through the actual
  TypeScript service parser path. Of these, 199 hashes match the saved mixed
  benchmark exactly; the remaining historical output was empty. Nonempty output
  is a coverage measurement, not proof of perfect extraction.
- **14/14 development scans** returned output. The eight ICDAR pages contain
  **11 annotated tables; all 11 are detected**. TEDS is **0.9461** for Rust and
  **0.9045** for the compared Docling pipeline. Structure-only TEDS is **1.0000**
  and **0.9925**, respectively. The other six scans come from DocLayNet pages.
- **26 OCR profile cases** complete without OCR failures, including degraded
  images, nine languages and a rotation mismatch. The deliberately blank input
  stays empty. **21/24 referenced cases** match after whitespace normalization;
  two external smoke cases have no accuracy reference.
- **Seven synthetic image/PDF cases** produce identical Node and Python results:
  PNG, JPEG, GIF, WebP, TIFF, two-frame TIFF and scanned PDF. Each retains the
  expected merged header and values. These are variants of one authored table,
  not seven independent real-world quality examples.
- The **Linux ARM64 runtime image** starts its HTTP service and parses JPEG,
  scanned PDF and multipage TIFF with networking disabled and Python absent.
  Linux x64 artifacts are pinned by the provisioning script but were not executed
  on this ARM64 development machine.
- Automated checks include **288 core Rust tests**, **18 native OCR tests**,
  **10 Node tests**, **8 Python tests**, WASM smoke/package tests, and **16 service
  compatibility tests**. Rust lint, TypeScript checking/build and service lint pass.

## Fixes validated

The native provider now uses the correct PP-OCRv6 BGR normalization and a
compatible PP-LCNet orientation model. It processes recognition crops one at a
time and reuses model sessions. The default adapter's older normalization was
not compatible with the selected detector family; correcting it fixed the noisy
English and Turkish character cases without changing recognition weights.

Table assembly repairs drifting numeric rows using repeated text baselines and
column positions. It requires conservation of all recognized tokens and does not
use document names, IDs or expected answers. Reliable ruled grids claim only their
own text; the remaining page is still examined for other tables and headings.
Unclaimed OCR lines are retained when a predicted grid is invalid.

Multiline metric headers can now be reconstructed from aligned units and repeated
body columns, with token conservation and conservative rejection of ambiguous
geometry. This runs after numeric-row repair so adding header row spans does not
suppress existing body alignment. Empty cells gain dash marks only when an
isolated horizontal stroke is visible in the cell interior. No additional model
calls or page-sized buffers are introduced.

Recognition now stretches contrast only for dim, low-contrast text crops, using
percentile bounds, early rejection of high-contrast or nearly uniform crops, and
an in-place lookup table. This fixes the extra noisy Swedish `blà` → `blå` error
without changing recognition weights, DPI or inference count. The three remaining
Swedish outputs match the tested Docling configuration after whitespace normalization.

The TypeScript service preserves direct/Pub/Sub job envelopes, workspace filters,
processing locks, checksum validation, quotas, immutable Markdown storage,
deletion checks, metadata/page counts, duplicate-job handoff repair and durable
indexing outbox behavior. Tests exercise retryable/permanent errors and recovery
from hung, crashed and malformed parser processes. Database/queue tests use fake
transports, not production services. No deployment was performed.

## Limits and interpretation

These are existing development inputs, not a held-out accuracy study. All eleven
annotated table structures now match in the refinement run, but individual header
words, symbols and cell contents remain imperfect. The three remaining referenced OCR errors concern Swedish
uppercase accents (`Å` → `A`). They are recognition-stage errors; the evidence does not isolate
model weights from cropping/resolution effects completely. Crop-margin experiments
fixed some Swedish cases but introduced skewed-Swedish and Turkish regressions,
so the recognition changes were rejected. No spelling dictionary
or reference-based output repair is applied.

The default pipeline does not infer LaTeX from formulas or describe images. The
previous service did not enable formula enrichment. Parser fidelity diagnostics
remain available in structured API output; the service explicitly rejects failed
or budget-exhausted OCR instead of acknowledging an unfinished OCR job.

Binding deadlines are cooperative. The service wraps the addon in a supervised
Node child to enforce a hard parsing deadline. Pixel, input, page and output
limits remain explicit; arbitrary inputs are not guaranteed to fit those budgets.

## Reproduction and artifacts

See [the contrast-fix report](../reports/native-ocr-accent-fix-2026-11-14/README.md)
for the final noisy-Swedish correction, regression checks and resource comparison.
See [the refinement report](../reports/native-ocr-refinement-2026-11-14/README.md)
for the latest table results, before/after resource measurements and rejected
recognition experiment. See [the migration report](../reports/native-ocr-migration-2026-11-14/README.md)
for performance measurements, machine details, model identities and per-document
results. Quality-pass timings were collected during other validation work and
must not be used as performance comparisons.

```sh
node scripts/provision_native_ocr.mjs --output "$PWD/target/native-ocr/provisioned"
cargo build --release -p bulkparse-cli --features ocr
cargo test -p bulkparse-ocr-native
python scripts/evaluate_ocr.py --binary "$PWD/target/release/bulkparse" \
  --config "$PWD/target/native-ocr/provisioned/config.json" --out target/ocr-check
python scripts/check_ocr_results.py target/ocr-check
```

After building the native language bindings, run
`scripts/check_ocr_binding_images.py` (Pillow is a fixture-only dependency) and
`scripts/validate_native_profiles.py`. `scripts/check_scanned_layouts.py prepare`
creates the deterministic scan set from the existing ICDAR/DocLayNet corpus;
`scripts/validate_native_ocr.py` scores saved native JSONL predictions. All model
inference in the new pipeline runs natively, including when called from Python.
