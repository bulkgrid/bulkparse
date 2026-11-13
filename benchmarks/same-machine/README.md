# Same-machine native and model-based PDF comparison

This protocol compares current BulkParse 0.1.0, the local AnyDoc 0.2.4 checkout
with its locked pdf-inspector 1.14.2 dependency, and Docling 2.150.0 with
Docling Parse 7.23.0. It uses the same pinned opendataloader-bench corpus and
unmodified NID/TEDS/MHS evaluator as the original 200-PDF study.

## Profiles fixed before the run

- BulkParse: current native PDF parser, default Unicode normalization, no OCR.
- AnyDoc: default native PDF-to-Markdown API, no OCR. Its OCR-needed errors
  remain errors; the adapter does not substitute partial extraction.
- Docling: standard pipeline, default Heron layout, TableFormer V1 accurate
  mode with cell matching, CPU with four configured threads, OCR off. Picture
  classification/description and code/formula enrichment are off. Its ordinary
  Markdown exporter is used, matching upstream's adapter; no custom HTML table
  export or score-specific output repair is applied.

These are different capability/cost profiles on the same machine, not identical
algorithms. Docling uses learned layout and table models. The Rust engines use
one Rayon thread; Docling uses four CPU threads and its standard internal
pipeline. Documents are submitted sequentially to one process per engine.
All model files are provisioned and hashed before timing; model loading and
process startup are excluded. Runtime downloads and remote services are disabled.

Each persistent engine processes all 200 documents in lexicographic order once
for an excluded warm-up and five times for measurement. Engine order rotates
by round. Only one engine processes documents at a time. The timer includes
reads, conversion, default Markdown export, writes and diagnostic bookkeeping.
Request/reply serialization, hashing and evaluation are excluded. Caches are
warm; no result cache, fsync, cache flush or CPU pinning is used. A full-run
response deadline of one hour marks a stalled run incomplete, not fast/successful.

All 200 attempts stay in the quality denominator. Failed conversions write empty
predictions; partial outputs are retained and labeled. Successful return is not
proof of full coverage. Predictions are saved for every round. If any differ,
quality is evaluated on all five measured rounds and reported as their mean,
with individual results retained. Failures consume elapsed time and are reported
alongside speed; they must not be presented as successful throughput.

## Reproduce

The existing `benchmarks/opendataloader` instructions provision the pinned
200-PDF corpus and evaluator environment. Build BulkParse from the frozen source
in `reports/opendataloader-table-boundaries-2026-11-11-v3`; the new harness verifies
that both current source and its saved worker still match that report.

Build the AnyDoc corpus adapter with the existing populated Cargo cache:

```sh
CARGO_HOME="$PWD/target/competitors/cargo-home" cargo build --offline --locked \
  --manifest-path benchmarks/same-machine/anydoc/Cargo.toml --release \
  --target-dir target/same-machine-benchmark/anydoc-build
```

Use the pinned Docling Python environment from the earlier holdout study. Model
locations under `target/competitors/models` are:

- `docling-project--docling-layout-heron`: Hugging Face repository
  `docling-project/docling-layout-heron`, revision
  `8f39ad3c0b4c58e9c2d2c84a38465abf757272d8`.
- `docling-project--docling-models/model_artifacts/tableformer/accurate`:
  `docling-project/docling-models`, tag `v2.3.0`, resolved revision
  `fc0f2d45e2218ea24bce5045f58a389aed16dc23`; download `tm_config.json` and
  `tableformer_accurate.safetensors`. Preserve the repository model card/license.

The harness records installed package versions, source hashes, model hashes,
effective pipeline options, executable hashes and full corpus accounting.

```sh
PYTHONPATH=scripts target/opendataloader-venv/bin/python -m unittest scripts/test_same_machine_benchmark.py
target/opendataloader-venv/bin/python scripts/benchmark_same_machine.py \
  --out reports/same-machine-pdf-NEW
target/opendataloader-venv/bin/python scripts/check_same_machine_results.py \
  reports/same-machine-pdf-NEW
```

Run on an otherwise idle host. Use a new output directory for every attempt.
Hardware metadata in the harness describes this verified Apple M3 Max host;
update and record it explicitly for a different machine. These corpora have
informed BulkParse development and are not fresh unseen accuracy evidence.
OCR routing needs a separate labeled evaluation.

For a newer verified BulkParse build, supply `--bulk-report <report-directory>` and
`--bulk-worker <persistent-worker-binary>` to `scripts/benchmark_same_machine.py`.
The runner verifies the selected report's entire source manifest and binary hash
before running; the comparison records that reference report in `identity.json`.
The checker verifies all 200 BulkParse prediction hashes against that selected
report. Defaults continue to reproduce the original table-boundary snapshot.
