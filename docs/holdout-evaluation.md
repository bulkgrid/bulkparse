# Frozen holdout evaluation — 2026-11-10

Follow-up: [ruled tables and Unicode options](pdf-tables.md) address some findings below. This report preserves the original frozen parser results; its corpus is now regression data.

BulkParse's positioned-text improvements generalize to several new column, font and rotation cases, but the holdout exposes clear gaps in table structure, Unicode output and Chinese text. Live comparisons with AnyDoc and Docling are now available. **The Rust parser was not changed during this evaluation.**

The final run contains 24 PDFs, three repetitions for each of three engines: 216 full-process conversions. The 12 external files were selected from a pinned PDFPlumber tree before observing conversion output. Twelve separately labeled synthetic cases were produced with CoreText/CoreGraphics from original expected text. No input hash matches the previous 173-file development corpus. This is a small test set, not representative production coverage or a permanently unseen benchmark.

## Profiles and completion

| Engine | Executed profile | Documents with successful exit in all three runs |
|---|---|---:|
| BulkParse 0.1.0 | Native positioned text; no OCR or table reconstruction | 24/24 |
| AnyDoc 0.2.4 / pdf-inspector 1.14.2 | Default native PDF conversion, including table heuristics; no OCR | 21/24 |
| Docling 2.150.0 / docling-parse 7.23.0 | Heron learned layout, CPU, four configured threads; OCR and table recognition off | 24/24 |

Successful exit does not mean complete content. BulkParse PDFs still report partial status. BulkParse and Docling returned empty text for the scan-only document and retained only the native page from the mixed native/scan document. AnyDoc returned an OCR-needed error for both and for the external `image_structure` case. These failures remain in the quality denominator; their fast error exits are not counted as successful conversion timings. Recorded outputs were deterministic across all three repetitions, including consistent failures.

These profiles deliberately avoid OCR. Docling's table recognition is disabled, so this is **not a comparison of each project's maximum table quality**. AnyDoc's default table behavior is included and is useful evidence of a capability BulkParse lacks.

## What the new cases show

| Authored case | BulkParse | AnyDoc | Docling profile |
|---|---|---|---|
| Two columns | All text; 100% intended block-pair order | All text; 78.6% order | All text; 78.6% order |
| Three columns | All text; 100% order | All text; 72.7% order | All text; 72.7% order |
| Spanning title | All text; 100% order | All text; 90.0% order | All text; 90.0% order |
| Narrow gutter | All text; 100% order | All text; 83.3% order | All text; 83.3% order |
| Ruled table | All words, column-first paragraphs; no table | Exact expected Markdown cells and rows | Row-first text; no Markdown table with table recognition off |
| Font subsets | 100% token recall | 100% token recall | 58.3% token recall |
| Quarter-turn text | All text and intended order | All text and intended order | All text and intended order |

Block order here is a strict check against authored intended blocks, with missing blocks counted against recall. These are deliberately simple synthetic layouts; an aligned layout can be ambiguous without the author's intent. This metric does not establish general newspaper or table reading-order accuracy.

The Arabic/Latin case needs careful interpretation:

| Metric | BulkParse | AnyDoc | Docling |
|---|---:|---:|---:|
| Literal token recall against authored text | 25.0% | 62.5% | 68.8% |
| Supplemental NFKC-normalized token recall | 68.8% | 62.5% | 68.8% |

BulkParse retains Arabic presentation-form characters, which account for much of its literal-score penalty. Normalization is a missing output-quality decision, not an OCR problem. Some remaining differences involve Persian yeh versus Arabic yeh in the generated PDF's character mappings; PDFKit reproduces these too. The extraction pipeline cannot safely infer every original character from an ambiguous mapping. The normalized metric was added **after** observing this case and is reported alongside the original metric, not substituted for it. No parser output was repaired during scoring.

## External evidence and reference limits

One external document has an upstream saved text reference. The other eleven use independent Apple PDFKit extraction as a **silver reference**, not human-verified truth. Agreement with PDFKit is not accuracy, and tokenization is particularly coarse for unspaced Chinese text. Do not average these results with the authored cases into one quality score.

| External case | Reference | BulkParse token recall | AnyDoc | Docling |
|---|---|---:|---:|---:|
| Supreme Court transcript, first page | Upstream saved text | 100.0% | 98.2% | 92.0% |
| Federal Register, 15 pages | PDFKit silver | 99.9% | 97.1% | 97.1% |
| Chinese three-column page, `issue-1147-example` | PDFKit silver | 73.2% | 77.5% | 87.3% |
| Financial statement, `issue-912` | PDFKit silver | 95.7% | 100.0% | 99.6% |

The last two rows are concrete cases where BulkParse trails the competitors on this diagnostic. They belong in the next investigation queue. The financial statement score does not verify that amounts are attached to the correct account or row. The Chinese score is not a percentage of characters correctly recovered. Full per-case results, including cases where Docling drops rotated/annotation content and AnyDoc rejects image-bearing content, are retained in the machine-readable report.

The generated fixture contact sheet and selected external pages were visually reviewed. This checks that the fixtures render as intended; it is not a full human audit of all conversion outputs.

## Resource observations

Medians of three full CLI processes on Apple Silicon macOS are below. There were no timed warmups; engine order rotated per repetition. Each Docling process imports Python packages and loads its model. These numbers **must not be used as persistent-worker throughput or core-library speed ratios**. There is no hard memory cap on macOS.

| Input | Engine | Wall time | Peak resident memory |
|---|---|---:|---:|
| Original prose | BulkParse | 4.41 ms | 3.30 MiB |
| Original prose | AnyDoc | 6.57 ms | 10.02 MiB |
| Original prose | Docling | 4,320.02 ms | 1,165.47 MiB |
| Court transcript | BulkParse | 12.51 ms | 16.81 MiB |
| Court transcript | AnyDoc | 17.56 ms | 25.34 MiB |
| Court transcript | Docling | 4,400.83 ms | 1,169.17 MiB |
| Federal Register, 15 pages | BulkParse | 21.24 ms | 9.09 MiB |
| Federal Register, 15 pages | AnyDoc | 28.27 ms | 17.12 MiB |
| Federal Register, 15 pages | Docling | 7,321.74 ms | 2,754.61 MiB |

BulkParse used less resident memory than both configured competitors on each mutually successful case in this set. It was faster on most such cases; on `issue-598-example`, both native engines took approximately 10.3 ms. The small sample and differing capabilities prevent a general superiority claim. Docling's learned-layout startup explains much of its per-document cost in this process-per-file setup. CPU measurements and individual samples are included in the report.

The measured native CLI artifacts were 3,436,704 bytes for BulkParse and 7,023,152 bytes for AnyDoc. Those binary sizes are not comparable with the size of Docling's Python executable alone: its installed packages and model are additional runtime dependencies. No cross-platform package-size claim is made.

## Reproduction and integrity

The [holdout protocol](../benchmarks/holdout/README.md) records selection, restoration, provisioning and interpretation. The final report directory (historical artifact; not distributed) contains:

- `run.json`: versions, explicit profiles, executable/model/environment hashes and measurement scope.
- `samples.jsonl` and `summary.json`: all 216 observations and 72 per-case/engine summaries, including failures.
- `provenance.json`: unchanged parser fingerprints, input/reference checks and AnyDoc source fingerprints.
- `harness/`: exact evaluation and adapter source snapshots used by this run.

Twelve Python harness tests pass. They cover failure scoring, block order, table structure, compatibility normalization and Python virtual-environment routing, alongside the earlier evaluation/benchmark checks. The existing Rust parser's 41-test validation remains the previous milestone's record; no Rust changes required a new test run here.

The original external documents, their reference text, model weights, installed engines and raw converted text are excluded from the source bundle. Original synthetic PDFs and references are included. The corpus is now observed; future fixes must use a fresh holdout to measure generalization.

## Next implementation priorities

1. Reconstruct simple PDF tables into the shared structured model, preserving cell boundaries and row/value associations. Keep word recovery and table structure as separate checks.
2. Define Unicode normalization policy and preserve original text/provenance when normalization is lossy. Investigate the Chinese page and ambiguous font mappings without document-specific replacements.
3. Add persistent-worker benchmarks that initialize each library once, then measure document conversion and bounded concurrency. This is the next useful evidence for Bulkgrid server sizing.
4. Integrate a real optional OCR provider and evaluate scans separately; run Docling's OCR/table-enabled profiles before comparing maximum conversion quality.

New formats can follow once these quality gaps have regression coverage. Nothing in this evaluation establishes a fastest or 10x claim.
