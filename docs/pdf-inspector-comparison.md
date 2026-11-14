# BulkParse vs Firecrawl PDF Inspector — 10 November 2026

We compared the local BulkParse implementation with the supplied Firecrawl
`legacy/pdf-inspector-main` snapshot, package version 1.25.2. Inspection speed is similar.
BulkParse's native Markdown conversion is faster and uses less memory on this
particular corpus. That is not evidence of universally better conversion quality.

The results below are the original pre-fix baseline. The subsequent [PDF quality fixes](pdf-quality-fixes.md) add headings, enable selective Unicode normalization by default, and correct dense numeric-row ordering; that page records the new benchmark separately.

## Measured results

32 PDFs, seven repetitions after warmup, release builds on this Apple Silicon
Mac. One persistent worker per engine; one Rayon thread. OCR disabled. All pages
inspected. Timings exclude input file reads, startup and JSON transport.

| Measurement | BulkParse | Firecrawl PDF Inspector |
| --- | ---: | ---: |
| Inspect all 32 documents, median total | 41.58 ms | 39.87 ms |
| Median per-document inspection | 0.101 ms | 0.102 ms |
| Inspection worker peak RSS | 34.56 MiB | 33.00 MiB |
| Convert all 32 documents to Markdown, median total | 54.24 ms | 422.20 ms |
| Median per-document conversion | 0.124 ms | 0.286 ms |
| Conversion worker peak RSS | 38.36 MiB | 52.59 MiB |
| Exact inspection routes on 20 controlled cases | 20/20 | 19/20 |
| Exact conversion-time routes on those cases | 20/20 | 18/20 |

All 896 measured library calls completed successfully and predictions were
identical across repetitions. Success here means the API returned a result;
it does not mean a complete or correct document. BulkParse PDF results remain
partial. Memory is the peak over the worker's whole lifetime, not per-document
allocation and not OCR memory.

BulkParse inspection was about 4% slower in aggregate; the median individual
case was essentially tied. Our inspector decodes text to check Unicode quality.
Firecrawl's detection-only path does not do equivalent decoded-text analysis.

Full conversion was **7.8× faster in aggregate**, but two related financial-table
PDFs dominate that gap. With those two removed, the remaining 30-document total
was **46.62 ms vs 98.09 ms**, about **2.1×**. The ratio of median per-document
times is approximately **2.3×**. Neither number establishes a universal speedup,
and Firecrawl performs more heading/table/layout reconstruction.

## What the quality checks found

- BulkParse preserved separate-column reading order on the controlled two-column,
  three-column, spanning-title and narrow-gutter cases; Firecrawl interleaved some
  of their lines. These are small authored regression cases.
- Firecrawl inferred heading structure that BulkParse emitted as ordinary
  paragraphs, and normalized Arabic presentation forms by default. BulkParse's
  then-default preserved those forms; its normalization option was not enabled.
- On the Arabic case, literal token recall was 25% for BulkParse versus 68.75%
  for Firecrawl. Applying the same NFKC comparison to both gives 68.75% each.
  The literal difference is therefore not evidence that BulkParse lost all of
  those words. Neither matches the authored reference completely.
- Both recovered every expected row in the one authored ruled-table case.
  Broader table accuracy is unestablished.
- Firecrawl's word-sequence proxies were better on the two financial-table inputs;
  BulkParse's were better on some other external inputs. Those references are
  PDFKit-derived and are not human-annotated layout gold.
- Firecrawl inspection recommended OCR for both pages of the native-plus-scan
  case, where the authored route is page 2 only. Its full conversion additionally
  recommended OCR for both pages of the native-plus-empty fixture. BulkParse
  matched the predefined routes. These are potentially unnecessary OCR calls,
  not measured OCR costs: no OCR was executed.
- Both identified the new native-header-over-scan fixture as an OCR candidate.

Twenty controlled routing matches do not establish real-world recall or false
positive rates. The corpus includes inputs used during development, correlated
scan variants, and no independent routing labels for the 12 external documents.
We have not benchmarked handwriting, damaged real-world scans, learned layout
models or OCR recognition against Firecrawl here.

## Reproducibility

[Benchmark instructions](../benchmarks/inspector-comparison/README.md) describe
configuration and the inspection capability difference. The final report is
`reports/inspector-comparison-2026-11-10-v2`:

- `manifest.json`: source/reference identities and controlled route expectations.
- `run.json`: compiler, platform, source, binary and lockfile hashes.
- `samples.jsonl`: every timed attempt, result identity and route.
- `per-case.json`: per-document timing, routing and reference-based diagnostics.
- `summary.json` and `resources.json`: aggregates and owned-child resource usage.

The experiment-specific checker has been retired. Use the [current benchmark and regression guide](../benchmarks/README.md) for maintained commands.
