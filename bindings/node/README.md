# BulkParse for Node.js

Requires Node 20+ and Rust. This package is not published yet. From this folder:

```sh
npm run build
npm test
```

The build script compiles the native addon for the current host and copies it to `bulkparse.node`. Prebuilt cross-platform distribution is planned; the current package is local-source-only. Do not set a cross-compilation target when using this host build script.

Save the example as an `.mjs` file in `bindings/node`, or use a `.js` file in a package with `"type": "module"`:

```js
import {parse, parseFile} from './index.cjs';
const result = await parse(Buffer.from('# Hello'), {filename: 'hello.md'});
console.log(result.markdown);
const document = await parseFile('../../fixtures/hello.docx', {strict: true});
console.log(document.document.blocks);
```

`parse(bytes, options)` and `parseFile(path, options)` return promises. Conversion runs on the shared Node worker pool; callers must bound queued work and input retention. Inputs are copied into owned Rust memory. Errors expose `.code` (for example `resource_limit`, `unsupported`, `malformed`, `incomplete`).

Options mirror Rust: `filename`, `format`, `strict`, `csv_has_headers`, `limits`. See the root README for limits and fidelity. TypeScript declarations describe the common structured output. `capabilities()` returns the build's format support. No networking or external parsing service is used.


CSV supports automatic comma/semicolon/tab/pipe detection and BOM-aware UTF-8/UTF-16 decoding. Without a BOM or explicit encoding, valid UTF-8 is preserved; otherwise legacy Windows-1252 decoding is attempted and reported with a `csv_encoding_inferred` diagnostic. Encoding inference is ambiguous, particularly for mixed-encoding files. Set `csv_encoding` to `utf8` for strict UTF-8, `windows1252` for a known legacy export, or `utf16_le` / `utf16_be` for BOM-less UTF-16. Explicit encodings must agree with any BOM. Undefined Windows-1252 bytes and binary controls are rejected. For delimiter ambiguity, pass `csv_delimiter` (one ASCII character); headers remain explicit through `csv_has_headers`.

PDFs support conservative ruled-table reconstruction. Arabic presentation forms and alphabetic ligatures expand by default. Pass `pdf_normalize_unicode: false` (Python: `pdf_normalize_unicode=False`) to retain mapped forms. `ActualText` is always preserved. See the root `docs/pdf-tables.md` for limits and validation.

PDF inspection is available without an OCR runtime: `inspectPdf(bytes)`. Conversion results also include `pdf_inspection`. The shared output schema is now 0.8. See [inspection and routing](../../docs/pdf-inspection.md).

The standard build enables BulkParse-owned Office/EPUB readers. See [format support and limits](../../docs/office-formats.md).

For resource-limited spreadsheets and CSV files, opt in to `partial_on_limit` (Python: `partial_on_limit=True`). Successful normal conversions are unchanged; supported limit failures can return an explicitly partial prefix within the original per-attempt budgets. This is not complete extraction and strict mode never accepts the recovery result. Inspect `status` and `resource_limit_partial` diagnostics. See [bounded large-document handling](../../docs/large-documents.md). Rebuild the binding to use this option.

For Markdown-only output, use `parseFileToMarkdown(inputPath, outputPath, options)`.
It reads and writes in the native worker and returns a small metadata summary,
without a JavaScript document model or Markdown buffer. Existing destinations are
never overwritten. CSV records and completed spreadsheet worksheets are released
as output is staged. Large XLSX/XLSM and ODS worksheet XML is processed by row,
and large XLSX shared-string tables use indexed temporary storage. Input bytes
and individual worksheet cell models are still buffered.
See [streaming Markdown](../../docs/streaming-markdown.md) for examples, temporary
storage requirements and failure semantics.

Embedded image bytes are available in `document.assets` (base64 JSON). See [image retention, export and limitations](../../docs/images.md).
