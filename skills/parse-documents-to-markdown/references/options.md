# Format options and diagnostics

Use these options with individual `parse` calls. Batch does not expose CSV
settings, image export or OCR. Run `parse --help` when a flag is unavailable in
the installed build.

## CSV, TSV and format detection

- Add `--csv-has-headers` only when the source has a real header row.
- Delimiter and encoding detection are automatic. Override with
  `--csv-delimiter ';'` or `--csv-encoding windows1252` when needed.
- Use `--format` for incorrectly named files or signature-less standard input:

```sh
npx -y @bulkgrid/bulkparse parse - --format csv --to markdown --output "./converted/data.csv.md" < "./data.csv"
```

Create the output parent directory first. Keep file output for large documents
instead of sending all Markdown into the conversation.

## Resource limits

Report resource-limit errors. Increase `--max-input-bytes` only when the source
and available resources justify it; the default is 32 MiB. For a useful truncated
spreadsheet/CSV prefix, `--partial-on-limit` can preserve bounded output with
diagnostics. Label the output partial; this flag conflicts with `--strict`.

Capture partial exit codes without losing output in shells that use `set -e`:

```sh
if npx -y @bulkgrid/bulkparse parse "./report.pdf" --to markdown --output "./converted/report.pdf.md"; then
  conversion_status=0
else
  conversion_status=$?
fi
```

## Embedded images

Add `--export-images` to a file-to-file Markdown `parse` command for supported
Office/EPUB assets. Images are saved separately; Markdown still contains alt
text rather than new image links. PDF image export and combining image export
with the OCR CLI path are unsupported. Exporting assets does not interpret their
contents. See the [image guide](https://github.com/bulkgrid/bulkparse/blob/main/docs/images.md)
for coverage and limits.

## Structured evidence

For coverage, diagnostics and provenance, run `parse --to json` into a separate
fresh file. Use it as supporting evidence alongside the requested Markdown;
nonempty output and diagnostics alone do not prove faithful reproduction.
