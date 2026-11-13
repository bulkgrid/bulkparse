# Independent table structure test

Frozen before any BulkParse predictions. Use all 67 competition PDFs, no training PDFs. Exact PDF-stem XML only; four unmatched b-variant XML alternatives are excluded. Regions are page-local table units; row/column offsets are normalized to each region origin. Inclusive end indices become spans. Empty grid holes become empty cells. Invalid overlapping cell annotations exclude the document before predictions.

Exclude exact prior PDF hashes or 5-token normalized text containment >=0.8 with >=50 shared shingles against all prior local PDFs and observed DocLayNet native cells. PDFKit performs independent text extraction. No replacements. Failed new overlap checks exclude the document; old extraction failures are reported as an audit limitation. Source-family overlap cannot be completely excluded.

Parse full PDFs with no gold crops or labels, local native engine, OCR off. Before is the saved heading-fields release; after is frozen before this test is run. No tuning on test outcomes. Score all annotated pages; predictions on unannotated pages are reported separately because completeness of annotations outside selected table pages is unknown.

Match predicted and gold table regions within each page by maximum-weight one-to-one assignment on NFKC/casefold token-multiset F1, minimum 0.10. Matching uses text only, never structural similarity. Unmatched tables score zero. Per-page score is sum of matched upstream TEDS (or TEDS-S) divided by max(gold count,predicted count). Primary aggregate weights pages by this denominator. Also report macro page scores, matched-only scores, detection precision/recall, and all per-table pairs. These custom end-to-end aggregates are not comparable numerically with opendataloader overall/TEDS.

Use unchanged pinned opendataloader TEDSEvaluator on canonical HTML, all cells td, escaped content, explicit spans. Do not change cell content or manually repair predictions. Conversion errors/timeouts score zero. Timing uses the saved sequential single-process workers: one warm-up full corpus followed by five full passes, alternating before/after. Report median, all runs, hashes, and hardware; exclude loading and evaluator time consistently with the main study.

Corrected annotations are published under [CDLA-Permissive-2.0](LICENSE) according to the source card; the card says original PDF licensing is unspecified, believed public domain by the original creator. Do not redistribute PDFs in result bundles.
