# Embedded images

BulkParse retains supported Office and EPUB images in `document.assets` by default.
Standalone files are preserved where possible; legacy DIB payloads are wrapped as
BMP. **PDF image decoding and export are not supported.** This matches the PDF
image behavior of the AnyDoc 0.2.4 implementation used in our comparison. PDFs
still support text extraction, structured output, image-aware inspection and
separately configured OCR; image geometry is inspected without exporting pixels.

The inventory can include unused
package resources, thumbnails and alternate previews. Known references are also
retained as `Inline::Image` entries with `asset:<id>` URLs in the reading flow. Each asset has a
content-derived `id`, `media_type`, first-origin `source`, and `data` (base64 in
JSON; shared bytes in Rust). Identical files with the same media type share one
asset. Schema 0.8 adds this field; older documents deserialize with empty assets.

Ordinary Markdown retains its existing text/alt-text behavior. Embedded image
bytes are not inserted as data URLs or automatically saved to disk. Existing
external image links remain links; BulkParse does not fetch them.

## Save Markdown and embedded image files

```sh
bulkparse parse document.docx -o document.md --export-images
```

```js
const summary = await parseFileToMarkdown('document.docx', 'document.md', {
  export_images: true,
});
console.log(summary.images); // relative paths, MIME types, origins, byte counts
```

```python
from bulkparse import parse_file_to_markdown
summary = parse_file_to_markdown('document.docx', 'document.md', export_images=True)
```

Rust uses `Parser::new(Options { export_images: true, ..Default::default() })`
and `parse_file_to_markdown(input, output)`.

Images are saved in a uniquely named `bulkparse-images-*` directory beside the
Markdown. **Saving assets does not rewrite the Markdown.** Embedded images render
as their alt text, with no added image links or additional-assets section, matching
AnyDoc’s behavior. Existing external image URLs remain Markdown images.

Structured output retains image references (`asset:<id>`) near related text where
resolved, including table cells, slides and Word auxiliary stories. The saved-file
summary maps each asset ID to a path; applications can use that mapping if they
want to present images. Unresolved resources can still be saved, but no reading-flow
location is invented. Exact page geometry is not reconstructed.

An agent needs access to the saved files and image-viewing tools to inspect them.
Markdown containing alt text alone does not provide a file link or transmit image
pixels. Original files are not sanitized; treat document assets as untrusted input
when opening them with other software.

Existing output files are never overwritten. A handled parsing, strict-mode, I/O
or Markdown-publication failure cleans up staged images. Markdown publication is
atomic; the directory and file are not a single filesystem transaction, so abrupt
process termination can leave an orphan staging directory. File export is for
native Node, Python, Rust and CLI APIs. WASM returns assets in structured output;
the application decides how to download or display them. `export_images` applies
only to the file-to-Markdown API. Combining CLI image export with the OCR CLI path
is not yet supported.

## Limits and supported sources

`extract_images: false` (CLI `--no-images`) disables image retention. Defaults:
`limits.max_image_bytes = 67_108_864` (64 MiB total retained image bytes) and
`limits.max_images = 1000`. Image limits preserve extracted text and add an
`image_asset_unavailable` diagnostic instead of failing the entire document.
Strict mode rejects such partial results. ZIP image expansion is also bounded;
read buffers and base64 serialization add overhead beyond retained asset bytes.
The limits are not a whole-process memory ceiling.

| Source | Current image retention |
| --- | --- |
| DOCX/DOCM, PPTX, XLSX/XLSM/XLSB, ODT/ODS/ODP, EPUB | Recognized image files in the package; extension-based inventory, including potentially unused resources |
| RTF | `pict` PNG/JPEG/EMF/WMF and DIB-to-BMP, hex or binary payloads |
| DOC | PICF image records referenced by character formatting; supported OfficeArt PNG/JPEG/EMF/WMF/DIB |
| PPT | Supported OfficeArt images in the Pictures stream |
| XLS | Supported OfficeArt images in BIFF drawing-group records |
| PDF | No image assets or export; text extraction and image-aware inspection only |

`extract_images` and `export_images` do not enable PDF image export. Requesting
file export for a PDF still writes its Markdown, returns an empty image list and
leaves no image directory. PDF image encodings do not incur pixel-decoding work
through this feature. PDF inspection still records image counts and coverage for
OCR routing. Ordinary image retention never invokes OCR.

Unsupported legacy OfficeArt/OLE representations, inline ODF binary-data images
and notebook image outputs remain outside this feature. Legacy nested tables,
floating geometry, full rich styles and individual note links remain partial.
External resources are not fetched. Unrecognized package extensions are not
inventoried. Retaining resources does not prove complete visible-content coverage,
correct placement, exact page compositing or interpretation of image contents.

## OCR and image understanding

Retaining images does not run OCR. BulkParse's current OCR integration processes
selected PDF pages; embedded Office images are not automatically sent to it.
OCR reads text in pixels. Understanding a chart, diagram or photograph requires
an image-capable model or a separate specialized analysis step.
