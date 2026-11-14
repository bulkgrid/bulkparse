# Expanded same-machine comparison

Benchmark adapters only; none of these comparators becomes a BulkParse dependency.
The parser is frozen for evaluation: no extraction rules or output repairs are
changed in response to this run.

## PDF

Nine configurations cover BulkParse, AnyDoc, pdf-inspector, LiteParse,
OpenDataLoader, PyMuPDF4LLM with and without learned layout, MarkItDown and
Docling. All attempt the same 200 PDFs from the pinned opendataloader-bench
checkout. The unchanged evaluator reports NID, TEDS, MHS and overall. Failed
conversions write empty predictions and remain in the denominator; partial
outputs remain labeled. A returned output is not a completeness guarantee.

One excluded full warm-up precedes five measured full passes. Engine order
rotates; one engine processes documents at a time, sequentially, in its
persistent process. Reads, parsing, Markdown export, output writes and outcome
bookkeeping are timed. Startup/model initialization, request/reply serialization,
hashing and evaluation are excluded. There is no result cache or output repair.
OpenDataLoader uses a persistent embedded JVM and the CLI's exit-code-returning
`run` implementation via reflection, avoiding a JVM launch per PDF. Its adapter
also reads the generated Markdown back before returning.

OCR is disabled. Native engines request one thread; Docling retains its existing
CPU/four-thread Heron + accurate TableFormer profile. PyMuPDF4LLM's native and
learned-layout profiles are both shown; neither uses OCR. MarkItDown may use its
bundled model for format detection, but not model-based PDF parsing. Actual
worker options and installed package versions are saved in each report.

Timing uses warm filesystem caches, without CPU pinning or cache flushing. The
host is the user's working Mac, not a controlled server. Memory is lifetime peak
RSS reported by macOS `/usr/bin/time -l`, including imports and warm-up; it is
not incremental memory per document or a sum of concurrent process-tree memory.
Timing ranges are retained. Quality is evaluated once if all five measured
outputs match; otherwise all five are evaluated and averaged.

## OCR

OCR is evaluated separately on 24 authored cases / 26 pages in nine languages, using Linux ARM64 containers, one CPU and 4 GiB RAM. See the [OCR protocol](../ocr-alternatives/README.md). Authored inputs are retained under `reports/broad-benchmark-2026-11-12/ocr/data`.

## Reproduce

Use the report's requirements files and Cargo lockfiles, provision Docling and
OCR models as documented in their existing protocols, then build:

```sh
cargo build --release --locked --manifest-path benchmarks/opendataloader/Cargo.toml --target-dir target/broad/bulkparse
cargo build --release --locked --manifest-path benchmarks/broad/pdf-inspector/Cargo.toml --target-dir target/broad/pdf-inspector
cargo build --release --locked --manifest-path benchmarks/same-machine/anydoc/Cargo.toml --target-dir target/same-machine-benchmark/anydoc-build
target/opendataloader-venv/bin/python scripts/benchmark_broad.py --mode pdf --out reports/new-pdf-run
python3 scripts/benchmark_ocr_alternatives.py --profiles rapid-small lite-tiny paddle-tiny tesseract-fast --out reports/new-ocr-run
```

Use fresh output directories. Do not run benchmark suites concurrently. The
published evidence retains hashes, effective options, per-pass timing and evaluator output. New runs regenerate predictions; historical binaries and source snapshots are not distributed. Upstream corpus inputs,
model weights and installed runtimes are not redistributed in the report. The
authored OCR images and reference text are included under `ocr/data`; restore
those to `target/ocr-alternatives/data` and provision the hash-pinned models
using the OCR protocol.

The superseded Office-only mode has been consolidated into the [200-document mixed runner](../realworld/README.md). `--smoke` checks two PDFs without publishing quality scores; the full run retains one warm-up and five measured passes.
