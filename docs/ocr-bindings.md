# OCR across Node, Python and WASM

OCR is explicit. The ordinary `parse`, `parseFile` / `parse_file`, and PDF
inspection calls do not load models or start an OCR process.

All three bindings accept the same validated OCR evidence and return the same
structured document, Markdown, diagnostics and page provenance. Node and Python
also expose reusable Rust OCR with native ONNX Runtime and PDFium. WASM accepts recognition results
from a browser or host OCR engine; it does **not** bundle an inference engine,
model weights, a PDF renderer, or Python.

## Node.js

```js
const { OcrParser } = require('@bulkgrid/bulkparse');
const fs = require('node:fs');

// Trusted deployment configuration, never supplied by a document or queue job.
const config = JSON.parse(fs.readFileSync('/opt/bulkparse/ocr-config.json', 'utf8'));
const parser = new OcrParser(config);
try {
  const result = await parser.parseFile('/data/input.tiff', {}, {
    max_pages: 2000,
    max_output_bytes: 32 * 1024 * 1024,
  });
  console.log(result.markdown);
  console.log(result.status, result.diagnostics);
} finally {
  parser.close();
}
```

`parser.parse(bytes, options, budget)` accepts a Buffer or Uint8Array. Native
conversion runs off the event loop. Reuse one parser instance across documents
to reuse verified model sessions. `close()` releases the instance's model sessions;
already submitted Node jobs retain it until they finish. Bound application
concurrency: each provider serializes access to its model sessions.

## Python

```python
import json
from bulkparse import OcrParser

with open('/opt/bulkparse/ocr-config.json') as file:
    config = json.load(file)
with OcrParser(config) as parser:
    result = parser.parse_file(
        '/data/input.tiff',
        budget={'max_pages': 2000, 'max_output_bytes': 32 * 1024 * 1024},
    )
    print(result['markdown'])
    print(result['status'], result['diagnostics'])
```

`parser.parse(data, filename=..., budget=..., **options)` accepts bytes or
bytearray. Conversion releases the GIL. The context manager releases the native model sessions;
`close()` is also available explicitly.

## Browser/WASM and supplied evidence

```js
import { init, parseWithOcr } from '@bulkgrid/bulkparse-wasm';
await init();

// Produced by your OCR engine from the actual input image. Coordinates are pixels.
const evidence = {
  page_count: 1,
  pages: [{
    page: 1,
    output: {
      type: 'lines',
      value: {
        image_size: [800, 600],
        lines: [{
          text: 'Recognized text',
          quad: [[20, 20], [220, 20], [220, 50], [20, 50]],
          confidence: 0.95,
        }],
      },
    },
  }],
};
const result = parseWithOcr(imageBytes, evidence, { filename: 'input.png' });
```

This is an evidence assembly API, not image recognition. The host must decode
and recognize the actual input, enforce pixel/runtime limits in its engine, and
supply all requested frames. The Rust core checks page counts, duplicate pages,
text budgets, geometry and table spans. It cannot verify that supplied text
matches the image pixels. Run WASM in a Web Worker for responsiveness.

The same function is available as Node's asynchronous `parseWithOcr` and Python's
`parse_with_ocr(data, evidence, filename=..., budget=..., **options)`.

The evidence envelope is `{page_count, pages: [{page, output}]}`. Output supports:

- `{type: 'text', value: 'plain Unicode text'}`
- `{type: 'lines', value: {image_size, lines}}`
- `{type: 'regions', value: {image_size, regions}}`

Regions can represent headings, paragraphs, tables with row/column spans,
formulas and figure descriptions. Region schemas match the types in the Node
and WASM declarations. This extends the existing semantic OCR provider contract.

## Input support and limits

The Rust native OCR provider accepts PNG, JPEG, GIF, WebP and TIFF, including
multiple frames. Transparent pixels are composited onto white and EXIF
orientation is applied. The document format is `image`; frame numbers appear
in block sources and coverage. All frames are considered, including animations.
Custom providers must return `page_count` to support image inputs.

PDFs use the inspector's existing OCR routing. `pdf_force_ocr: true` (Python:
`pdf_force_ocr=True`) requests whole-page OCR even for pages routed to native
extraction, allowing an application to retry empty output. Native content is
retained if recognition fails or is empty. Inspection evidence stays separate
from this override.

## Setup and resource budgets

```sh
node scripts/provision_native_ocr.mjs --output /absolute/ocr
```

Use the generated `config.json`. Setup downloads checksum-pinned models and native
libraries explicitly. Parsing never downloads files or launches Python. The native
provider verifies model hashes before loading. See [configuration](../crates/bulkparse-ocr-native/README.md).

Core OCR defaults are 8 pages and 1 MiB of recognized text per call; pass larger
budgets explicitly when needed. Parser input/output/page limits also apply. Native
configuration defaults are 8 million decoded pixels, 256 MiB OCR input, 2000
maximum image frames, one inference thread, 30 seconds per page and 120 seconds
per document. The provisioning script sets both deadlines to 120 seconds.

Binding deadlines are **cooperative** between native stages: an active ONNX call
must return before cancellation is observed. Applications requiring a hard limit
must supervise the binding in a process. Bulkgrid's TypeScript service does so,
killing a timed-out Node child before accepting another conversion. Calls through
one provider are serialized and reuse models; no per-page process is started.
`close()` releases the instance; already submitted calls retain it until complete.

WASM remains an evidence assembly API. Browser inference requires a separate
browser-compatible engine; native ONNX Runtime/PDFium cannot load in WASM.

## Layout and quality

Rust performs image decoding, alpha compositing, orientation handling, model
orchestration, geometry validation and table assembly. Native inference supplies
text detection/recognition, line orientation, page layout and table structure.
Visible-grid rules support clear merged cells; the optional layout/table models
also handle borderless tables. Unclaimed OCR lines are retained when layout or
table predictions are invalid. Headings and column reading order use layout
regions. This is not a guarantee of correct structure on every scan.

The default pipeline does not perform formula-to-LaTeX recognition or describe
images. The previous service did not enable formula enrichment. Custom semantic
providers can still supply formulas and figures through the evidence API.

See [measured validation and known gaps](ocr-bindings-validation.md). Python files
under benchmark tooling and legacy adapters are retained for comparisons; they
are not dependencies of the native bindings, CLI or service deployment.
