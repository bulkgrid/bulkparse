# Low-contrast OCR correction — 14 November 2026

BulkParse now reads **blå**, correcting its extra Swedish error. The three
remaining Swedish outputs match the previously tested Docling configuration after
whitespace normalization. Both still misread uppercase Å as A in those cases.

## Speed and memory

Same M3 Max, one CPU inference thread, 17 inputs including the low-contrast PDFs.
Each build gets a full-corpus warm-up and one measured sequential pass:

| Metric | Before | After |
| --- | ---: | ---: |
| Elapsed time | 110.57 s | 110.61 s |
| CPU time | 110.53 s | 110.63 s |
| Peak sampled process-tree RSS | 1.586 GiB | 1.533 GiB |

Elapsed time differs by about **0.04%**, effectively unchanged in this diagnostic
run. Peak memory is lower in the observed run. No material speed or memory
regression was observed; single passes do not prove a universal performance
bound or a causal memory improvement. Both builds complete all 17 inputs.
[Raw timing, CPU and memory accounting](performance.json).

## Cause and fix

The accent exists in the source pixels. Docling's recognition model itself returns
`blå`; it is not a later spelling replacement. With Docling's rendering scale
changed from its default 216 DPI to 200 DPI, it instead returns `blà`. Feeding our
saved crop through the same recognition weights with OpenCV linear resizing also
returns `blà`. Raising BulkParse's rendering resolution alone does not fix it:
region selection, crop geometry and resampling differ between pipelines.

This isolates the discrepancy to sensitivity to the pixels reaching recognition,
rather than a character-encoding or Markdown conversion bug. It does not establish
one resampling operation as the sole cause. On the same crop and same recognition
weights, stretching the compressed foreground/background contrast fixes the word.

The accepted Rust rule normalizes **only dim, low-contrast text crops**:

- Use a 256-bin luminance histogram and the first/99th percentiles to ignore sparse
  noise. Require elevated dark values, muted highlights and sufficient signal
  range; reject nearly uniform patches and already high-contrast text.
- Exit early once enough dark or bright pixels prove the crop cannot qualify.
  Ordinary white-margin crops typically need only about 1% of their pixels read.
- Apply a small integer lookup table in the existing crop buffer. Recognition
  weights, rendering DPI, detector geometry and model-call count remain unchanged.

There are no language checks, document IDs, word substitutions, extra recognition
passes or additional image buffers. The histogram/lookup use a few KiB of fixed
stack storage. The existing crop is adjusted; the source page and layout image
remain unchanged.

Diagnostic artifacts: [resize-only probe](resize-probe.json),
[contrast-only probe](contrast-probe.json), [Docling DPI probe](docling-dpi-probe.json),
[BulkParse DPI probe](native-dpi-probe.json), [three-case comparison](docling-parity.json).
The diagnostic probes use Python tooling, but the production fix and inference
remain Rust. Model weights are unchanged. These are development fixtures.

## Regression checks

- **26 OCR profile cases complete**; 25 Markdown outputs are identical to the prior
  build. The only change is `blà` → `blå` in the noisy Swedish case. The reference
  exact-match count remains **21/24**, because uppercase Å is still wrong in each
  of the three shared failures. The intentionally blank case remains empty.
- All **14 complex scans** return output and all **11 annotated tables** are
  detected. TEDS changes from **0.94613285 to 0.94615437**, with structure-only TEDS
  unchanged at **1.0000**. No page's table score regresses. The only Markdown change
  is another corrected recognition: `ToC` → `TOC` in a chemical-limit table.
- All **200 mixed documents** return nonempty Markdown with hashes identical to
  the preceding service validation.
- Native Node/Python results match across all **seven** authored image/PDF variants.
  Native OCR unit tests, binding tests and Rust lint pass; counts are in the
  accompanying check record.

These checks establish no regressions on the tested development inputs, not
universal OCR accuracy. [Profile metrics](multilingual-quality.json),
[table metrics](scanned-quality.json), [mixed results](service-mixed-200.json),
[binding results](binding-images.json), [source/model identity](identity.json).

## Reproduce

Build the CLI, Node and Python bindings, using the same provisioned model config
as the preceding migration/refinement runs. Then run:

```sh
cargo test -p bulkparse-ocr-native
cargo clippy --workspace --all-targets --all-features -- -D warnings
python scripts/validate_native_profiles.py \
  --config target/native-ocr/provisioned/config.json --out target/ocr-profile-check
python scripts/check_native_scans.py \
  --config target/native-ocr/provisioned/config.json --out target/ocr-scan-check.jsonl
```

The performance manifest adds the low-contrast English and Swedish PDFs plus the
low-resolution Swedish PDF to the 14 complex scans. To compare a frozen baseline
addon with the updated addon, point `BULKPARSE_SERVICE_ROOT` at separate copies of
the same built service worker resolving each addon, and set `BULKPARSE_OCR_CONFIG`.
Run each build separately, without competing build/validation jobs:

```sh
python scripts/benchmark_native_migration.py --engines native-service \
  --manifest reports/native-ocr-accent-fix-2026-11-14/performance-manifest.json \
  --node-worker scripts/native_service_bench_worker.mjs --out target/ocr-perf-check
```

The harness requires psutil. Quality scoring uses the existing opendataloader
Python evaluation environment. No production database, queue, network storage or
paid API is used. Nothing is deployed by these checks.

The service’s bundled source archive and Node addon were refreshed from the tested build and verified by hash. All 16 service tests, TypeScript checks, and lint passed. Nothing was deployed.
