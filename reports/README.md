# Benchmark evidence

Only evidence supporting the current README comparisons is included:

- [Mixed documents](mixed-200-2026-11-13/README.md): 200 inputs, 19 formats, eight engines; coverage, speed and memory. Quality review is pending.
- [Current BulkParse validation](biff-legacy-2026-11-13/README.md): the native row used in that comparison, 200/200 nonempty outputs.
- [PDF and OCR](broad-benchmark-2026-11-12/README.md): annotated PDF extraction and a separate multilingual OCR comparison.

These directories contain input manifests, settings, versions, hashes, timing records and scored measurements. Authored OCR inputs are included. Downloaded documents, extracted third-party text, model weights, binaries, duplicate source snapshots and obsolete runs are excluded.

## Reproduction and regression checks

Follow the [maintained benchmark guide](../benchmarks/README.md) for the mixed-document, PDF and OCR runners and reusable before/after checks. Superseded experiment scripts have been retired; measurements and hashes in published reports remain unchanged. Paths inside historical evidence are provenance, not promises that old scripts or source snapshots are distributed.

Current native OCR validation is retained in [migration](native-ocr-migration-2026-11-14/), [refinement](native-ocr-refinement-2026-11-14/) and [accent regression](native-ocr-accent-fix-2026-11-14/). Fresh results belong in a new ignored report directory.
