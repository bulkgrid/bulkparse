# PDF quality fixes — 10 November 2026

BulkParse now recovers conservative font-size headings, expands Arabic presentation forms by default, and preserves repeated dense numeric rows. These address the native-conversion gaps found in the [Firecrawl comparison](pdf-inspector-comparison.md). They do not add OCR, models, dependencies, network calls or document-specific text rules.

Validation below applies to core-source digest `479e440ad8d958beaaf28c501c6c96a93312ccb22e17cfa15495fc0e6c9bc7a0`. A separate edit adding vertical-edge handling to `rules.rs` appeared after these checks. It was preserved and is not covered by these results; the final workspace formatting check detected unformatted code in that file. Revalidate the combined workspace after that work is finished.

## Changes

- **Headings:** short, isolated lines substantially larger than body text become shared JSON heading blocks and Markdown headings. Levels follow relative sizes on each page. Repeated body sizes and tightly spaced prose protect footnote-heavy pages; uniform-size lines, drop caps, single-letter diagram labels, sentence endings and hyphenated continuations are excluded. Table-cell text does not undergo heading inference. Heading text obeys the existing text/node budgets.
- **Numeric rows:** at least three consecutive rows with three aligned numeric cells and a majority of numeric fragments are joined before column ordering. This keeps dense table labels with their values. It does not invent cells or header spans when the existing ruled-table detector cannot establish a safe schema. Prose columns retain column-first ordering.
- **Unicode:** `pdf_normalize_unicode` now defaults to `true`, including when deserializing empty options in bindings. Expansion is restricted to Arabic presentation forms and alphabetic ligatures. `ActualText`, circled digits and fullwidth letters remain unchanged. Real Persian/Arabic letter variants are not rewritten to match a reference.

To retain the former Unicode behavior, use `pdf_normalize_unicode: false` in Rust/Node/WASM, `pdf_normalize_unicode=False` in Python, or `--pdf-normalize-unicode=false` in the CLI. The existing bare `--pdf-normalize-unicode` flag still enables expansion. This is an intentional default-output change.

## Quality evidence

The existing 32-document comparison corpus is now development/regression evidence, not an unseen evaluation set. References include authored text and PDFKit-derived silver text; these measures are not overall semantic or layout accuracy.

| Check | Before | After |
| --- | ---: | ---: |
| Authored Arabic example: literal token recall | 25.0% | 68.75% |
| Dense numeric table: adjacent-word recall | 62.03% | 99.60% |
| Rotated version of that table: adjacent-word recall | 62.03% | 99.60% |
| Authored spanning title | Paragraph | Heading |
| Controlled OCR routing cases | 20/20 | 20/20 |

Both dense-table examples retain 100% token recall. The Arabic result now matches Firecrawl's literal token recall on that fixture; remaining letter-mapping discrepancies persist. The authored newspaper-style column checks remain intact. Another external table's adjacent-word recall improves from 93.89% to 100%.

One external Federal Register document's adjacent-word proxy changes slightly downward, from 88.599% to 88.578%, when its cost-table rows are grouped. This is retained in the report, not hidden. Wrapped cells and multi-line headers there remain imperfect. Inspection results are unchanged.

## Verification

- 94 Rust core/provider/pool tests pass, including new heading, footnote, numeric-row and false-positive cases; 21 tests pass without default core features. All-feature workspace Clippy and formatting pass.
- The unchanged 173-input fixture manifest still yields 47 complete, 116 partial and 10 failed results. The expected malformed/encrypted failures remain; primary PDF lexical regression checks pass.
- All 80 PDFs pass per-page raw-character preservation, existing table-cell preservation, saved-inspector equality and inspection/conversion parity. This check explicitly disables normalization for raw-content comparison; it permits intended order and block-type changes and is not a reading-order score.
- All nine existing authored table fixtures still match their expected structures.
- Rebuilt Node, Python, WASM and CLI match exactly on 15 conversion and three inspection cases, including default normalization, explicit opt-out and a heading fixture. Native binding and WASM smoke checks also pass. Browser UI, wheel publication and cross-platform CI were not rerun.

## Performance

Final paired run: release builds, seven repetitions after warmup, 32 PDFs, one persistent worker per engine and one Rayon thread. OCR is disabled. All 896 calls succeed with deterministic results. Timings exclude file I/O, startup and JSON transport; RSS is the peak over each worker's lifetime.

| Measurement | BulkParse | Firecrawl PDF Inspector |
| --- | ---: | ---: |
| Inspection, median total for 32 PDFs | 43.92 ms | 41.80 ms |
| Conversion, median total for 32 PDFs | 55.51 ms | 433.20 ms |
| Conversion worker peak RSS | 37.52 MiB | 52.34 MiB |
| Conversion, sum of per-file medians excluding two related dense tables | 49.00 ms | 101.27 ms |

The aggregate conversion gap remains about 7.8×, or about 2.1× excluding those two tables. This is specific to the corpus and configurations. Inspection is slightly slower and performs different work: BulkParse also decodes text for quality checks. The old BulkParse conversion aggregate was 54.24 ms in a separate run; these observations do not establish the precise overhead of the new heuristics. The final conversion ranges were 54.25–78.68 ms and 426.40–523.82 ms respectively.

## Limits and reproduction

PDF results remain partial. Font-size heuristics can miss same-size/bold-only headings or confuse larger figure labels with headings; hierarchy is page-local and rotated headings are not inferred. Numeric row grouping is not general table reconstruction. Merged headers, wrapped cells, complex layouts, Chinese/font mapping and independent multilingual OCR evaluation still need work.

Final evidence:

- Native preservation and table regression (historical artifact; not distributed)
- 173-input corpus (historical artifact; not distributed)
- Paired comparison (historical artifact; not distributed)

Earlier trial reports remain historical; the final core-source digests in the regression and comparison reports match. External source documents and converted text remain under local fixture/target paths rather than being copied into reports.

Current checks are documented in the [benchmark guide](../benchmarks/README.md#regression-checks). Use `benchmark_regressions.py` for before/after output comparison and `check_pdf_visible_text.py` for native character accounting. The old experiment-specific runners have been retired.

Before/after checks require explicit baseline and candidate executables; use a fresh report path for each run.
