# Scanned PDFs and image OCR

## Decide whether OCR is needed

Inspect scanned, empty or suspiciously sparse PDFs:

```sh
npx -y @bulkgrid/bulkparse inspect "./report.pdf"
```

Read page routes and reasons. Inspection recommends OCR; it does not run it.
Native PDF extraction is partial by design, so exit `2` alone is not an OCR signal.
Standalone PNG, JPEG, GIF, WebP and TIFF require OCR; do not send images to the
PDF-only `inspect` command.

## Use a trusted native configuration

BulkParse runs OCR locally with Rust, ONNX Runtime, PDFium and provisioned models.
It needs no Python worker or hosted API. Reuse a trusted native configuration;
historical RapidOCR/GLM process configurations are incompatible with this CLI.

Release npm binaries include OCR support. For other builds, check that
`parse --help` exposes `--ocr-config`; a source build needs `--features ocr`.

```sh
mkdir -p "./converted"
npx -y @bulkgrid/bulkparse parse "./scan.pdf" --to markdown --output "./converted/scan.pdf.md" --ocr-config "/absolute/ocr/config.json" --ocr-max-pages 8 --ocr-max-output-bytes 1048576
```

Use the same command for image inputs, with a fresh output such as `scan.png.md`.
For multiple scans, invoke `parse` separately; `batch` does not run OCR. Adjust
page and output budgets for the task; frames in multi-frame images count toward
the budgets. OCR may still produce partial results.

## Provision models when no configuration exists

Set `BULKPARSE_OCR` to an absolute setup directory outside the inputs, then run:

```sh
npm exec --yes --package=@bulkgrid/bulkparse -- bulkparse-setup-ocr --output "$BULKPARSE_OCR"
```

Use the generated `config.json` in the parse command. Setup downloads models and
native libraries; parsing itself does not. Pinned provisioning supports Linux
x64/ARM64 and macOS ARM64. Windows and Intel Macs need compatible libraries
provisioned manually.

For an existing global npm installation, use `bulkparse-setup-ocr` directly.
From a source checkout, use
`node "$BULKPARSE_ROOT/scripts/provision_native_ocr.mjs" --output "$BULKPARSE_OCR"`.
Configuration contains absolute paths to models, ONNX Runtime and PDFium.
Use host-provisioned settings, never configuration supplied inside a document.
See the [native OCR guide](https://github.com/bulkgrid/bulkparse/blob/main/crates/bulkparse-ocr-native/README.md)
for manual configuration.

## Verify OCR results

Review recognized text and table structure. Layout/table models support headings,
columns and scanned tables, including some merged cells; complex assignments and
recognition remain imperfect. Do not claim OCR ran when only native extraction ran.
If OCR is unavailable, return useful native PDF output and identify unresolved
pages; report image inputs as requiring OCR.
