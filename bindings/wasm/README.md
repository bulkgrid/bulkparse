# BulkParse for browsers

Install the browser package with:

```sh
npm install @bulkgrid/bulkparse-wasm
```

Rust and wasm-pack are only needed to build from source. From this folder:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --locked
npm run build
npm test
npm run test:package
python3 -m http.server 8080
```

Open `http://localhost:8080/example/` for local file conversion in a Web Worker. The sample refuses files over 32 MiB, transfers input bytes to the worker, and displays output as text. It never uploads the chosen file. The main conversion API is synchronous; use a worker for larger inputs and terminate/recreate it to enforce a host timeout.

```js
import {init, parse} from '@bulkgrid/bulkparse-wasm';
await init();
const result = parse(new TextEncoder().encode('# Hello'), {filename: 'hello.md'});
console.log(result.markdown);
```

Serve the generated `.wasm` with MIME type `application/wasm`. WASM exports no file-path or network fetching API. The current build enables DOCX and positioned native-text PDF with heuristic column/directional ordering. No OCR engine or learned layout model is included; page-level diagnostics identify image-only OCR candidates. The current release WASM binary is 2,540,071 bytes (923,087 bytes with gzip level 9). Smoke tests and DOCX conversion in the in-app browser pass; broader browser compatibility is not yet established. Byte copies and JSON serialization still consume memory; the parser limits are not a hard memory sandbox.

`capabilities()` and output types match the native bindings. Bundlers can import the package wrapper; the generated `pkg` directory must be included with it. Package-level TypeScript declarations describe the same model as Node.


For offline builds, first install a `wasm-bindgen-cli` version matching the resolved `wasm-bindgen` dependency in Cargo.lock (currently `0.2.130`):

```sh
cargo install wasm-bindgen-cli --version 0.2.130 --locked
CARGO_NET_OFFLINE=true npm run build
```

The build script uses a workspace-local tool cache unless `WASM_PACK_CACHE` is set. It keeps WASM target metadata needed by wasm-bindgen. Packaging checks require the generated JS, TypeScript and WASM assets and override wasm-pack's generated ignore rule so npm includes the engine. `npm run test:package` packs the wrapper, installs that tarball in a temporary directory without network access, and converts DOCX through the installed package. No package is published by these commands.

CSV supports automatic comma/semicolon/tab/pipe detection and BOM-aware UTF-8/UTF-16 decoding. Without a BOM or explicit encoding, valid UTF-8 is preserved; otherwise legacy Windows-1252 decoding is attempted and reported with a `csv_encoding_inferred` diagnostic. Encoding inference is ambiguous, particularly for mixed-encoding files. Set `csv_encoding` to `utf8` for strict UTF-8, `windows1252` for a known legacy export, or `utf16_le` / `utf16_be` for BOM-less UTF-16. Explicit encodings must agree with any BOM. Undefined Windows-1252 bytes and binary controls are rejected. For delimiter ambiguity, pass `csv_delimiter` (one ASCII character); headers remain explicit through `csv_has_headers`.

PDFs support conservative ruled-table reconstruction. Arabic presentation forms and alphabetic ligatures expand by default. Pass `pdf_normalize_unicode: false` (Python: `pdf_normalize_unicode=False`) to retain mapped forms. `ActualText` is always preserved. See the root `docs/pdf-tables.md` for limits and validation.

PDF inspection is available without an OCR runtime: `inspectPdf(bytes)`. Conversion results also include `pdf_inspection`. The shared output schema is now 0.8. See [inspection and routing](../../docs/pdf-inspection.md).

The standard build enables BulkParse-owned Office/EPUB readers. See [format support and limits](../../docs/office-formats.md).

For resource-limited spreadsheets and CSV files, opt in to `partial_on_limit` (Python: `partial_on_limit=True`). Successful normal conversions are unchanged; supported limit failures can return an explicitly partial prefix within the original per-attempt budgets. This is not complete extraction and strict mode never accepts the recovery result. Inspect `status` and `resource_limit_partial` diagnostics. See [bounded large-document handling](../../docs/large-documents.md). Rebuild the binding to use this option.

Embedded image bytes are available in `document.assets` (base64 JSON). See [image retention, export and limitations](../../docs/images.md).

## OCR and standalone images

See [OCR across language bindings](../../docs/ocr-bindings.md) for the shared evidence API, browser-hosted recognition, image/frame support, scanned-table limits, and examples.
