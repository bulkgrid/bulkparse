# Fixture fixes — 2026-11-10

The silent DOCX content-control loss, inherited list omission, CSV dialect/encoding failures, and recognized code/quote style omissions identified in the [baseline evaluation](fixture-evaluation.md) are fixed. A narrower PDF spacing correction and recovery from malformed optional DOCX styles are also implemented. Complex PDF layout, bidirectional ordering, OCR and advanced Office content remain unfinished.

## Changes

- DOCX traversal now unwraps content controls at body, row and cell levels, including nested tables. Unknown table/row elements produce content-loss diagnostics rather than disappearing under a complete status.
- Paragraph style inheritance resolves simple decimal/bullet list definitions, levels, start overrides and continuation across tables/prose. Numbering instance zero suppresses inherited numbering. Missing definitions, unsupported custom markers, numbered headings and missing ancestor levels produce partial results. Linked numbering styles and arbitrary marker formats are not fully supported.
- Inherited paragraph/character bold, italic and strike formatting is applied, with direct run overrides. Recognized Source Code/Code/HTML Preformatted and Quote/Quotation/Intense Quote styles produce code and quote blocks. These are explicit style-name conventions, not general code detection from fonts or visual appearance.
- Malformed optional styles/numbering XML can fall back to body extraction with a partial diagnostic. Main-document XML remains strict. Depth, node and archive expansion limits still fail; a limit error is not treated as recoverable style damage. Missing referenced styles now warn about potentially lost inherited formatting/numbering.
- CSV inspects at most 64 KiB and 32 complete records per candidate delimiter (comma, semicolon, tab, pipe). Quoted newlines are supported. Ambiguous detection fails and asks for an explicit delimiter. Row widths remain strict; TSV always uses tabs. UTF-8 and BOM-aware UTF-16LE/BE are supported, with an override for BOM-less UTF-16. Invalid encoding sequences and conflicting BOM overrides fail.
- PDF font/CMap decoding still uses lopdf. BulkParse now assembles text operations, handles fractional as well as integer TJ spacing, and separates repositioned array fragments without inserting spaces between adjacent show operations. This is a heuristic text extractor: it does not reconstruct geometry, columns, tables or logical bidirectional order. All PDF results remain partial.
- No new Cargo dependencies or model downloads. DOCX numbering shares immutable level definitions and bounds levels to the OOXML range. Node/Python/WASM all use the same core options; CLI conversion exposes `--csv-delimiter` and `--csv-encoding`.

## Same-corpus results

The final run uses the exact same manifest bytes and source/reference SHA-256 values as the baseline: 173 files, including 100 primary quality fixtures. Binary and Rust-source hashes were also rechecked against the final build. The local competitor checkouts were not modified; no competitor engines were executed. Saved outputs are comparison references, not independently annotated ground truth.

| Primary format | Files | Baseline complete / partial / failed | After fixes complete / partial / failed |
| --- | ---: | ---: | ---: |
| DOCX | 47 | 10 / 37 / 0 | 13 / 34 / 0 |
| PDF, including OCR | 29 | 0 / 29 / 0 | 0 / 29 / 0 |
| CSV | 10 | 8 / 0 / 2 | 10 / 0 / 0 |
| Markdown | 14 | 14 / 0 / 0 | 14 / 0 / 0 |
| Total | 100 | 32 / 66 / 2 | 37 / 63 / 0 |

Across all 173 files: 47 complete, 116 partial and 10 failed, compared with 42 / 118 / 13 before. There were no crashes or harness stops. The ten remaining failures are malformed/abusive inputs or the encrypted PDF; two recoverable malformed-main-XML fixtures still fail deliberately. Nine primary outputs remain empty and partial, principally scans/OCR inputs. “Complete” is the parser's coverage status, not a general fidelity score.

Structural assertions verify the actual regressions, independently of token overlap:

| Fixture | Before | After |
| --- | --- | --- |
| Docling `docx_sdt_table_cells.docx` | Lost cover/reviewer/status values and one table column | All three values retained; both affected rows have three cells |
| Docling `docx_list_resumed_after_table.docx` | Plain paragraphs without numbering | Two items starting at 1, then two starting at 3 after the table |
| Docling tab/pipe/semicolon CSVs | One column | Twelve columns in every row |
| AnyDoc semicolon and UTF-16 CSVs | Conversion failed | Complete tables with all reference tokens retained |
| Docling `docx_code_blocks.docx` | Code styles rendered as prose | Two recognized code blocks |
| AnyDoc `handmade-blockstyle.docx` | Lost code/quote semantics | Five code blocks and two quote blocks |
| AnyDoc corrupt optional styles | Conversion failed | Body retained, partial warning; strict mode rejects |

For PDF `2305.03393v1-pg9.pdf`, the lexical token-recall proxy improves from 51.2% to 98.1%; `redp5110_sampled.pdf` improves from 93.3% to 94.2%. The newspaper fixture remains at 2.7%, the Elsevier fixture around 2.6%, and the Arabic fixture at 11.4%. These are word-overlap diagnostics, not layout/reading-order accuracy. A preliminary PDF change caused regressions and was corrected before the final run; the final comparison has no material token-recall regression. OCR and positioned/bidirectional extraction still need their own implementation and evaluation.

## Validation and reproduction

- 28 default-feature Rust integration tests; 16 with default features disabled.
- Workspace Clippy with warnings denied and formatting checks pass.
- Node and Python: three tests each, including UTF-16/delimiter overrides and ambiguity errors.
- WASM release build, conversion smoke checks and offline packed-package installation pass. Artifact: 2,328,299 bytes, or 855,256 bytes with gzip level 9 (about 66 KB / 22 KB larger than the previous build).
- Eight cases match exactly across Node, Python, WASM and CLI, covering all seven formats plus explicit UTF-16/delimiter options.
- Strict-mode checks pass for restored content controls and inherited lists, and reject unsupported numbering and corrupt optional styles without output.
- The original generated `hello.docx` fixture now includes its referenced heading style and relationship; it previously relied on a missing style being silently accepted. Third-party fixtures are unchanged.

The observed maximum per-child peak RSS across the corpus is 56,976 KiB, versus 57,616 KiB in the baseline. These single local runs do not establish a memory or speed improvement; process startup, polling, caches and concurrent builds affect measurements. macOS has no hard memory sandbox in this harness. There is no “10x” or competitor-superiority claim.

```sh
cargo test --offline --locked -p bulkparse
cargo test --offline --locked -p bulkparse --no-default-features
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
cargo build --offline --release -p bulkparse-cli
# Choose a new output directory for each evaluation:
python3 scripts/evaluate_fixtures.py --out reports/fixture-fixes-next
python3 scripts/check_fixture_regressions.py reports/fixture-fixes-next
# After rebuilding the three bindings and making the Python extension importable:
node scripts/check_binding_parity.mjs
```

Final machine-readable results: `reports/fixture-fixes-2026-11-10-run2` (historical artifact; not distributed). The unchanged baseline remains in `reports/fixture-evaluation-2026-11-10-run2`. Raw converted text remains under ignored `target/fixture-evaluations/`; source fixtures and full reference/converted outputs are excluded from the source archive. The regression tests generate original synthetic documents and do not depend on downloading third-party fixtures.
