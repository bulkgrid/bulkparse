# Bounded large-document recovery

Full output for larger documents is now [supported by default](large-document-profile.md),
with streamed Markdown or the complete in-memory structured API. This page describes
the separate, opt-in prefix recovery behavior when a supported limit is exceeded.

The standard parser returns an error when a resource limit is reached. Enable
`partial_on_limit` when a useful prefix is preferable to no output. This is
available for CSV/TSV, XLS, XLSX/XLSM and ODS. It is disabled by default and never
returns recovered output in strict mode.

```rust
let parser = bulkparse::Parser::new(bulkparse::Options {
    partial_on_limit: true,
    ..Default::default()
});
```

Node and WASM accept `{ partial_on_limit: true }`. Python accepts
`parse(data, filename="large.xlsx", partial_on_limit=True)`. Rebuild the binding
to include the updated core. The CLI exposes:

```sh
bulkparse parse large.xlsx --partial-on-limit --to json
```

A partial CLI result exits with code **2**, writes output, and emits diagnostics;
this is the existing partial-result convention. The flag conflicts with `--strict`.

## Output contract

Successful normal conversion is unchanged. On a supported resource-limit failure,
the failed attempt releases its intermediate structures and recovery starts with
the **same limits**. Limits are not increased. A second attempt does additional
work, so the combined work is not limited to one attempt's work budget.

- CSV/TSV returns complete records only. `csv_rows_truncated` gives the retained
  count and first omitted record when the output budget is reached.
- XLS returns completed worksheets only. The unfinished worksheet and subsequent
  sheets are omitted and identified by `xls_sheets_truncated`.
- XLSX/XLSM and ODS return populated cells as Row/Column/Value tables. Source
  coordinates are preserved in the document model. `spreadsheet_cell_preview`
  records the retained count and last retained coordinate. Empty ranges do not
  expand into a huge rectangle. These previews omit original layout, merges,
  styling, notes, links and chart reconstruction. XLSX uses raw cached values,
  without numeric/date formatting or formula calculation.

Every recovered result has `status: partial` and a `resource_limit_partial`
diagnostic with `content_loss: true`. It must not be reported as a complete
conversion. Markdown alone cannot express all coverage information; inspect the
structured status and diagnostics.

## Bounds and validation

Input size, output text, nodes, cells, depth, page counts, archive entries and
expanded-byte limits remain in place. XML recovery streams through capped member
reads, rejects DTDs and invalid archive paths, and stops before growing the output
past its budget. Repeated ODS rows/cells are expanded only while output fits.
Duplicate XLSX coordinates and malformed XML encountered in the consumed prefix
are errors. Cancellation is checked during scanning and output construction.

The unexamined tail is not validated, and a ZIP-member checksum is not certified
when reading stops early. Input-size rejection is not bypassed. An empty recovery
is an error. Large shared-string tables can stop recovery before a requested
value becomes available; the parser never invents a substitute cell value.

These are parser budgets, not a hard process-memory cap. The normal parsing
attempt happens first. XLSX/XLSM and ODF XML parts larger than 256 KiB now get a
streaming node-budget preflight before their text or document tree is allocated.
The preflight retains only the current XML event and a read buffer, and rejects
parts whose guaranteed node count already exceeds the remaining budget. Adjacent
text, entities and CDATA are counted as one coalesced text node; final validation
and exact accounting still belong to the document-tree parser.
Accepted large parts are decompressed again for full extraction; smaller parts
keep the direct path. ZIP text buffers become strings without a second copy.
These are general size and budget rules, with no corpus identifiers in the parser.

This substantially reduces the measured oversized-spreadsheet peaks. It is not
end-to-end constant-memory parsing: inputs, accepted document trees, the output
model and Markdown can still coexist; a single large XML event also needs storage.
Use worker deadlines and operating-system memory limits when a hard boundary is
required. The [Markdown-only writer](streaming-markdown.md) now streams CSV records
and releases completed spreadsheet worksheets, avoiding a full output model and
Markdown string together. Native successful XLSX/XLSM and ODS parsing now stages
large worksheet XML by row, and large XLSX shared-string tables use indexed
temporary storage. Input bytes, worksheet cell models and layout metadata remain
buffered; higher-fidelity recovery and streaming the cell model remain future work.

For Markdown-only XLSX/ODS recovery, coordinate/value preview rows are now written
as they are extracted, without retaining the complete preview table. The
structured API still returns the full retained-prefix model. Both paths use the
same budgets, stopping points and diagnostics. Failed-attempt context is released
before recovery, and known-width table rows reserve only their required cell slots.
CSV checks the exact extraction budgets in a reusable-record pass before building
a table or staging Markdown, avoiding work that a limit failure would discard.

Per-document memory profiling and before/after benchmark (historical artifact; not distributed).

Benchmark, source-value checks and regression results (historical artifact; not distributed).
