# Streaming Markdown output

Use the Markdown-only writer when you need a file or byte output rather than the
complete structured document model. CSV/TSV records are rendered and released
individually. XLSX/XLSM, XLS, XLSB and native ODS readers render and release final
table rows as they write them, then release the worksheet before moving to the
next sheet. Tables render incrementally instead of building an additional
whole-table Markdown string.

## Node.js

```js
import {parseFileToMarkdown} from '@bulkgrid/bulkparse';

const summary = await parseFileToMarkdown('workbook.xlsx', 'workbook.md', {
  partial_on_limit: false,
});
console.log(summary.status, summary.diagnostics);
```

The promise resolves with format, status, diagnostics, render diagnostics,
coverage, parser version and `bytes_written`. It does not return `document` or
`markdown`. Reading and writing run in the native worker without copying the input
or Markdown through JavaScript. The input filename comes from its path; `format`
is available for explicit format selection. Rebuild the native addon to use this API.

## Rust

```rust,no_run
use bulkparse::Parser;

let summary = Parser::default()
    .parse_file_to_markdown("workbook.xlsx", "workbook.md")?;
# Ok::<(), bulkparse::Error>(())
```

`Parser::parse_to_markdown(bytes, filename, &mut writer)` writes to any `std::io::Write`.
`parse_reader_to_markdown` accepts a capped input reader. `parse_to_markdown_with_cancel`
accepts a cancellation token. These writer APIs are available on native targets;
the WASM and Python bindings retain their existing structured conversion APIs.

## CLI

```sh
bulkparse parse workbook.xlsx --to markdown --output workbook.md
bulkparse parse large.csv --partial-on-limit --output preview.md
```

Markdown conversion and Markdown batch output use the writer path automatically.
Explicit OCR conversion keeps its existing provider path. Structured JSON output
and the existing `parse` APIs retain their complete-model contract.

Oversized documents are [handled by default](large-document-profile.md),
through both streamed Markdown and the full structured API. No large-document flag is required.

## Failure and recovery contract

Output is staged in a spool with a 64 KiB memory threshold; larger output spills
to a temporary file. Rendering uses an additional 64 KiB write buffer. This avoids
publishing an unfinished document if a later record/sheet fails, strict mode
rejects the conversion, or partial recovery restarts parsing. A failed first
attempt is discarded before recovery writes its output. The native writer caps Markdown, including
markup, at 128 MiB by default (`max_markdown_bytes`). Default work budgets now
support larger documents; explicit caller limits are never silently raised.

File-to-file APIs and CLI file output publish atomically and refuse to overwrite
an existing destination. A parsing or strict-mode error leaves the destination
untouched. A write error or cancellation while copying to a caller-provided writer
can leave a prefix in that writer; only a successful return certifies completion.
Temporary files are cleaned up when their owners are dropped.

Partial recovery remains explicit and never implies complete extraction. Inspect
`status`, `diagnostics` and `render_diagnostics`; the CLI exits with code 2 for
partial output. See [large-document recovery](large-documents.md).

## Memory boundaries and tradeoffs

Native XLSX/XLSM and ODS parsing also streams large worksheet XML parts from the
ZIP archive, validating and staging individual rows instead of retaining the full
XML text and tree. Large XLSX shared-string tables use indexed temporary storage
and a bounded lookup cache, preserving rich text without retaining every string
in memory. Parts up to 256 KiB keep the simpler in-memory path. Unusual rows that
contain document-wide metadata use the original tree parser to preserve semantics.
These input improvements also apply to the native structured APIs; WASM retains
its in-memory XML path.

Validated XLSX rows store compact extraction records, while ODS rows retain their
planning data so they do not need another XML parse just to calculate bounds and
cached values. Consecutive rows share namespace wrappers. Temporary storage uses
64 KiB write buffering and a 64 KiB read window to avoid a seek and read for every
small record.

XLS record indexes are budget-checked before allocation, avoiding a large index
for a workbook that will be rejected at the node limit. Plain-text table planning
also checks merged-grid bounds before constructing cell paragraphs. These checks
also apply to structured output; limits and error precedence are preserved.

Input bytes are still buffered up to their cap. Encoded CSV may need a decoded
text buffer. Styles, merge/layout data and one worksheet's sparse cell/block map
remain in memory, as does the current XML event or row fragment. XLSX/XLSM,
XLS/XLSB and native ODS Markdown output consume that map one row at a time,
releasing each final row after writing it instead of retaining the complete final
row/cell grid. XLSX rich-text decorations and hyperlinks are applied to each row
before writing; covered-cell content retains its original coordinates inside
merged cells. Cell/node limits are charged before any table rows are written,
preserving failure and recovery behavior. ODS also releases its template-coordinate
map before table assembly. Structured APIs still retain their returned table.
Successful multi-sheet workbooks release earlier sheets; a large single worksheet
can still determine the peak. Markdown-only XLSX/ODS recovery emits coordinate/value preview rows
as they are extracted; structured recovery still retains its returned model.
Other formats may materialize their model before rendering. This is not
end-to-end constant-memory parsing.

Staging adds temporary-disk I/O and requires a writable temporary directory. It
can be slower than returning an in-memory result. Bound concurrent conversions
according to available memory and disk capacity. Measured RSS is not a hard memory
guarantee, and the macOS measurements do not establish Linux production usage.

Measured results and regression checks (historical artifact; not distributed).
