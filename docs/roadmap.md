# Roadmap

## First validation gate

- Completed locally on Apple Silicon macOS: dependency lockfile, workspace compilation, default/minimal core tests, Node/Python native smoke tests, fixture parity with the CLI, WASM release build/smoke tests, native/WASM parity, browser DOCX conversion and offline npm-package validation. See [validation](validation.md).
- Build and execute Node, Python and WASM smoke tests in CI; local checks pass; remote CI and broader platform coverage remain.
- Add cross-language golden output comparison, CLI integration tests and a real-browser test.
- Measure release binary, WASM and wheel sizes; establish a small baseline corpus.
- Fuzz malformed ZIP/XML/PDF/Markdown/CSV inputs; audit dependencies and document process isolation.

## Fidelity before breadth

The [first upstream evaluation](fixture-evaluation.md) is complete: 100 primary quality fixtures plus 73 separate robustness/supplemental cases. Prioritize unreported content-control omissions and inherited numbering in DOCX, then CSV dialect/encoding handling. Preserve the baseline manifest when evaluating fixes.

- DOCX: finish custom numbering, image/equation references and visual placement. Explicit merged tables, text boxes, referenced auxiliary stories and current-revision selection now have general support; page-specific auxiliary placement remains approximate.
- PDF: [built-in inspection](pdf-inspection.md) now classifies every page and routes scans, sparse text over large images and corrupt text layers to opt-in OCR. Region-level OCR fusion and independent classifier evaluation remain. Native positioned text, conservative ruled tables, font-size headings and repeated numeric-row ordering are implemented. Selective Unicode presentation-form expansion is now the default; see [quality fixes](pdf-quality-fixes.md). Optional page-selective OCR now retains geometry/confidence and handles simple horizontal columns; explicit recognition profiles now have a controlled multilingual/resource comparison. Expand independent real-scan, complex-layout, paragraph and scanned-table evaluation. See [OCR profiles](ocr-profiles.md).
- Markdown: preserve task markers, table alignment, link titles and unsupported metadata; round-trip semantic tests.
- Renderer: table cell block conversion, complex inline boundaries, safe link policy, renderer-specific loss diagnostics.

## Further formats and fidelity

| Group | Candidates | Work required |
| --- | --- | --- |
| Web | HTML, XHTML | DOM parsing, boilerplate policy, tables, sanitized output |
| Spreadsheets | XLSX/XLSM, XLS, XLSB, ODS now experimental | Improve rich styles, charts, conditional formatting and visibility; see [contracts](office-formats.md) |
| Slides | PPTX/PPTM, PPT, ODP now experimental | Improve spatial ordering, nested lists, charts, SmartArt and image extraction |
| Documents | ODT, RTF and DOC now experimental | Full styles, numbering, auxiliary stories, embedded objects and legacy tables |
| Publishing | EPUB now experimental; FB2 not implemented | EPUB CSS/visibility, rich numbering, assets and encryption policy |
| Mail | EML, MBOX; later MSG | MIME decoding, attachment policy, recursive size budgets |
| Data | JSON, JSONL, XML | Explicit document mapping; avoid treating arbitrary trees as prose |
| Images | PNG, JPEG, TIFF, WebP | Optional OCR; frame/page and pixel limits |
| Legacy | BulkParse-owned DOC, PPT, XLS readers now experimental | Broaden independent corpus coverage and formatting fidelity |

Each format needs a capability contract, resource budget, real fixtures, fidelity evaluation and binding parity. Distinguish container variants and aliases from genuinely new extraction capabilities. Archives alone are containers, not a document format count.

## Public release gate

The optional [native process pool](worker-pool.md) now implements bounded admission, deadlines, cancellation, crash replacement and metrics. Before Bulkgrid deployment, validate it on the target server with OS-enforced limits, sustained representative traffic and service-level overload handling. Durable job storage, tenant authorization and retries remain host responsibilities.

Publish packages only after CI passes on the chosen OS/architecture matrix. Automate N-API artifacts and Python wheels, add versioned schema compatibility tests, check package names, attach licenses/SBOMs, and document supported runtime versions. Benchmark evidence must precede “fastest”, “smallest” or “10x” marketing.
