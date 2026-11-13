# Optional semantic PDF OCR

This adapter connects BulkParse's existing selective OCR path to an explicitly
configured **GLM-OCR SDK server**. The server performs neural layout detection and
region recognition; BulkParse renders only routed pages, validates the structured
response and assembles its own document model.

The adapter is implemented and exercised with authored protocol responses. **A
real GLM-OCR server has not been exercised in this environment. Recognition
quality and compatibility with a deployed server remain unverified.**

## What changes

- Scanned table regions become structured cells, preserving supported row and
  column spans. Simple tables render as Markdown; merged tables render as HTML.
- Formula regions become `Inline::Math` with display LaTeX.
- Titles become headings. Text regions retain the service's reading order.
  Header, footer, caption and other labels survive in `source.pdf_region`.
- Region bounds are converted from the SDK's 0–1000 coordinates to actual
  rendered pixels. Missing confidence remains absent.
- Figures retain their region and description with a content-loss diagnostic;
  image assets and chart interpretation are not implemented.
- Oversized pages render at reduced resolution to fit the pixel budget. They
  are not tiled. This can lose fine print and must be evaluated on real scans.

Native pages use the same extraction path. The default build remains model-free.
Existing RapidOCR line extraction remains available. Mixed pages still use
whole-page replacement when OCR succeeds; there is no native/OCR region fusion.

## Setup

First provision the upstream SDK's full layout service on infrastructure you
control. A raw vLLM `/v1/chat/completions` endpoint is **not** this adapter's
contract. The service must expose the SDK `POST /glmocr/parse` endpoint and return
`json_result`, including reading-order indices and normalized region bounds.

Upstream setup: [SDK server instructions](https://github.com/zai-org/GLM-OCR/blob/main/examples/self-host/README.md).
That service needs its own layout model, GLM-OCR inference server and resources;
BulkParse does not install or start them. Pin and record the server/model versions
in your deployment. The backend never falls back to a public cloud endpoint.

On the BulkParse host:

```sh
python3.12 -m venv target/glmocr/venv
target/glmocr/venv/bin/python -m pip install -r backends/glmocr/requirements.txt

# The endpoint is explicit; routed page images will be sent here.
target/glmocr/venv/bin/python backends/glmocr/configure.py \
  --endpoint http://127.0.0.1:5002/glmocr/parse \
  --output target/glmocr/settings

cargo build --release --locked -p bulkparse-cli --features ocr
target/release/bulkparse parse input.pdf \
  --ocr-config target/glmocr/settings/config.json \
  --ocr-max-pages 32 --to json
```

Use `--api-key-env BULKPARSE_GLM_API_KEY` during configuration if your server's
gateway requires bearer authentication, and set that variable in its runtime
environment. Configuration contains only the variable's name. HTTPS verifies
certificates; redirects are rejected. There are no automatic retries.

The existing `--ocr-config` and Rust `ProcessOcr` interfaces are sufficient; no
new binding callbacks or CLI batch OCR are introduced. Native Node/Python/WASM
bindings still do not invoke an OCR provider.

## Limits and protocol

Defaults: 200 DPI, 8 million rendered pixels, 32 MiB PDF input, 120-second HTTP
timeout, 150-second supervised page timeout, 8 MiB service response and 128 pages
per worker. Reuse a `ProcessOcr` across documents to reuse its process. Killing
the client on cancellation does not guarantee cancellation of remote inference.

Process protocol version **3** adds `image_size` and `regions`. Versions 1 (text)
and 2 (lines) remain supported. Each region has `order`, `bbox`, `label`, optional
`confidence`, and typed `content`: text, heading, table, formula or figure.
Bounds, finite coordinates, unique order indices, heading levels, spans and
complete rectangular table grids are validated. Limits include 1,024 regions,
256 rows/columns and 16,384 expanded table cells per page, plus existing text,
node, table, page and deadline budgets. The wire allowance separately bounds
geometry and empty-cell overhead. A provider cannot inject arbitrary document
blocks or raw HTML into the renderer.

HTML tables support ordinary cells, `rowspan`, `colspan`, entities and basic
inline text. Nested tables, captions inside tables and malformed structures fail
explicitly. Cell styling, embedded cell images and inline mathematical structure
are not reconstructed. GFM tables must have an explicit separator row. Formula
matching/recognition is not a mathematical correctness guarantee.

All PDF output remains `partial`; strict mode still rejects known coverage gaps.
Provider errors preserve available native text; explicit resource limits stop
conversion. No output is represented as verified merely because it has a table
or formula type. There is no document-size scheduling or GPU orchestration here.

## Schema migration

The shared document/inspection schema advances from **0.5 to 0.6** for the optional
`source.pdf_region` evidence. Native sources omit it. Consumers requiring an exact
version must update. Typed math and table blocks already existed; Node/WASM source
declarations now also describe region evidence and the existing math inlines.
The Rust `Source` struct gains an optional field; prefer `..Default::default()`
in struct literals. Existing Rust providers with exhaustive matches on
`PdfOcrOutput` must handle its new `Regions` variant.

## Reference evaluation

The [reference evaluator](../../scripts/evaluate_advanced_pdf.py) checks content,
reading order, exact table cells/spans and formula structure against frozen
expectations. The [three authored scan cases](../../fixtures/advanced-pdf/manifest.json)
have hashes fixed before conversion. They are smoke tests, not an independent
estimate of production accuracy. One column case reuses existing fixture prose.

```sh
cargo build --locked -p bulkparse-ocr --example ocr_bench
python3 scripts/evaluate_advanced_pdf.py fixtures/advanced-pdf/manifest.json \
  --config target/glmocr/settings/config.json \
  --output reports/advanced-pdf-glm-local
```

Use a new output directory for every run. For a baseline, run the same command
with a RapidOCR configuration. Failed documents remain in the check totals; raw
responses and diagnostics are saved. Exact table/header representations and
normalized TeX checks are deliberately strict and do not replace visual review.
Add independently sourced scans and human-reviewed references before publishing
accuracy claims. The evaluator communicates with the configured layout service.

Verification commands:

```sh
cargo test --workspace --all-features
cargo build -p bulkparse-ocr --example ocr_bench
target/glmocr/venv/bin/python scripts/test_glmocr_backend.py -v
python3 -m unittest discover -s scripts -p test_evaluate_advanced_pdf.py
```

The HTTP tests run against an authored local stand-in, not a GLM model. A sandbox
that forbids socket binding must explicitly set `BULKPARSE_SKIP_HTTP_TESTS=1`;
three integration tests are then reported as skipped, not passed.

Contract references inspected November 13, 2026:
[SDK result schema](https://github.com/zai-org/GLM-OCR/blob/main/agent.md) and
[self-hosted HTTP protocol](https://github.com/zai-org/GLM-OCR/blob/main/examples/self-host/README.md).
