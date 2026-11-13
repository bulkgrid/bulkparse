# Current BulkParse mixed-document validation

**200/200 nonempty outputs across 19 formats.** Apple M3 Max, 36 GiB RAM; November 13, 2026. OCR disabled. The README uses the current-build diagnostic pass, with sequential reads, conversion and Markdown rendering. Startup and response transport are excluded from timing; worker lifetime RSS includes initialization and transport.

| Measurement | One diagnostic pass |
| --- | ---: |
| Nonempty outputs | 200/200 |
| Median successful conversion | 2.93 ms |
| Full corpus time | 16.359 s |
| CPU time | 16.326 s |
| Peak worker RSS | 1,883.6 MiB |

[Per-document records](diagnostic/after-0.json) · [Run settings](diagnostic/identity.json) · [Manifest](diagnostic/manifest.json) · [Summary](diagnostic/summary.json) · [Validation record](verification.json).

The verification includes all 200 streamed/in-memory Markdown comparisons and full JSON checks for the XLS files. [Independent workbook-value checks](workbook-values.json) cover 466 nonempty cells. Nonempty output does not establish complete fidelity, and mixed-corpus quality review is unfinished.

The summary also records matched control aggregates. Older intermediate runs, executable binaries, extracted document text and duplicate source snapshots are not distributed. The validation record's source-match statement describes the measured build at validation time, not future edits to this checkout.

## Reproduce current behavior

```sh
python3 scripts/fetch_mixed200.py
cargo build --release --locked -p bulkparse --example office_bench
python3 scripts/benchmark_mixed200.py \
  --manifest reports/mixed-200-2026-11-13/corpus-manifest.json \
  --engines bulkparse --out reports/new-bulkparse-run \
  --bulkparse-binary target/release/examples/office_bench
```

The benchmark harness requires macOS. The parser itself is not restricted to macOS.
