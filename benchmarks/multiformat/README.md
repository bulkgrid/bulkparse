# Multiformat benchmark protocol

BulkParse, AnyDoc and Docling run in separate persistent processes. This directory
is a benchmark adapter only; production BulkParse never links to AnyDoc/Docling.

The original frozen report is here (historical artifact; not distributed).
The quality follow-up (historical artifact; not distributed)
uses the same corpus and timing protocol, with transparent source-grounded checks.
The final run used 70 external reference fixtures and 11 authored semantic checks.
Nine malformed files ran separately. No OCR or LLM judging was used. These inputs
are observed development cases, not an independent holdout.

## Reproduce in the BulkParse checkout

1. Restore matching `anydoc-main` and `docling-main` reference trees. Verify the
   paths and SHA-256 hashes in the final corpus manifest. The upstream fixtures
   are not bundled in the evidence archive. The original source hash snapshot
   and final source snapshot distinguish the implementation under test.
2. Install dependencies from `Cargo.lock` and the comparator's separate lockfile.
   Use the pinned Docling 2.150.0 environment from the previous same-machine
   benchmark; `python-pptx` is only a writer for the authored PPTX fixture, not a
   BulkParse runtime dependency. Its generated fixture hash is frozen.
3. Build and test:

```sh
cargo test --workspace --all-features
cargo build --release -p bulkparse-cli
cargo build --release -p bulkparse --example office_bench
CARGO_TARGET_DIR="$PWD/target/multiformat-anydoc" cargo build \
  --manifest-path benchmarks/multiformat/anydoc/Cargo.toml --release --locked
# If regenerating authored inputs, verify their hashes before benchmarking:
target/competitors/docling-venv/bin/python scripts/generate_office_benchmark.py
python3 scripts/benchmark_office_formats.py --out reports/my-office-run
```

The harness expects the Docling interpreter at
`target/competitors/docling-venv/bin/python`. It checks every corpus hash before
running. Output directories must be new. The command needs local process resource
accounting access for `/usr/bin/time -l` on macOS; unavailable metrics must remain
unreported, not guessed. Linux timing can be run, but the present report and
memory parser were validated on macOS only.

One full warm-up is excluded. Five full passes rotate engine order. Per-document
clocks include reads, native/library conversion and Markdown export; they exclude
process startup, protocol serialization, disk output and scoring. Corpus times
sum document clocks. All inputs are sequential, and no conversions from separate
engines overlap. One-thread environment settings are recorded in the harness.
Failures remain in results and timings. Unsupported Docling formats are not
attempted and are not assigned zero-duration successes.

Authored checks assert literal required/forbidden text. Structural checks are in
`crates/bulkparse/tests/office.rs`. Neither is an LLM quality assessment. A
returned document, even with no diagnostics, does not establish full fidelity.
Do not compare the aggregate Docling time to the 81-document Rust workloads
without accounting for its smaller supported subset and output differences.

## Source-grounded quality follow-up

After a new benchmark run, evaluate the frozen development checks and regenerate
the per-document audit of the original 67 partial results:

```sh
python3 scripts/validate_office_benchmark.py reports/my-office-run
python3 scripts/evaluate_office_quality.py --run reports/my-office-run --out reports/my-quality-evaluation
python3 scripts/check_office_pdf_regression.py --out reports/my-quality-evaluation/pdf-regression.json
```

The evaluator verifies source and relevant package-part hashes. Its manifest
includes both fixed behaviors and remaining gaps. These checks were selected
after inspecting the corpus and are not blind/independent or a replacement for
AnyDoc's private benchmark. Retain the historical `bulkparse-pass-1.json` to
reproduce the before/after comparison. The PDF check requires its separate
frozen PDF manifest and prediction directory from the previous PDF report.

The legacy/equation/navigation follow-up (historical artifact; not distributed)
uses the same inputs and measurement protocol with schema 0.5. Reproduce its
54 selected source checks using `evaluate_office_quality.py` with
`--baseline reports/multiformat-quality-2026-11-12/benchmark-final` and
`--spec reports/multiformat-legacy-quality-2026-11-12/quality-checks.json`.
The previous 45 cases and nine additional equation cases are reported separately.
