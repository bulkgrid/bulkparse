# Implementation notes

Detailed scope, operational notes and historical validation context. For the product introduction and latest benchmark overview, see the [BulkParse README](../README.md).

An open-source Rust engine for converting documents into structured JSON and Markdown, with Node.js, Python, and browser WebAssembly bindings. Designed for Bulkgrid and independent use: no service account, network requests, model downloads, or Bulkgrid dependency.

**Status: experimental first implementation, not a production release.** The Rust workspace compiles, core tests pass, and native Node/Python conversions have been verified on Apple Silicon macOS. WASM smoke tests, native/WASM fixture parity, browser DOCX conversion and offline npm-package installation also pass. Broader cross-platform validation remains. See the [validation record](../docs/validation.md) for exact checks and the [frozen holdout comparison](../docs/holdout-evaluation.md) for workload-specific measurements. No fastest or 10x claim is established.

## Quick start

Install Rust, then run from the repository root:

```sh
cargo fetch
cargo test -p bulkparse
cargo test -p bulkparse --no-default-features
cargo run -p bulkparse-cli -- parse fixtures/hello.docx
cargo run -p bulkparse-cli -- parse fixtures/table.csv --to json
cargo run -p bulkparse-cli -- capabilities
cargo run -p bulkparse-cli -- schema
cargo run --release -p bulkparse-cli -- batch fixtures --output /tmp/bulkparse-output
```

Output files are created without overwriting existing files. Batch conversion is sequential, continues after per-file errors, and prints JSONL results. Exit codes: `0` complete, `1` failure, `2` partial conversion. A directory containing a PDF will currently produce exit code `2`. Place batch output outside the input directory.

## Implemented scope

| Input | Engine | Current extraction | Known limits |
| --- | --- | --- | --- |
| UTF-8 text | BulkParse | Paragraphs | No legacy encoding detection |
| Markdown | pulldown-cmark + BulkParse mapping | Headings, lists, quotes, links, code, GFM tables | Some presentation metadata is not represented; HTML stays inert |
| CSV / TSV | csv + BulkParse mapping | Quoted fields, multiline cells, comma/semicolon/tab/pipe CSV, UTF-8 and UTF-16 | Bounded dialect detection; ambiguity requires an override; fixed row width; explicit headers |
| Jupyter notebooks | serde_json + BulkParse mapping | Markdown/code cells, text outputs, cell provenance | No execution; rich MIME outputs reported as gaps |
| DOCX / DOCM | zip + roxmltree + BulkParse OOXML mapping | Body text, content controls, headings, inherited formatting, code/quote styles, simple lists, links, grid-based merged tables, text boxes, referenced headers/footers and linked notes/comments | Custom numbering, image pixels, equations, floating placement and page-specific auxiliary layout remain incomplete |
| PDF | lopdf + BulkParse mapping | Positioned text, column/directional heuristics, Form XObjects, painted-grid and sparse-rule tables, selected borderless numeric tables | Always partial: ambiguous/open merged cells, text-heavy borderless tables and complex layouts remain unresolved; OCR requires an explicit provider |

The **`office` feature** adds BulkParse-owned readers for XLSX/XLSM, PPTX/PPTM/PPSX/PPSM, XLS, XLSB, ODT, ODS, ODP, EPUB, RTF, DOC and PPT/PPS/POT. They extract structured text and tables with explicit partial-result diagnostics; full visual and formatting fidelity remains experimental. See the [format contracts](../docs/office-formats.md). No external document converter is required at runtime.

`docx`, `pdf` and `office` are optional Cargo features, all enabled by default. `default-features = false` builds the smaller text/Markdown/tabular/notebook engine. The capability API reports enabled formats. The output schema is **0.7**, including structured math and anchor inlines. Options accept canonical names `text`, `markdown`, `csv`, `tsv`, `notebook`, `docx`, `pdf`, `xlsx`, `xls`, `xlsb`, `pptx`, `ppt`, `odt`, `ods`, `odp`, `epub`, `rtf`, `doc`; filename aliases are listed in the format contracts.

CSV defaults to UTF-8 unless a UTF-8/UTF-16 BOM is present. Detection examines at most 64 KiB and 32 complete records for each of comma, semicolon, tab and pipe; tied candidates fail explicitly. TSV always uses tabs. For ambiguous files, use `--csv-delimiter ';'`; for BOM-less UTF-16, use `--csv-encoding utf16-le` or `utf16-be`. Library/binding options are `csv_delimiter` (one ASCII character) and `csv_encoding` (`utf8`, `utf16_le`, `utf16_be`). Headers remain opt-in with `csv_has_headers`. Example:

```sh
cargo run -- parse data.csv --csv-delimiter ';' --csv-encoding utf16-le --csv-has-headers --to json
```

## Benchmarks

See the [current comparisons](../README.md#benchmarks) for mixed-document conversion, PDF extraction and OCR. Corpus manifests, measured settings and limitations are linked there.

## Rust API

```rust
use bulkparse::{parse, Options, Parser};

let result = Parser::default().parse_bytes(b"# Hello", Some("hello.md"))?;
let json = result.to_json()?;
let markdown = result.to_markdown();
let both = parse(b"# Hello", Some("hello.md"), Options::default())?;
// Inside a function returning Result<_, Box<dyn std::error::Error>>.
```

`Parser` also accepts files and capped readers. Rust callers can use `parse_with_cancel` with a shared `CancellationToken`. Cancellation is cooperative between parsing steps; it cannot interrupt an underlying engine call.

PDFs use bounded ToUnicode maps, font-width positioning and heuristic line/column/directional ordering. See the [PDF milestone report](../docs/positioned-pdf.md) for measured improvements and limitations. Rust hosts can opt into a budgeted [OCR provider](../docs/pdf-ocr.md); no provider or model is installed by default.

The optional OCR backend now reuses verified model sessions in supervised workers.
Its default small-model profile uses adaptive detection resolution, with a
fine-print fallback. See the Linux CPU before/after benchmark (historical artifact; not distributed)
for measured speed, memory tradeoffs, quality and reproducible outputs.

PDFs reconstruct painted grids, shaded cells, some sparse-rule tables and repeated numeric tables into JSON cells. Borderless tables require at least three aligned numeric columns and several rows, with stronger evidence when there is no header. Native PDF tables render as escaped HTML within Markdown, preserving actual rows without an invented empty header; consumers need HTML-table support. Rectangular row/column spans are inferred from painted boundaries, and sparse-rule spanning headers and blank worksheet columns are supported. Semantic headers are not inferred. See the merged-cell benchmark and limitations (historical artifact; not distributed). Arabic presentation forms and alphabetic ligatures expand by default; set `pdf_normalize_unicode: false` (CLI `--pdf-normalize-unicode=false`) to retain mapped glyph forms. `ActualText` is always preserved. Larger isolated lines can become headings, and repeated dense numeric rows stay in row order. See [PDF quality improvements](../docs/pdf-quality-fixes.md) and the earlier [table and Unicode validation](../docs/pdf-tables.md).

A real [optional PDF OCR backend](../backends/rapidocr/README.md) is now available through Rust and an OCR-enabled CLI. It renders image-only pages with PDFium and recognizes text with RapidOCR/ONNX Runtime in an explicitly provisioned Python process. Default builds stay independent of these engines and models. [OCR layout validation](../docs/ocr-layout.md) records preserved coordinates/confidence and corrected column order on the authored fixture. The default Swedish-accent error remains. [Explicit recognition profiles](../docs/ocr-profiles.md) now offer a larger model that fixes selected accent cases, with measured runtime/memory tradeoffs. OCR results retain explicit limitations. Node/Python reusable OCR parsers and the WASM evidence-assembly API are now available; the native parsers also accept standalone images. See [binding APIs and limits](ocr-bindings.md).

## Bindings

All bindings invoke the same Rust `parse` path and return the same `document`, `status`, `diagnostics`, `coverage`, `parser_version`, `markdown`, and `render_diagnostics` fields.

- [Node.js](../bindings/node/README.md): asynchronous native conversion on the Node worker pool, TypeScript declarations, capped file reads.
- [Python](../bindings/python/README.md): native extension, releases the GIL while parsing, capped file reads.
- [Browser/WASM](../bindings/wasm/README.md): local bytes only, with a Web Worker example.

Packages have not been published. Build from this source tree for now. Native artifacts have been exercised on Apple Silicon macOS only; Linux, Windows, published wheels and broader browser compatibility still need validation.

## Output and limits

JSON is the canonical representation; Markdown is a projection. IDs are deterministic within one conversion, not persistent document identifiers. Provenance includes page, cell, row, package part, and byte ranges where available. Byte ranges are UTF-8, end-exclusive offsets into the decoded part/cell/text, after removing any BOM; they are not positions in the original ZIP or notebook JSON. Schema version `0.6` is experimental; OCR line blocks may include `source.ocr` with rendered image dimensions, quadrilateral coordinates and an engine confidence score. Semantic OCR blocks may instead include `source.pdf_region` with region bounds, label, reading order and optional confidence. Native sources omit both fields. See the [optional advanced backend](../backends/glmocr/README.md).

`complete` means no known content gap was reported by this implementation; it is not a visual fidelity guarantee. Parsing diagnostics and rendering diagnostics are separate. `strict: true` rejects known content gaps, including Markdown projection loss when using `parse`. Inspect both diagnostics arrays. Unsupported formats fail explicitly. Image-only PDFs now produce page-level `pdf_ocr_needed` diagnostics; blank pages are distinguished when image evidence is available. Hidden OCR-layer quality and mixed-content regions are not fully classified.

Defaults: 32 MiB input, 32 MiB extracted text, 16,777,216 counted nodes, depth 64, 4,194,304 table cells, 100,000 additional repetition-generated cells, 2,000 PDF pages, 10,000 archive entries, 256 MiB read DOCX/PDF expansion, and 128 MiB native-writer Markdown. Large-document handling is automatic; explicit lower limits remain enforced. Limits are configurable. These checks **are not a hard memory or CPU sandbox**: third-party parsers may allocate before checks, PDF expansion is checked after underlying decompression, not before allocation, and JSON parsing materializes the notebook. The engine and JSON bridge hold input and output in memory; bindings introduce copies. Batch file discovery currently materializes the path list.

Use isolated workers with OS memory/CPU limits for untrusted uploads. Host concurrency and queue bounds belong to the caller. The browser example runs conversion in a worker that the host may terminate. Rendered Markdown still requires a safe Markdown renderer; output JSON preserves original links and raw source. BulkParse never fetches those links, runs notebook code, or executes macros.

The optional native [worker-pool crate](../docs/worker-pool.md) now provides bounded admission, persistent child processes, end-to-end deadlines, cancellation, crash recovery and metrics. It uses the same conversion output and keeps complete, partial and failed results distinct. The core and existing bindings remain independent of this host component. OS resource limits and deployment-server validation are still required.

## Development

```sh
cargo fmt --all --check
cargo check --workspace
cargo test -p bulkparse
cargo test -p bulkparse --no-default-features
cargo clippy -p bulkparse -p bulkparse-cli --all-targets -- -D warnings
python3 scripts/generate_fixtures.py
```

Our bundled fixtures are original, reproducible examples under the project license. An [upstream fixture evaluation](../docs/fixture-evaluation.md) now covers 173 inputs from the local AnyDoc/Docling reference checkouts, with source hashes and separate quality/robustness tracks. It identifies silent DOCX omissions; `complete` and strict mode are not yet reliable fidelity guarantees. The reference documents themselves are not bundled.

A separate [holdout protocol](../benchmarks/holdout/README.md) freezes the parser before evaluation on 12 external PDFs and 12 original generated cases, with live AnyDoc and Docling profiles. Original generated PDF bytes are preserved in `fixtures/holdout`; the external documents and their converted text are not redistributed. Gold and differential references are reported separately.

[Persistent-worker measurements](../docs/worker-benchmarks.md) now cover 1, 2, 4 and 8 processes, including throughput, latency, CPU and sampled pool memory. BulkParse scaled from 428 to 3,130 returned results/second on this small PDF workload; partial and empty results count, capability profiles differ, and these are not production capacity guarantees. No OCR ran.

See [architecture](../docs/architecture.md), [roadmap](../docs/roadmap.md), and [contributing](../CONTRIBUTING.md). The checked-in CI workflow is a validation plan; it has not yet run remotely.

## License

[MIT licensed](../LICENSE), with required [third-party notices](../THIRD_PARTY_NOTICES.md).


## PDF inspection

Inspection is built into PDF conversion and is also available without OCR:

```sh
cargo run -p bulkparse-cli -- inspect document.pdf
```

Rust: `Parser::inspect_pdf`; Node/WASM: `inspectPdf(bytes)`; Python:
`inspect_pdf(bytes)`. Conversion JSON includes `pdf_inspection` with page types,
routing reasons and OCR outcomes. Output schema is **0.7**. Inspection adds
no OCR runtime or model dependency. See [design, API and limitations](../docs/pdf-inspection.md).

Rust `Source.part` now uses `Option<Arc<str>>` so worksheet identifiers are shared across cells. JSON remains a string with the same schema; Rust callers constructing source values can use `Some(name.into())`.

Rust `Source.byte_range` uses `Option<Arc<[usize; 2]>>` to keep absent ranges small and share present ranges when provenance is cloned. Construct it with `Some([start, end].into())` and read it with `.as_deref()`. JSON still uses the same optional two-integer array; the JSON Schema and Node/Python/WASM contracts are unchanged.
