# BulkParse-owned Office and EPUB readers

The `office` Cargo feature adds eleven canonical formats: `xlsx`, `xls`, `xlsb`,
`pptx`, `ppt`, `odt`, `ods`, `odp`, `epub`, `rtf`, and `doc`. It is enabled by
default and in the CLI, process pool, Node, Python and WASM bindings. The existing
DOCX reader already accepts DOCM. The JSON schema is **0.7**: `Inline::Math { tex, display }` and `Inline::Anchor { id }` preserve structured equations and internal link targets. Consumers matching inline variants exhaustively must update. PDF inspection also reports the shared version; its fields are unchanged.

These are BulkParse implementations, not wrappers around AnyDoc, Docling,
LibreOffice, Calamine or a cloud service. ZIP, XML, CFB and encoding libraries
provide containers and decoding. They do not parse documents. No macros,
formulas, external links, OCR or language models execute in these readers.

| Format / aliases | Implemented extraction | Remaining limitations |
| --- | --- | --- |
| XLSX / XLSM | Workbook relationship order; shared/inline strings; typed values; cached formulas; common numeric and ISO-normalized date/time values, literal/negative/zero format sections and elapsed hours; 1900/1904 epochs; merged cells; rich text, cell emphasis, hyperlinks, comments and cached chart series; original cell coordinates | Formula evaluation, full number-format language and style inheritance, chart visuals/cache freshness, image-in-cell contents and conditional formatting |
| PPTX / PPTM / PPSX / PPSM | Slide relationship order; grouped text; title placeholders; bold/italic/strike and bullets inherited through layout/master; external links; rich table cells, declared headers and merged cells; conservative position-based shape ordering; notes; nested lists, Roman/alphabetic markers and cached chart series; OMML equations | Ambiguous/rotated/overlapping spatial layouts, full theme/style cascade, exotic numbering systems, chart visuals/cache freshness, SmartArt and image pixels |
| XLS | BIFF5/7 and BIFF8 workbook/sheet records; code-page text in BIFF5/7, shared strings with continuation encoding changes in BIFF8; cached formulas, common numbers, merges; nested chart/object substreams isolated from worksheet coordinates; BIFF8 continued drawing/chart-caption text and cell comments | BIFF versions before 5, unsupported legacy code pages, BIFF5/7 inline comments, rich formatting, print layout, drawings, links; omitted legacy comments and unsupported visual content have diagnostics; hidden row/column values are retained with a diagnostic. Missing legacy code pages use Windows-1252 with a notice; unsupported declared code pages and invalid encoded text are rejected |
| XLSB | Binary workbook relationships, rows/cells, shared strings, cached formulas, numeric formats and merges | Full style/visibility metadata, drawings, links and advanced records |
| ODT | Paragraphs, explicit/style headings, inherited/default emphasis, code/quote styles, list numbering/restarts/continuations, MathML and package-local equation objects, links, whitespace, rich cell lists and nested tables, declared table headers, inline notes | Full visual styles and cell-style cascade, auxiliary stories and non-equation embedded objects |
| ODS | Sheets, typed/cached cell values, bounded repeated rows/cells and merges | Styles, automatic recalculation, charts, images and exact display/visibility rules |
| ODP | Page order, grouped text, lists, rich tables and page provenance; conservative ordering from explicit shape bounds | Ambiguous/rotated spatial layouts, theme styles, images and embedded objects |
| EPUB | Container/OPF relationships, spine order, semantic XHTML headings/lists/tables/links, signed/reversed/alphabetic numbering, scoped CSS emphasis/visibility/cascade, MathML equations, metadata title, contents documents and collision-free internal anchors, alternative text | Full CSS layout/selectors/conditional rules, EPUB 2 NCX-only contents, HTML repair, encrypted/obfuscated resources and image pixels; internal DTD subsets are rejected |
| RTF | Scoped groups, code pages, Unicode/surrogates, text styling, inherited paragraph roles isolated from list-label resets, escaped physical line endings, structured equations, paragraphs and tables assembled from physical cell boundaries; horizontal/vertical merge chains and explicit headers; separately retained header/footer definitions | Full list numbering, nested tables and malformed/ambiguous merge geometry, images/objects, page-specific auxiliary placement and repeated section variants |
| DOC | Word 97+ simple and piece-table main stories, code pages, field results, STSH/FKP paragraph and character-style inheritance, emphasis, headings/code/quotes; physical table boundaries, row headers and horizontal/vertical merges, retaining rich cell paragraphs; piece-table footnote/endnote, header/footer, comment and text-box text appended by story with provenance | Full character-property language, automatic numbering, floating objects, note links and auxiliary layout, nested tables and ambiguous/modified table geometry |
| PPT / PPS / POT | Active persistence directory, active slide order, UTF-16 style runs, per-slide master emphasis, nested bullets and notes matched by slide ID | Full rich formatting, automatic numbering, table geometry, spatial layout and images; damaged persistence is rejected rather than scanning obsolete slide records |

All capabilities remain experimental. Returning output is not a claim that a
format's full semantics are implemented. Known content gaps produce diagnostics
with `content_loss: true` and a `partial` status. `strict: true` rejects partial
results. Cached formulas can be stale; the readers preserve cached values and
report that they have not calculated them. Hidden XLSX sheets/slides and cell
values follow a visible-content policy; hidden rows/columns leave blank positions
so provenance continues to identify original cells.

All tables render as GFM pipe tables. Rich inline emphasis and safe links are retained; lists, multiple paragraphs and nested tables become inline cell text separated with `<br>`. Nested rows retain column order with ` / ` separators. The first explicit header row becomes the GFM header; additional header rows remain visible data with a rendering diagnostic. Headerless tables get an empty header. Merged cells keep their anchor text with blank covered positions, and JSON retains row/column spans and full block structure. Covered-cell content remains inside its merged anchor with its original provenance. Whitespace sits outside Markdown emphasis delimiters. Rendering remains incremental and only active row-span positions require layout storage.

Equations use a bounded MathML/OMML subset translated into generated TeX; annotations and macros are not executed. Unsupported structures retain available expression content with a partial diagnostic. Legacy DOC formatting metadata failures retain main-story text with a warning; malformed required text/persistence still fails. EPUB targets are validated against emitted anchors, preserve case, and report missing/hidden or duplicate targets.

Position-based ordering only sorts contiguous groups with usable geometry; unknown positions retain source-order barriers. PPTX uses available own or inherited placeholder bounds; rotated/flipped groups keep source order. ODP recognizes explicit length units. These are reading-order heuristics, not a visual layout engine.

## Bounds and failure behavior

The readers use shared input, archive-entry, expanded-byte, XML-node/depth,
output-text, table-cell and page budgets. Shared-string and ODF repeat expansion
is bounded before materialization. Sparse dimensions are not trusted as evidence
of used cells; a genuinely enormous populated bounding grid fails at the cell
limit. Internal package paths cannot escape their archive, external content
relationships are not loaded, XML entities cannot fetch remote data, and CFB
streams are read with expanded-byte limits. Cancellation is cooperative. Use the
existing process pool and host OS limits for untrusted production uploads.

EPUB visibility is conservative when unsupported CSS selectors or conditional rules can change `display`: text is retained with a partial diagnostic. No stylesheet or link fetches external content. Numeric and chart caches are not recalculated. Nondecimal list labels are preserved as text where the shared document model cannot represent them. Chart series remain separate tables; comments can include compatibility text stored by Excel.

## Validation and comparison

See the legacy layout/formatting follow-up (historical artifact; not distributed), the table/layout follow-up (historical artifact; not distributed), the legacy/equation/navigation follow-up (historical artifact; not distributed), the extraction quality follow-up (historical artifact; not distributed) and the original multiformat report (historical artifact; not distributed) for the
frozen corpus, authored checks, same-machine timing runs, failures and memory
measurement limits. These fixtures informed development; they are not an unseen
corpus or an LLM-judged quality score. The report does not establish parity with
AnyDoc's published 100-document, fourteen-format benchmark.

Design references were AnyDoc's package/format readers and Docling's Excel and
PowerPoint backends. BulkParse owns the mapping into its model and its limits,
diagnostics and renderer. See [third-party notices](../THIRD_PARTY_NOTICES.md).

## Bounded large-document recovery

`partial_on_limit: true` enables a second, bounded recovery attempt for CSV/TSV, XLS, XLSX/XLSM and ODS resource-limit failures. CSV retains complete records, XLS retains completed worksheets, and XML spreadsheets return coordinate/value previews. Recovered output is always partial, with explicit omissions. Default and strict behavior remain unchanged. See [the API contract and limitations](large-documents.md) and measured regression results (historical artifact; not distributed).

Auxiliary Word recovery reads existing binary story ranges without loading image assets; text is appended by story, not reconstructed at its original page position. DOCX drawing titles/descriptions are retained when distinct from extracted text-box content. Legacy XLS comment text retains its cell location when the object/NOTE mapping is available. Source-backed recovery checks and performance limits (historical artifact; not distributed).
