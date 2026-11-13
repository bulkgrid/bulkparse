# Benchmark evidence

Only evidence supporting the current README comparisons is included:

- [Mixed documents](mixed-200-2026-11-13/README.md): 200 inputs, 19 formats, eight engines; coverage, speed and memory. Quality review is pending.
- [Current BulkParse validation](biff-legacy-2026-11-13/README.md): the native row used in that comparison, 200/200 nonempty outputs.
- [PDF and OCR](broad-benchmark-2026-11-12/README.md): annotated PDF extraction and a separate multilingual OCR comparison.

These directories contain input manifests, settings, versions, hashes, timing records and scored measurements. Authored OCR inputs are included. Downloaded documents, extracted third-party text, model weights, binaries, duplicate source snapshots and obsolete runs are excluded.

## Historical tools

All benchmark and regression scripts remain available. Some historical scripts require regenerated inputs, reference checkouts or baseline runs that are not distributed. Their presence does not imply that their old result directories are included. Follow the current protocols linked above for the published comparisons. Fresh benchmark output belongs in a new ignored `reports/` directory.
