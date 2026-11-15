# Spreadsheet performance and regression validation

November 14, 2026. Apple M3 Max, 36 GiB RAM. OCR disabled. General parsing rules only; no document IDs, filenames or corpus-specific content are used in the implementation.

ODS avoids reparsing validated single-text paragraphs and copying paragraph buffers. XLSX/XLSM borrows temporary XML values and metadata while constructing row records, avoids allocating ordinary cell addresses, and resolves each cell style index once. Rich cells, styles, formulas, merged cells, limits and the bounded worksheet/shared-string spool retain their existing behavior. No new runtime dependency or resource-limit increase.

## Before and after: all 200 documents

One excluded warm-up and one measured pass per build and output mode, sequential documents in a persistent worker. Version order alternates between warm-up and measurement. Times include file reads, parsing and Markdown rendering, excluding process startup and response transport. Peak RSS includes initialization, transport and warm-up.

| Mode | Before seconds | After seconds | Before CPU seconds | After CPU seconds | Before peak MiB | After peak MiB | Outputs |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Structured parse + Markdown | 17.309 | 14.079 | 17.071 | 13.944 | 2,223.7 | 2,142.4 | 200/200 before and after |
| Streamed Markdown | 17.038 | 14.321 | 16.961 | 14.261 | 711.3 | 707.2 | 200/200 before and after |

Ordinary-output corpus time decreased **18.7%**, with peak worker RSS **3.7% lower** in this paired run. Streaming time decreased **15.9%**, with peak worker RSS **0.6% lower**. RSS is an operating-system high-water measurement; allocator retention and run order affect it, and these small memory differences are not hard guarantees.

| Format | Before median ms | After median ms | Outputs |
| --- | ---: | ---: | --- |
| ods | 54.35 | 40.83 | 19/19 |
| xlsm | 76.20 | 66.47 | 6/6 |

All 200 ordinary Markdown outputs and all 200 streamed Markdown outputs match the baseline hashes. Status, diagnostics, render diagnostics and available image metadata also match. Complete serialized JSON matches for **69 inputs**: all 66 ODS/ODT/ODP/XLSX/XLSM documents in the mixed corpus plus three generated plain, rich-text and merged-cell stress cases. JSON size, SHA-256 and syntax were checked; its verification time is excluded from parsing time.

The 290 core tests, six benchmark-maintenance tests, formatting and strict Clippy checks passed. The core WebAssembly build passed with four existing dead-code warnings. Equality means no detected output regression, not perfect source fidelity or a new quality score.

## Fresh identical-input comparison

The same runner measured both engines on every ODS and XLSM file from the frozen corpus: 25 inputs, one excluded warm-up, one measured pass. BulkParse returned 25/25 nonempty outputs; AnyDoc returned 21/25. Failures remain in the records. Common-input timing is limited to the same successful files for both tools.

| Format | BulkParse outputs | AnyDoc outputs | Common inputs | BulkParse median ms | AnyDoc median ms | BulkParse common total s | AnyDoc common total s |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| ods | 19/19 | 16/19 | 16 | 16.23 | 14.79 | 3.132 | 3.461 |
| xlsm | 6/6 | 5/6 | 5 | 39.07 | 39.10 | 0.801 | 0.644 |

**Remaining gaps:** XLSM is effectively tied at the median, but BulkParse still takes about 24% longer over the five shared XLSM files together. ODS remains about 10% slower at the median, while taking about 9% less time over its 16 shared files together. These are different summaries of a small sample; neither supports a blanket speed-parity claim.

On this 25-input workload, worker lifetime peak RSS was 1,521.5 MiB for BulkParse and 2,379.6 MiB for AnyDoc. Both attempted all 25 files, but completed different workloads. This comparison does not measure extraction quality.

## Evidence and reproduction

[Summary](summary.json) · [Mixed-corpus measurements](mixed-measurements.json) · [Complete JSON checks](structured-measurements.json) · [Comparison measurements](comparison-measurements.json) · [Build identities and source hashes](identity.json) · [Frozen 200-document manifest](../mixed-200-2026-11-13/corpus-manifest.json). Diagnostics and image metadata are retained as complete-object hashes to keep the report compact; extracted third-party text, binaries and intermediate experiments are excluded.

Preserve a baseline `office_bench` built from revision `1f758df54b9664d89844c4b2e6308591f55290eb`, then build this implementation. Verify candidate source hashes against `identity.json`. With comparator environments provisioned as described in the maintained benchmark guide:

```sh
python3 scripts/fetch_mixed200.py
cargo build --release --locked -p bulkparse --example office_bench
python3 scripts/benchmark_regressions.py \
  --baseline-binary /absolute/path/to/baseline-office-bench \
  --modes default stream --warmup --out reports/new-spreadsheet-mixed
python3 scripts/generate_spreadsheet_stress.py --out target/spreadsheet-speed/stress-final
python3 scripts/benchmark_regressions.py \
  --baseline-binary /absolute/path/to/baseline-office-bench \
  --manifest reports/spreadsheet-speed-2026-11-14/structured-manifest.json \
  --modes json --out reports/new-spreadsheet-json
python3 scripts/benchmark_mixed200.py --engines bulkparse anydoc \
  --formats ods xlsm --warmup --out reports/new-spreadsheet-comparison
python3 scripts/check_mixed200.py reports/new-spreadsheet-comparison
```

Use fresh output paths; the stress generator intentionally refuses to overwrite existing inputs. Measurements require macOS for worker CPU/RSS accounting. These diagnostic runs are not five-pass timing studies, and the corpus has informed development. The README’s overall and per-format BulkParse cells use the ordinary-output pass from this 200-document validation. Other engines retain their November 13 measurements without a full-corpus warm-up; the README discloses the differing dates and warm-up settings. The separate 25-input comparison above is not substituted into the mixed-corpus table.
