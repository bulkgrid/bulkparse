# Changelog

## 0.1.4 — 2026-11-22

- Always render tables as GFM pipe tables across document formats and streaming output. Add empty headers where needed, retain merged-cell positions as blank cells, and flatten nested/block content into readable inline cell text. Structured JSON preserves spans, headers, and original blocks.
- Preserve literal pipes inside code spans and keep single quantity tildes readable without unnecessary backslashes. Keep authored line breaks and positioned PDF fragment spacing consistent.
- Share table placement between structured and streaming rendering, retain the allocation-free plain-cell path, and bound merged-table layout storage by column count.

## 0.1.3 — 2026-11-21

- Reconstruct borderless PDF label/value tables from repeated geometric alignment, explicit units or currencies, and shared column gaps. Preserve aligned headings, empty heading cells, annotations, cell positions, and surrounding reading order.
- Normalize zero-width word breaks without removing joiners or changing visible PDF ActualText.
- Remove redundant Markdown escapes from ordinary punctuation, decimals, dates, percentages, and paths across document formats. Preserve the escapes needed for literal Markdown syntax, links, HTML-like text, entities, and table separators.
- Recover OCR setup when the pinned GitHub runtime archive is unavailable, using a checksum-verified Microsoft npm archive for the same runtime version.
- Keep layout detection on borrowed text ranges and optimize unchanged-text scanning. Add generated layout regressions, Markdown round-trip checks, and a rendering benchmark.

The borderless detector remains conservative: ambiguous prose tables and wrapped or merged borderless cells retain the existing fallback.
