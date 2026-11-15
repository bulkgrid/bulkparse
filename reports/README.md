# Benchmark evidence

Evidence supports the published comparisons and current parser regression checks:

- [Mixed documents](mixed-200-2026-11-13/README.md): 200 inputs, 19 formats, eight engines; coverage, speed and memory. Quality review is pending.
- [PDF and OCR](broad-benchmark-2026-11-12/README.md): annotated PDF extraction and a separate multilingual OCR comparison.
- [Current BulkParse validation](table-memory-2026-11-15/README.md): table allocation profiles and three fresh-worker checks without warm-up on AC power, with 200/200 unchanged Markdown and JSON outputs. The README displays the best of the three ordinary-output runs.

These directories contain input manifests, settings, versions, hashes, timing records and scored measurements. Authored OCR inputs are included. Downloaded documents, extracted third-party text, model weights, binaries, duplicate source snapshots and obsolete runs are excluded.

## Reproduction and regression checks

Follow the [maintained benchmark guide](../benchmarks/README.md) for the mixed-document, PDF and OCR runners and reusable before/after checks. Superseded experiment scripts have been retired; measurements and hashes in published reports remain unchanged. Paths inside historical evidence are provenance, not promises that old scripts or source snapshots are distributed.

Current native OCR validation is retained in [migration](native-ocr-migration-2026-11-14/), [refinement](native-ocr-refinement-2026-11-14/) and [accent regression](native-ocr-accent-fix-2026-11-14/). Fresh results belong in a new ignored report directory.
