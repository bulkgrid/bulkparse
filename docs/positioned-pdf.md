# Positioned PDF extraction — 2026-11-10

Follow-up: the subsequent [frozen holdout evaluation](holdout-evaluation.md) runs live AnyDoc and Docling on new inputs and records additional failures. The measurements and dependency limitations below describe the earlier milestone.

BulkParse now reconstructs native PDF text using glyph positions, font widths and page geometry. This fixes substantial text loss and reading-order problems in the newspaper, Elsevier and Arabic fixtures while keeping the default pipeline local and model-free. PDFs still report partial results: reliable table structure, heading classification and visual fidelity are not implemented.

## Measured text recovery

The same 173-file manifest was rerun against the previous parser. The following numbers are token-overlap recall against saved reference text, **not overall conversion accuracy**. Adjacent-token overlap offers a limited order signal; it cannot prove correct column, paragraph or table order.

| Fixture | Previous token recall | Current token recall | Current adjacent-token recall |
|---|---:|---:|---:|
| Newspaper | 2.7% | 89.6% | 78.7% |
| Elsevier paper | 2.6% | 98.5% | 95.7% |
| Arabic `right_to_left_01` | 11.4% | 91.9% | 86.0% |
| Normal four-page PDF | 98.6% | 100.0% | 96.5% |

All 100 primary quality fixtures converted without a failure; all 29 primary PDFs remain partial. Across all 173 inputs, results were 47 complete, 116 partial and 10 failures, including malformed, resource-abuse and encrypted inputs. Existing DOCX and CSV regression checks still pass. No primary PDF lost more than 0.5 percentage points of token recall.

There are tradeoffs. Adjacent-token recall for the normal four-page PDF decreased from 97.6% to 96.5%. A supplemental ODF presentation export decreased from 85.05% to 84.11% token recall. Newspaper hyphenation, complex column boundaries, rotated text and ambiguous mixed-direction text still need work. Recovering text from a table does not mean its rows and columns are reconstructed.

## Resource measurements

These are median full CLI subprocess measurements on Apple Silicon macOS, including startup and Markdown serialization: one warmup, seven recorded repetitions, rotating engine order, repeated OS caches. RSS is per-child peak resident memory. They are not server-load results or comparisons with live competitor engines.

| Fixture | Wall time before → after | CPU time before → after | Peak RSS before → after |
|---|---:|---:|---:|
| Newspaper | 9.57 → 9.93 ms | 8.19 → 8.56 ms | 14,480 → 14,608 KiB |
| Elsevier | 33.48 → 60.39 ms | 31.27 → 58.30 ms | 56,944 → 62,192 KiB |
| Arabic | 4.91 → 5.04 ms | 3.38 → 3.68 ms | 4,832 → 5,440 KiB |
| Normal four-page | 6.93 → 8.99 ms | 5.71 → 7.71 ms | 6,624 → 9,152 KiB |

The newspaper improves substantially at similar resource cost. Elsevier extracts much more text but takes about 1.8 times the wall time and 9% more median peak RSS. Further optimization needs to preserve this recovered content. No fastest or 10x claim is established.

The CLI grew from 3,285,856 to 3,436,704 bytes. The WASM module is 2,465,481 bytes, or 902,884 bytes with gzip level 9. The only added runtime dependency is optional `unicode-bidi`, enabled by the PDF feature; no OCR or layout model is bundled.

## Implementation

- Bounded ToUnicode CMap parsing supports compact delimiters, character mappings, ranges and UTF-16 sequences. Large ranges stay unexpanded. Simple-font fallback encodings and inconsistent one-byte codespaces receive explicit recovery handling.
- Glyph extraction applies font advances, character/word spacing, text and graphics transforms, inherited resources, Form XObjects and text-span `ActualText` replacement. Cycles and excessive nesting are diagnosed or rejected.
- Geometric line grouping and whitespace cuts recover columns and spanning headings as ordered text. Quarter-turn rotations use separate coordinate frames; other rotations remain uncertain.
- Directional glyph runs use Unicode bidi character classes, preserve Latin/digit order and retain atomic ligature strings. This is a geometric heuristic, not a complete inverse Unicode Bidirectional Algorithm.
- An optional, budgeted Rust OCR-provider interface routes image-containing pages with no native text to a caller-supplied provider. Native-text and blank pages are skipped. See [OCR integration and limits](pdf-ocr.md).

The core still uses `lopdf` for PDF objects, streams and content decoding. This milestone adds BulkParse's own text interpretation and layout reconstruction above that engine. Stream expansion limits are checked after decompression and are not a hard memory sandbox. OCR callbacks require provider-level time, pixel and memory limits. No actual OCR engine was installed or evaluated; only routing and budget behavior were tested with deterministic providers. Node, Python and WASM receive OCR-needed diagnostics, while provider registration is currently Rust-only.

Implementation references: [PDF 1.7 specification](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf), [PDF 2.0 text clauses and errata](https://pdf-issues.pdfa.org/32000-2-2020/clause09.html), and [Unicode Bidirectional Algorithm](https://www.unicode.org/reports/tr9/).

## Verification and reproduction

41 Rust integration tests pass, including 13 new positioned-PDF/OCR regressions. The minimal feature build passes 16 tests. Formatting and workspace Clippy pass. Seven Python harness tests pass. Rebuilt Node, Python, WASM and CLI outputs match exactly on 11 cases, including all three target PDFs. The WASM package installs and runs offline. See [validation details](validation.md).

```sh
cargo test --offline --locked -p bulkparse
python3 scripts/check_pdf_regressions.py reports/positioned-pdf-2026-11-10
python3 scripts/check_fixture_regressions.py reports/positioned-pdf-2026-11-10
node scripts/check_binding_parity.mjs --pdf-corpus
python3 scripts/benchmark_parsers.py --out reports/pdf-benchmark-next
```

The corpus checks require the original local fixture checkouts and raw evaluation outputs. The benchmark also requires the saved pre-change binary or a supplied pinned baseline. These artifacts are not redistributed in the source bundle. Input/reference hashes, source fingerprints, settings and results are retained in the corpus report (historical artifact; not distributed) and benchmark report (historical artifact; not distributed).

Live AnyDoc benchmarking was blocked by an uncached `cfb` dependency. Docling and its model artifacts are unavailable. The optional external-engine configuration and unexecuted Docling adapter are preparation for that comparison, not results. See [benchmark setup](../benchmarks/README.md).

Next priorities are a real bounded OCR provider with scan evaluation, stronger layout goldens, equivalent-profile live competitor runs, and allocation profiling on the Elsevier case before adding more formats.
