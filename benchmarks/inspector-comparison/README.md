# BulkParse vs Firecrawl PDF Inspector

This benchmark uses the supplied `pdf-inspector-main` snapshot (package version
1.25.2) and BulkParse's current PDF-only core. It does not modify either engine.
The benchmark adapters are separate workspaces; their lockfiles and source and
binary hashes pin the experiment. The downloaded competitor source has no
verified Git revision. No OCR runtime, models, rendering or network calls run.

Build from the project root (the alternate competitor Cargo home was already
populated by previous local work):

```sh
CARGO_TARGET_DIR="$PWD/target/inspector-comparison/bulkparse" cargo build \
  --manifest-path benchmarks/inspector-comparison/bulkparse/Cargo.toml --release --offline
CARGO_HOME="$PWD/target/competitors/cargo-home" \
CARGO_TARGET_DIR="$PWD/target/inspector-comparison/firecrawl" cargo build \
  --manifest-path benchmarks/inspector-comparison/firecrawl/Cargo.toml --release --offline
python3 scripts/compare_pdf_inspectors.py --out reports/inspector-comparison-repeat
python3 scripts/check_inspector_comparison.py reports/inspector-comparison-repeat
```

Both binaries use release builds, thin LTO, stripped symbols and the same Rust
compiler. One persistent worker per engine is driven sequentially, with engine
order alternating by case and repetition. Rayon is limited to one thread; no
claim is made about Firecrawl's default multithreaded throughput. Each case gets
one unmeasured warmup per mode and seven measured repetitions. Input reads and
JSON serialization are outside the internal Rust timer. Conversion includes
Markdown rendering. Memory is whole-worker peak RSS from `wait4`, including
warmups, input reads and serialization, not allocation per document or OCR memory.

Modes:

- BulkParse inspection: `Parser::inspect_pdf`, all pages, native text decoded.
- Firecrawl inspection: `process_pdf_mem_with_options`, `DetectOnly`,
  `ScanStrategy::Full`; no decoded-text quality pass. This is related work with
  different capabilities, not identical algorithms.
- BulkParse conversion: default PDF options, including Unicode normalization off,
  followed by Markdown rendering.
- Firecrawl conversion: `ProcessMode::Full`, `ScanStrategy::Full`, default Markdown
  settings. This performs more structural inference than BulkParse.

The 32 inputs comprise the existing 24-PDF holdout set, six additional controlled
OCR scans, one native-header-over-scan regression and one native-plus-empty PDF.
These are development/regression inputs, not a new independent holdout. Twenty
have predefined structural OCR-candidate routes. The blank raster scan is still
an OCR candidate: its pixels are not inspected. Twelve external documents have no
OCR-routing gold and are excluded from routing accuracy counts.

Text metrics reuse the existing evaluation helpers. Ten authored native documents
have text/order references; one external input has an upstream transcription;
eleven use PDFKit-derived silver references. OCR-dependent full-text references
are retained but their native-only missing text is not confused with OCR quality.
No single composite accuracy score is produced. Repeat predictions are checked
for determinism, all attempts remain in accounting, and worker failures are not
silently removed.

Results: `reports/inspector-comparison-2026-11-10-v2`. Raw predictions stay under
`target/inspector-comparison/inspector-comparison-2026-11-10-v2`; reports contain
hashes and metrics, not unreviewed third-party document text.

The initial report without `-v2` has valid timings but unavailable RSS: macOS
blocked `/usr/bin/time -l`'s `kern.clockrate` query. The final run replaced that
measurement with `wait4` on each directly owned child; all four workers exited 0.
Use the v2 report for the comparison.
