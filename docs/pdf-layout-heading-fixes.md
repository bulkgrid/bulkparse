# Native PDF layout and heading improvements — 10 November 2026

The bundled Firecrawl pdf-inspector is version **1.25.2**, not the historical
**0.2.6** engine in the July 2026 comparison. Its `markdown/heading.rs`,
`markdown/analysis.rs`, `tables/grid.rs`, and `tables/detect_rects.rs` were
reviewed locally. These sources use repeated visual styles and numbering,
body-relative typography, caption/fragment exclusion, and geometric evidence
for merged cells. The source is MIT licensed; attribution and its complete
license are in [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md).

BulkParse now recognizes font emphasis from native font names/descriptors,
body-size bold section titles, small caps, and repeated smaller sidebar labels.
Physical title continuations are joined before column ordering, including
centred titles and numbered titles with hanging indentation. Guards exclude
captions, wrapped caption continuations, author lists, URLs and many ordinary
bold sentence leads. The sidebar adaptation uses existing glyph geometry,
size, emphasis, repeated distinct labels and displaced position. It does not
copy the upstream classifier wholesale or its font-family/structure-tree logic.

Table changes reconstruct nested unboxed headers over repeated numeric columns,
retain row/column spans, recover a clipped worksheet edge and independent header
labels, preserve explicit blank answer cells, and recover styled prose tables
inside a native bounding rectangle. Row starts tolerate baseline variation and
keep longer paragraphs together. Native spaces remain available to the reading-
order pass so justified text and bullet lists do not become false columns.
Ambiguous L-shaped merges remain rejected. No model, OCR, runtime dependency,
reference text, document ID or saved prediction is used by the parser.

The two remaining native zero-TEDS layouts and worksheet regression drove this
work. The benchmark corpus was therefore used during development; the result
is not an independent holdout. Two image-only table failures still require
image interpretation outside the OCR-off scope. One recovered prose table has
an image-backed internal grid, so some empty-cell spans remain ambiguous. All
PDF results continue to report partial status. Heading inference is heuristic;
font-family sequences, explicit structure-tree roles and richer font-program
metadata remain useful upstream ideas for future work.

Final scores, timing methodology, source identities, per-document changes,
regression checks and reproduction instructions are stored in the latest
`reports/opendataloader-layout-headings-2026-11-10-v2` report.


## Reading order and heading follow-up — 11 November 2026

The next update detects repeated vertical gutters from a bounded glyph-occupancy
projection, including gutters covered by native space glyphs. It requires text
on multiple baselines on both sides and does not apply this projection inside
table cells. Cropped exports sometimes retain entire neighbouring pages in their
content stream: conversion now omits upright glyphs wholly outside the inherited
page box and records the omitted non-whitespace character count per page.
Overlapping glyphs, rotated glyphs and pages with unknown bounds are retained
conservatively. Inspection continues to see the original stream.

Heading changes suppress contents-page entries, overlapping indented caption
continuations, isolated body-size running heads and certain multi-line prose.
Wrapped headings retain their title font after a differently sized section
marker. Emphasized question headings and isolated bold sentence-case display
headings can retain terminal punctuation. No document identifiers or exact
benchmark wording are matched by the parser.

Five new authored tests cover off-page text, unknown/overlapping bounds, native
spaces crossing column gutters, contents and caption exclusions, a differently
sized Roman section marker, and emphasized question headings. Three older
fixtures used invalid off-page coordinates; their placements/page dimensions
were corrected while retaining their original assertions. Preservation checks
now account separately for diagnosed off-page omissions; the former exact-text
checker is retained and its expected failures are documented.

The final report is `reports/opendataloader-reading-headings-2026-11-11`.
The corpus remains a development set. Raster text, ambiguous chart labels,
subtitle distinctions and some reading-order cases remain unresolved.

## Precision and regression fixes — 11 November 2026

The follow-up joins a separated bullet to its adjacent same-baseline item before
column cuts. Short uppercase running titles paired with brief numeric identifiers
are kept in one plain row only with body-size, separation and page-edge evidence.
Both text runs remain in the output. This repairs the two regressions reported
in the previous reading-order run without checking document IDs or content words.

Heading precision now follows same-style caption and marked-list/footnote
continuations across interleaved columns, excludes long body-size bold prose,
and recognizes parallel labels only with nearby numeric-chart or table evidence.
Repeated table labels remain plain even when a continued table paints them in
one run. Ordinary parallel section headings without data evidence are retained.
Table/figure captions introduced with a colon are recognized too. A small numeric
reference between wrapped caption lines is kept after the intact caption;
ordinary captions retain their existing line geometry. These are independent
extensions of the previously attributed pdf-inspector-inspired layout approach.

Five further authored tests cover separated markers, wrapped list emphasis,
renamed/rescaled split headers, multiline captions and bold prose with positive
heading controls, caption superscripts, and parallel headings with/without data.
No table extraction or off-page clipping rules changed. Native per-page
character multisets and inspection are compared strictly to the previous parser
on all 200 benchmark PDFs plus the 104 existing regression inputs; the nine
exact authored table fixtures are also checked. This corpus was used for tuning,
so neither it nor those previously seen regressions constitute a new holdout.

Frozen source, full predictions, evaluations, timing passes and checks are in
`reports/opendataloader-precision-2026-11-11`. Remaining headings without reference
labels include maps, charts, apparent titles and continued contents pages;
absence from the reference alone does not independently prove a false positive.

## Remaining unmatched headings — 11 November 2026

A visual review of the five documents with heading markup but no reference
headings distinguished four extraction mistakes from one defensible title.
The parser now uses four additional general signals:

- A body-size top-margin line on an image-dominated page, separated from
  bottom caption text by a large image-sized gap, stays plain. This uses the
  existing page-box and conservative image-coverage evidence, without OCR.
- A short textual numeric category aligned with three numeric intervals beneath
  a figure caption is a vertical chart label rather than a section heading.
- A lettered bold list introduction followed immediately by a line that begins
  bold and continues in ordinary type is a paragraph lead. A fully normal
  following line does not supply this mixed-style evidence.
- A dominant hierarchical outline with at least six aligned, nondecreasing page
  references is a contents page even without a marker title. It retains the
  page numbers beside their entries and suppresses body-size outline headings.
  Sparse references, decreasing values, ordinary prose and extracted tables do
  not meet the detection requirements. Large contents titles remain eligible.

Four authored tests include positive and negative controls, varied text and
numeric values. Table extraction, clipping, Unicode normalization and inspection
classification are unchanged. Existing image coverage is only exposed to the
layout stage. No document identifier, reference text or saved answer is used
by the parser. The numerical thresholds were developed on the same benchmark,
so the corpus remains a development set rather than an independent holdout.

The prominent title above the table in the fifth case is deliberately retained:
the supplied reference omits heading markup, but the visible typography supports
a title. Ground truth is never edited. The report records this annotation
ambiguity, the tiny NID decrease caused solely by removing markup from one
paragraph lead, and the unchanged native-text/inspection checks.

Final artifacts: `reports/opendataloader-remaining-headings-2026-11-11`.
