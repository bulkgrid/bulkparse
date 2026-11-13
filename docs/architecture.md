# BulkParse architecture

The goal is high conversion quality at low operating cost. “10x better” is a benchmark target for defined workloads, not a property guaranteed by Rust or format count.

## Present implementation

The workspace contains a portable core, native CLI, three thin adapters, and an optional native process host. The core detects input, applies format-specific parsing, builds one typed document model, and renders Markdown or serializes JSON. Parsing logic stays in Rust; bindings move bytes, options, results, and errors.

Detection prefers PDF/ZIP content signatures over a filename. Ambiguous text requires a filename or explicit format. Binary formats must match their signature. DOCX uses archive metadata and XML relationships without extracting files to disk. XML DTDs are disabled; package paths cannot escape the logical package root.

The canonical model represents blocks, nested lists/quotes, inline formatting, tables, source locations, diagnostics and coverage. A generated JSON Schema comes from Rust types. TypeScript declarations currently mirror the model manually; generated declarations and cross-language golden tests are required before a stable release.

The native bindings enable DOCX/PDF. Node schedules work through N-API AsyncTask; Python detaches from the GIL. WASM uses the same core and exports byte-based conversion. No filesystem APIs are exported in WASM. Browser calls are synchronous inside the worker; transferring the input ArrayBuffer avoids one host-side copy, but the WASM boundary and output JSON still allocate.

The CLI converts one file or processes a directory sequentially. It never starts a hidden process pool. Temporary output files are persisted without replacing existing files. Batch output includes original extensions to prevent collisions between `report.pdf` and `report.docx`.

The separately selected [bulkparse-pool host](worker-pool.md) bounds waiting and active work, supervises persistent worker processes, and enforces deadlines/cancellation by terminating a child when necessary. Jobs receive a single terminal report; crashed work is not replayed automatically. Bulkgrid still owns upload authorization, durable queueing, retry policy, storage and deployment resource limits.

## Engine choices and competition

| Project | Approach observed in reviewed source | Implication for BulkParse |
| --- | --- | --- |
| AnyDoc | Rust with format-specific extraction, ZIP/XML/CFB libraries, PDF path using pdf-inspector/lopdf; native and WASM adapters | Rust and bindings alone are not a differentiator |
| Docling | Python orchestration; format backends such as python-docx, python-pptx, openpyxl; docling-parse PDF engine; optional layout/OCR pipelines | Mature document understanding is the quality baseline, especially for difficult PDFs |
| BulkParse prototype | Rust semantic mapping over pulldown-cmark, csv, serde_json, zip/roxmltree and lopdf | Reuse low-level engines, own the common model, limits, diagnostics and host contract |

These observations describe the source reviewed, not a maintained current feature comparison. Links: [AnyDoc](https://github.com/firecrawl/anydoc), [Docling](https://github.com/docling-project/docling), [docling-parse](https://github.com/docling-project/docling-parse). Recheck specific versions before publishing comparative claims. License and distribution audits are required before distributing dependency bundles or models.

## Design to develop next

1. **Format-specific fidelity.** Resolve Office structure directly where available. Preserve headings, lists, formulas, tables and source references rather than flattening everything into text.
2. **Selective expensive processing.** Add page-level PDF quality signals and optional OCR/layout backends. Process only pages needing them, with an explicit budget. Confidence thresholds require calibration against labeled data.
3. **Bounded work.** Introduce streaming/event APIs where the format supports them, cancellation checkpoints, page limits and backpressure. Do not advertise streaming solely because input accepts a reader.
4. **Small deployment profiles.** Keep expensive models, rendering engines and legacy format dependencies optional. Measure compressed WASM, installed package size, cold starts and peak RSS separately.
5. **Visible conversion gaps.** Keep missing text, unsupported objects and renderer loss machine-readable. Distinguish extraction quality from presentation fidelity.
6. **Host independence.** Bulkgrid handles storage, queues, retries, tenants and billing. The community library remains usable without a Bulkgrid account or network access.

## Measurement plan

Use an openly licensed corpus with representative sizes and difficulty levels. Keep native-text PDFs, scans, tables, multi-column layouts, Office files and text formats separate. Pin all tool versions and settings. Compare equivalent output and quality, not a text extractor against an OCR pipeline under different requirements.

Record wall time (cold/warm p50/p95), CPU seconds, peak RSS, throughput at fixed concurrency, package size, extracted text accuracy, reading order, table structure and reported failures. Count partial and failed conversions in the results. Publish hardware, corpus hashes, raw measurements, commands and quality annotations. A 10x claim needs a named workload and equal or better measured quality. GPU-backed quality profiles must be compared and costed separately.

Local [quality evaluations](holdout-evaluation.md), [persistent-worker comparisons](worker-benchmarks.md), and a [host lifecycle exercise](worker-pool.md) are available. Capability differences and workload limitations are recorded alongside the measurements; they do not establish general superiority or production readiness.
