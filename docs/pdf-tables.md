# Ruled PDF tables and Unicode options — 2026-11-10

BulkParse now reconstructs conservative, axis-aligned ruled tables into the shared JSON cell model. Markdown is rendered from those cells. The original holdout table changes from column-first paragraphs to the exact expected rows and cells. Selective Unicode normalization expands glyph presentation forms by default; set `pdf_normalize_unicode: false` to retain the original mapped characters. This default changed during the subsequent [PDF quality fixes](pdf-quality-fixes.md).

## Table behavior

Detection requires at least three painted horizontal rules with consistent extents, repeated column evidence and whitespace gutters that survive all rows. Stroked paths and thin filled rectangles are supported, including graphics/Form transforms. Empty cells retain their positions, wrapped text stays in its cell, and surrounding prose remains outside the table. Cells carry one-based page, row and column provenance.

The detector rejects candidates whose endpoints cut through glyphs or words. Corpus review found short rule segments producing fragmented cells in a scientific PDF; this general boundary check removed those false positives. It does not inspect filenames, hashes, expected phrases or document-specific markers.

Headers are not guessed: `header_rows` is zero, so GFM rendering adds an empty header row and retains every source row as data. Table-cell and text budgets apply. Detection remains heuristic and PDFs remain partial, with `pdf_table_inferred` diagnostics for reconstructed tables.

Borderless tables, merged/spanning cells, rotated table geometry, very tall rows and complex clipping/transparency are not reliably supported. Some ambiguous tables deliberately remain prose. This is not a general table-understanding or visual-fidelity guarantee.

## Unicode policy

The new boolean option is available through Rust, Node.js, Python and WASM:

```rust
let options = bulkparse::Options {
    pdf_normalize_unicode: true,
    ..Default::default()
};
```

```sh
cargo run --release -p bulkparse-cli -- parse input.pdf --pdf-normalize-unicode
```

With the option enabled, Arabic presentation forms and alphabetic ligatures expand to their logical character sequences. Other compatibility characters, including circled digits and fullwidth Latin letters, remain unchanged. Explicit PDF `ActualText` is authoritative and is never normalized by this option. This is selective expansion, not blanket NFKC of the document. The expansion is bounded by text limits and reported through `pdf_unicode_normalized`; disable the option to recover original mapped forms. No OCR, model or language-specific character guessing is involved.

On the authored Arabic/Latin fixture, literal token recall rises from 25.0% to 68.8% with the option enabled. This does not fix ambiguous Persian/Arabic yeh mappings in that PDF. Chinese extraction issues identified in the holdout are also unresolved. These are token-overlap diagnostics, not overall conversion accuracy.

## Original table milestone validation

- 48 Rust integration tests pass, including seven added table/Unicode regressions. The minimal feature build passes 16 tests; workspace Clippy and formatting pass.
- Nine authored PDFs passed exact JSON table checks: six positive cases (including two tables on one page) and three ambiguous/prose cases that must not become tables. Their expected structures were written before rendering with CoreText. They passed the initial implementation; after the boundary correction they were rerun as regression cases, not presented as unseen again. The generated pages were visually reviewed.
- The previous 24-case holdout was rerun with the saved old binary, current default and normalization-enabled profiles, three repetitions each. Only the original ruled-table case changes the default profile's recorded quality metrics. Its exact Markdown row recall and authored block order both rise from 0% to 100%. The former holdout is now development/regression evidence.
- The identical 173-input development corpus still produces 47 complete, 116 partial and 10 failed results. All primary quality inputs parse; targeted PDF and DOCX/CSV checks pass. Additional tables are found in the ODF export and IBM sample PDF, but their full fidelity has not been human-audited.
- Rebuilt Node, Python, WASM and CLI outputs match on 13 cases, including table JSON and the normalization option. WASM offline package installation passes. Twelve Python harness tests pass. The earlier browser UI check was not repeated.

The initial table implementation and false-positive finding are recorded separately from the final boundary-checked implementation (`freeze-v2.json`). Historical parser/competitor results are retained unchanged.

## Resource cost

Seven recorded full-process CLI repetitions after one warmup, rotating the saved pre-table and current profiles on the same machine. These are small local observations, not production-load results or a new live competitor comparison.

| PDF | Wall time before → current default | Peak RSS before → current default |
|---|---:|---:|
| Newspaper | 10.09 → 10.49 ms | 14,656 → 14,704 KiB |
| Elsevier | 61.28 → 63.18 ms | 62,240 → 62,912 KiB |
| Arabic | 4.17 → 4.24 ms | 5,440 → 5,472 KiB |
| Four-page prose | 9.05 → 9.13 ms | 9,216 → 9,312 KiB |

The CLI is 3,503,968 bytes (previously 3,436,704). WASM is 2,540,071 bytes, or 923,087 bytes with gzip level 9. `unicode-normalization` is now a direct optional PDF dependency; it was already present in the lockfile. No heavy engine or model was added.

## Reproduce

```sh
cargo test --offline --locked -p bulkparse
python3 scripts/validate_pdf_tables.py --freeze benchmarks/pdf-tables/freeze-v2.json
node scripts/check_binding_parity.mjs --pdf-corpus
python3 scripts/evaluate_holdout.py --freeze benchmarks/pdf-tables/freeze-v2.json \
  --engines benchmarks/pdf-tables/engines.json --repeats 3 --out reports/table-regression-new
```

The authored fixtures are bundled under `fixtures/pdf-tables`. Corpus checks require the local external fixtures; before/after runs require the saved baseline executable under `target/`. A new machine/build must record its own binary freeze rather than bypass fingerprint checks.

Evidence: [authored cases and results](../benchmarks/pdf-tables/), 173-input corpus (historical artifact; not distributed), 24-case regression (historical artifact; not distributed), and resource measurements (historical artifact; not distributed).

Subsequent work added [OCR routing](pdf-inspection.md), [persistent-worker comparisons](pdf-inspector-comparison.md), and [PDF quality fixes](pdf-quality-fixes.md). Broader human-reviewed layout cases and Chinese/font mapping remain priorities.
