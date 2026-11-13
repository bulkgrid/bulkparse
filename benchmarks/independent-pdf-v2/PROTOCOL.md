# Independent PDF study v2 — frozen before parser outputs

The parser versions are the source snapshots in freeze.json: before the reading/
heading improvement series, the previous release, and the current release.
Do not change parser code or tune rules against this corpus during this study.
The test sample becomes observed after this evaluation and must not be reused
as an unseen holdout after fixes.

Selection uses the official DocLayNet 1.0.0 test split. Choose at most ten pages
per category, one page per original source document, in seeded SHA256 order.
Seven eligible manual documents are available, so that category contributes
seven: 57 pages total before overlap checks. This availability amendment was
made from metadata before any parser outputs. Exclude source-document filename
matches and byte-identical matches to every PDF already in the workspace.
Also compare independent native-cell text against existing reference/extracted
text before running parsers: flag 5-token shingle containment >= 0.8 (at least
50 shared shingles) as probable page overlap, and exclude it without replacement.
Report excluded pages and don't change the selection to improve outcomes.
Hash absence cannot establish that all original documents or near duplicates
are unrelated; source-name and text checks provide additional evidence only.

References: human COCO Section-header and Title boxes, with text reconstructed
from the independently provided native text cells whose centres fall in each
box. Use precedence zero only. Sort cells top-to-bottom, then left-to-right.
No headings are authored from outputs of any tested parser. Empty-text heading
regions remain unmatched false negatives and are counted separately.

Heading metrics: normalize NFKC, case-fold and remove non-alphanumeric characters.
Use one-to-one maximum-cardinality matching, with exact matches as the strict
measure and difflib character similarity >=0.90 as the primary tolerant measure.
Count all emitted heading blocks, including on pages without annotated headings.
Report micro precision, recall, F1, per-page counts and category breakdowns.
This evaluates heading detection/content, not heading levels, and is not MHS.
Use paired document bootstrap (2,000 draws, fixed seed) for the change in micro F1.

Text: report token multiset recall against the provided native-cell text (NFKC,
case-fold, Unicode alphanumeric tokens). This is an independent extractor's
silver reference, not a human transcript. Also compare exact per-page native
character multisets between versions with normalization disabled, inspect added/
removed characters and verify inspection JSON and statuses. Retain failures.

Order: report an explicitly limited within-region adjacent-line pair recall,
using exact normalized unique text-cell anchors of >=20 alphanumeric characters
inside Text, List-item and Caption regions. Both anchors must be present in the
right order; missing anchors count as failures. This is geometric/silver evidence,
not a global reading-order annotation. Visually review every page whose heading
set, order or native-character content changes, using the original rendered page
and labelled boxes. Record uncertain decisions; do not alter supplied labels.

Tables: compare actual table structures and extracted cell text between versions
and review changes. DocLayNet supplies table regions, not cell-structure gold:
do not report TEDS or claim structural table accuracy from this dataset.
No combined NID/TEDS/MHS or cross-corpus overall score is invented.

Performance: one persistent Rust worker per version, OCR off, no models, Unicode
normalization enabled for quality/timing, one excluded complete-sample warm-up,
five sequential complete-sample passes per version, rotating version order.
Reuse the existing file-to-file worker timer (reads, parse/render/writes included;
build/startup/transport/hash/scoring excluded). Compare all prediction hashes.
No CPU pinning, fsync, cache flush or thermal isolation. All failures retained.

Publish source/sample/evaluator hashes, protocol, selection, outcomes and review
notes. Keep downloaded source PDFs/images and raw extracted content under target;
provide download instructions and hashes rather than repackaging source material.
