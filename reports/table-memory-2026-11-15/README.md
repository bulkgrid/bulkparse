# Table allocation and worker-memory check

November 15, 2026. Apple M3 Max, 36 GiB RAM, macOS, AC power throughout. The frozen 200-document corpus, input order, image extraction policy and resource limits are unchanged. OCR is disabled.

## Result

The parser now builds unmerged spreadsheet tables directly into their final rows instead of constructing a second tree of allocated paragraphs. XLSX worksheet XML, metadata DOM and row-storage buffers are released before the final table is allocated. These rules apply to any matching table structure; no document identifiers or corpus-specific conditions are used.

All 200 documents still return output. Markdown, complete structured JSON, status, diagnostics and image metadata match the baseline. Three generated spreadsheet stress cases also match in ordinary, streaming and JSON modes. This establishes output equivalence on these inputs, not perfect extraction quality.

The main README displays the **best of three candidate checks**: the third run had both the lowest worker peak RSS and the fastest corpus time. Its overall row and every per-format cell come from that same complete pass; metrics are not selected from different runs. Other engines retain their separate single-pass measurements. This selection policy differs from those engines and should not be treated as a controlled statistical ranking.

| Displayed candidate run | Outputs | Formats | Median document ms | Corpus seconds | CPU seconds | Peak RSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Third check | 200/200 | 19/19 | 3.26 | 13.843 | 13.739 | 1,693.5 |

## All ordinary-output checks

Each row below is a fresh pair of persistent workers processing the same documents sequentially, once each, without warm-up. Thread controls for RAYON, OMP, OPENBLAS and MKL are all one. Worker startup and response transport are excluded from conversion time; lifetime RSS includes both. File caches were not flushed. No other performance jobs ran concurrently.

| Check | Order | Baseline seconds | Candidate seconds | Baseline peak MiB | Candidate peak MiB |
| --- | --- | ---: | ---: | ---: | ---: |
| First | Baseline first | 13.733 | 13.960 | 1,971.1 | 1,824.7 |
| Second | Candidate first | 14.117 | 14.128 | 1,865.4 | 2,123.2 |
| Third | Baseline first | 13.941 | 13.843 | 1,941.2 | 1,693.5 |
| Median of the three checks | — | 13.941 | 13.960 | 1,941.2 | 1,824.7 |

The measured median RSS is 6.0% lower; median corpus time differs by +0.14%. However, candidate RSS ranges from 1,693.5 to 2,123.2 MiB, and the second pair is worse. The measurements do **not** establish a guaranteed reduction in worker peak memory or prove the historical memory increase has been eliminated. The best run alone must not be used to make that claim.

The first check additionally verified all 200 documents through streaming Markdown and complete JSON serialization:

| Mode | Baseline seconds | Candidate seconds | Baseline peak MiB | Candidate peak MiB | Changed outputs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Streaming Markdown | 14.660 | 14.357 | 570.5 | 590.6 | 0 |
| Structured result with JSON verification | 14.122 | 13.945 | 1,948.8 | 1,922.3 | 0 |

JSON serialization, syntax validation and hashing run outside conversion timing but remain included in lifetime RSS. The streaming measurement includes output readback for verification in RSS. Its single observed peak is slightly higher; these checks establish no output regression, not a universal memory improvement for every API mode.

## What drives memory

Instrumented runs distinguish live Rust allocations from macOS allocator reservations and worker RSS. After each document is released, tracked live allocations return to at most 17,992 bytes, including profiling bookkeeping. Allocator reservations can remain around 2 GiB. Large spreadsheets create many short-lived allocations, and retaining or fragmenting those allocations contributes to the gap between live data and whole-worker RSS. These observations do not isolate the cause of the precise historical difference between earlier benchmark runs.

The largest XLSM document retains about 558.3 MiB in its complete structured result. Its parser live-allocation peak changes only from 581.01 to 580.81 MiB. The maximum live peak including Markdown and benchmark transport remains about 616.1 MiB. Returning the full in-memory document still has a substantial cost.

Across the full corpus, the accepted changes eliminate **169,993,064 cumulatively allocated bytes (162.1 MiB)** and **445,317 allocation calls**. This is allocation work avoided, not 162.1 MiB less peak memory. Instrumentation changes allocation timing, so instrumented timings and RSS are not substituted into the normal benchmark results.

Exploratory variants that reserved merged rows differently were rejected: one reduced the largest ODS result's live storage by about 69 MiB, but both variants increased ordinary-worker RSS in both of their paired checks. Only direct unmerged-table construction and earlier worksheet-buffer release remain. The exploratory summaries are retained to avoid presenting only favorable trials.

## Validation and reproduction

The baseline is the parser at revision `d433537a86a9701a41f741bcc553edf5cb9e402c`. Candidate source hashes and exact executable identities are recorded alongside the results. The benchmark protocol itself was not changed.

Validation: 291 core tests, six benchmark-maintenance tests, strict Clippy, formatting and the WASM build check passed. The WASM check emits five dead-code warnings. The new table test compares sparse coordinates, empty cells, Unicode, block identifiers and decoration ordering against the original structured builder. Existing tests cover limits, cancellation, recovery, merged layouts and streaming behavior.

Preserve a release `office_bench` binary from the baseline, then build the candidate and run on AC power:

```sh
cargo build --release --locked -p bulkparse --example office_bench --example allocation_profile
python3 scripts/benchmark_regressions.py \
  --baseline-binary /absolute/path/to/baseline-office-bench \
  --candidate-binary target/release/examples/office_bench \
  --reverse-order --out /absolute/path/to/new-check
```

The default verifies ordinary, streaming and JSON modes. Repeat with new output directories and `--modes default`, alternating the presence of `--reverse-order`, for additional fresh-worker ordinary-output checks. Do not add `--warmup`. Sample power throughout each run. macOS is required to reproduce the recorded resource counters.

Generate independent large-sheet stress inputs with `scripts/generate_spreadsheet_stress.py --out /absolute/path/to/new-stress`; pass its `stress-manifest.json` to the same comparison script with `--manifest`.

For allocation diagnostics, feed the absolute corpus paths, in manifest order and one per line, to `target/release/examples/allocation_profile --structured`. Keep all four thread controls set to one. Never use that instrumented binary for speed claims.

## Evidence

- [Summary and displayed-run selection](summary.json), [candidate source identity](source-identity.json), [power samples](power.json), [test results](checks.json).
- [First check and all three API modes](early-release-validation/summary.json), [per-document measurements](early-release-validation/measurements.json), [executable identity](early-release-validation/identity.json).
- [Second check](early-release-reverse/summary.json), [measurements](early-release-reverse/measurements.json).
- [Third check used in the README](early-release-third/summary.json), [measurements](early-release-third/measurements.json).
- [Stress-case checks](early-release-stress/summary.json), [measurements](early-release-stress/measurements.json), [stress manifest](stress-manifest.json).
- [Allocation profiles](allocation-profiles.json), [instrumented resource log](allocation-profile-resources.log), [exploratory checks](exploratory-checks.json).
- [Frozen 200-document corpus](../mixed-200-2026-11-13/corpus-manifest.json).

Per-document records retain Markdown and complete-JSON hashes where applicable. Diagnostics and image metadata are represented by hashes of canonical JSON. Input documents and binaries are not redistributed here. Resource logs are stored beside each check.
