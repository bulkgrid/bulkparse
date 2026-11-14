# Mixed-document benchmark

The frozen corpus contains **200 documents across 19 formats**. It combines 112 development documents with 88 additions selected before conversion. It is not an entirely unseen holdout. Source URLs, licensing metadata and hashes are in the [manifest](../../reports/mixed-200-2026-11-13/corpus-manifest.json); publisher selection and deduplication are recorded in the [freeze metadata](../../reports/mixed-200-2026-11-13/corpus-freeze.json). Inputs are downloaded separately and must retain their source notices.

[Published results and exact run sources](../../reports/mixed-200-2026-11-13/README.md).

## Measurement

Eight engines process documents sequentially in persistent workers, with OCR off. The overview uses one diagnostic pass per displayed engine. The current BulkParse pass was run later the same day on the identical corpus. Worker startup and output transport are excluded from conversion timing; required subprocess launches are included. Unsupported inputs remain in the coverage denominator. A 90-second per-document deadline records a failure and restarts the worker before continuing.

Per-document median time covers nonempty results. Corpus time includes failed attempts, excluding inputs outside the configured adapter's format profile. Different successful subsets must not be ranked as equal workloads. Partial results are not guarantees of complete extraction.

CPU includes worker and completed child CPU. macOS high-water RSS includes initialization and waited-for child-process peaks; it is not the simultaneous process-tree sum. Missing accounting after terminated workers is reported as a lower bound.

## Reproduce

The current measurement harness requires macOS, Rust, Python, Java where required by a configured converter, and installed CLI tools. Inspect `scripts/benchmark_realworld.py` and `scripts/worker_mixed200.py` for the expected executable paths. Provision the benchmark-only converter checkout used by `benchmarks/multiformat/anydoc/Cargo.toml` at `legacy/anydoc-main/`; its source hashes and package version are recorded in the results. Reference checkouts are ignored by Git.

Create isolated Python environments at `target/competitors/broad-venv`, `target/competitors/docling-venv` and `target/competitors/realworld-venv`. Install the corresponding `*-requirements.txt` files from the [run directory](../../reports/mixed-200-2026-11-13/run/). Provision the recorded Docling models separately using their upstream instructions and licenses; model hashes are in [model-manifest.json](../../reports/mixed-200-2026-11-13/model-manifest.json). Install Pandoc and LibreOffice at the paths expected by the worker, or adjust and record those paths.

```sh
python3 scripts/fetch_mixed200.py
cargo build --release --locked -p bulkparse --example office_bench
cargo build --release --locked --manifest-path benchmarks/multiformat/anydoc/Cargo.toml --target-dir target/multiformat-anydoc
python3 scripts/benchmark_mixed200.py \
  --manifest reports/mixed-200-2026-11-13/corpus-manifest.json \
  --bulkparse-binary target/release/examples/office_bench \
  --out reports/new-mixed-run
```

Start with one pass. A repeated performance study can add `--final-validation --passes 5 --warmup`. Use a new output directory and do not run other benchmarks concurrently. The harness verifies input hashes and records source hashes and options. Source URLs may cease to serve identical bytes; changed downloads must fail rather than silently replace the corpus.

## Quality assessment: planned, not completed

Use one seeded 40-document sample covering all tested formats. Compare source-rendered references with each engine's output over the same content scope. Assess completeness, structure, formatting and cleanliness on anchored 0–100 scales. Conceal engine labels where practical and preserve the evidence supporting every judgment.

This project's chosen review route is assistant review, not an independent blind study. No mixed-corpus quality score has been published. Speed, nonempty output and warning counts are not substitutes for extraction accuracy.

Use equal per-format weights shared by every engine. Failed and unsupported inputs score zero when meaningful reference content is assessable; unjudgeable references must be reported separately. Also report supported-only quality and sample counts, so coverage and fidelity remain distinguishable. Never compare averages over different format subsets as an overall ranking.

The judging tools remain available for future evaluation. External paid judging requires a separate cost estimate and approval; no such jobs are part of the published benchmark.
