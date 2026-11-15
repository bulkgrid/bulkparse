# Benchmarks and regression checks

Run commands from the repository root. Results go into a **new** directory; published evidence stays unchanged. Python is development tooling, not a parser runtime dependency.

## Current comparisons

| Comparison | Runner | Protocol and inputs |
| --- | --- | --- |
| Mixed documents, overall and per format | `scripts/benchmark_mixed200.py` | [200 documents, 19 formats, eight engines](realworld/README.md) |
| PDF extraction: overall, NID, TEDS, MHS | `scripts/benchmark_broad.py` | [200 PDFs, nine profiles](broad/README.md) |
| OCR alternatives: CER, WER, speed, CPU, memory | `scripts/benchmark_ocr_alternatives.py` | [24 authored cases, four published profiles](ocr-alternatives/README.md) |
| Integrated Rust OCR and scanned layouts | `scripts/benchmark_native_migration.py`, `scripts/check_native_scans.py` | [Bindings and validation](../docs/ocr-bindings-validation.md) |

The mixed-document overview and format breakdown use the same run. There is no separate Office-only benchmark to maintain. The mixed corpus measures coverage and performance; it has no published whole-document quality score. PDF and OCR scores measure different tasks.

### Mixed documents

Provision comparator environments and models using the linked protocol, then:

```sh
python3 scripts/fetch_mixed200.py
cargo build --release --locked -p bulkparse --example office_bench
python3 scripts/benchmark_mixed200.py --out reports/new-mixed-run
python3 scripts/check_mixed200.py reports/new-mixed-run
python3 scripts/report_mixed200.py reports/new-mixed-run --out reports/new-mixed-run/table.md
```

The default manifest is the frozen 200-document corpus, including all 27 original failure cases. Start with one pass. Add `--warmup --passes 5 --final-validation` only for a repeated timing study. `--engines bulkparse` needs no competitor environment; `--smoke` exercises the smallest file in each format and is not a full benchmark. Use `--formats ods xlsm` for a targeted performance check; it keeps every document in those formats and labels the selected subset. The report writer accepts fresh runs and never edits the README.

### PDF and OCR

After provisioning and building the workers described in their protocols:

```sh
target/opendataloader-venv/bin/python scripts/benchmark_broad.py --out reports/new-pdf-run
python3 scripts/benchmark_ocr_alternatives.py --profiles rapid-small lite-tiny paddle-tiny tesseract-fast --out reports/new-ocr-run
```

PDF and OCR retain one excluded warm-up and five sequential measured corpus passes. The PDF runner writes scores, timings and memory into `summary.json`; the OCR runner records per-profile evidence and summaries. The single-engine `benchmark_opendataloader.py` remains available for faster PDF development checks. Do not run performance suites concurrently. Mixed/PDF memory accounting requires macOS; the OCR alternatives run inside Linux containers. Models and corpus downloads are not included in the repository.

## Regression checks

### Fast checks

```sh
cargo test --locked -p bulkparse
python3 -m pip install -r benchmarks/requirements-test.txt
python3 scripts/run_harness_tests.py
```

The Python tests check evaluator accounting, worker deadlines, failure denominators and report validation. Optional evaluator/model artifacts are required for some suites; skipped tests are reported. Native OCR checks and binding parity commands are documented in [OCR validation](../docs/ocr-bindings-validation.md). `check_binding_parity.mjs` checks Rust CLI, Node, Python and WASM results after building those bindings.

### Before/after output, speed and memory

Build and copy `target/release/examples/office_bench` **before** changing the parser, then rebuild the candidate. Supply those two executables explicitly:

```sh
python3 scripts/benchmark_regressions.py \
  --baseline-binary /absolute/path/to/baseline-office-bench \
  --candidate-binary target/release/examples/office_bench \
  --out reports/new-regression-run
```

This defaults to the same 200 documents and checks default Markdown, streamed Markdown and full structured JSON serialization. It compares Markdown hashes, status, diagnostics and image metadata; successful JSON outputs must validate and match in complete serialized size and SHA-256 hash. It records conversion time and worker lifetime peak RSS separately for each build/mode. Timing excludes JSON verification; its memory is included. JSON mode compares the complete serialized model through its hash; serialization timing is not part of equality. Expected failures remain failures, and equality is not proof of fidelity. Intentional output changes need review; the script never refreshes expected results automatically.

Use `--passes 5 --warmup` for steadier timing, or `--modes stream` for a targeted check. Arbitrary frozen JSON manifests with `id`, `path`, `sha256` and `format` can be supplied through `--manifest`, including `benchmarks/realworld-failures/manifest.json`. Retained historical expectations can also be checked with `check_realworld_failures.py`; they may flag intentional later improvements.

Generated spreadsheet stress cases supplement the real-world files:

```sh
python3 scripts/generate_spreadsheet_stress.py --out target/new-spreadsheet-stress --scale 1
python3 scripts/benchmark_regressions.py \
  --baseline-binary /absolute/path/to/baseline-office-bench \
  --manifest target/new-spreadsheet-stress/stress-manifest.json \
  --out reports/new-spreadsheet-regression
```

Increase `--scale` for larger plain, rich-text and merged-cell worksheets. These authored cases are not included in the mixed-document leaderboard.

For exact structured-model equality, `check_structured_output.py` accepts the same manifest and explicit baseline/candidate **CLI** executables (not `office_bench`). It compares complete JSON bytes, exit codes and error output, using temporary files to avoid loading large outputs into the harness's memory. Add `--include-recovery` to cover both ordinary and bounded recovery output. Its `--out` is a JSON file.

### Targeted quality checks

- `check_complete_csv_output.py` and `check_realworld_csv_content.py`: source-row preservation, with encoding limitations explicit.
- `check_pdf_visible_text.py`: before/after native text accounting on a supplied PDF directory.
- `evaluate_independent_pdf.py`: DocLayNet heading, text and order checks using explicit baseline/candidate CLI executables and the frozen `independent-pdf-v2` inputs. Restore inputs with `doclaynet_sample.py` and the frozen selection; the corpus has since informed development.
- `benchmark_icdar_tables.py`: table structure/content scoring using the frozen ICDAR selection, explicit builds and source manifests.
- `evaluate_fixtures.py`, `evaluate_holdout.py`, `evaluate_advanced_pdf.py`, `check_broad_holdout.py`: retained structural/reference evaluators; use `--help` and their committed manifests for required inputs.
- `report_diagnostic_categories.py RECORDS --out FILE`: summarize missing-content risks, formatting limitations and notices without treating warnings as quality scores.

## Maintenance

`benchmark_support.py` contains the shared process protocols and executable paths. Engine adapters remain separate so their effective configuration is inspectable. Each active runner records input/build hashes and preserves failures in accounting.

One-off optimization runners, old packaging scripts and hard-coded report writers were removed. Their replacement is the generic before/after runner and the current comparison runners above. Published JSON measurements, corpus manifests and source hashes remain immutable provenance; paths inside historical evidence can name retired tooling. New results must be reviewed before updating public comparison tables.
