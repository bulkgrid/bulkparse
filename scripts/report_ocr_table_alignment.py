#!/usr/bin/env python3
"""Write the verified OCR/table study and refresh the README from saved results."""
from pathlib import Path
import json
ROOT=Path(__file__).resolve().parents[1]
REPORT=ROOT/'reports/ocr-table-alignment-2026-11-11'
COMPARE=ROOT/'reports/same-machine-ocr-table-alignment-2026-11-11'
def read(p):return json.loads(p.read_text())
def main():
    assert read(REPORT/'validation.json')['passed'] and read(COMPARE/'validation.json')['passed']
    rows=read(COMPARE/'comparison.json');by={r['engine']:r for r in rows}
    old=read(ROOT/'reports/same-machine-pdf-2026-11-11/comparison.json')
    previous=next(r for r in old if r['engine']=='bulkparse')['metrics']
    icdar=read(ROOT/'reports/icdar-ocr-table-alignment-2026-11-11/quality.json')['summary']
    q=read(ROOT/'reports/independent-ocr-table-alignment-2026-11-11/quality.json')['summary']['current']
    table='| Engine | Overall | Reading order (NID) | Tables (TEDS) | Headings (MHS) | Time / 200 PDFs | Returned / attempted |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: |\n'
    names={'bulkparse':'BulkParse 0.1.0','anydoc':'AnyDoc 0.2.4','docling':'Docling 2.150.0'}
    for engine in ['bulkparse','anydoc','docling']:
        r=by[engine];m=r['metrics'];v=[f"{m[k]:.3f}" for k in ['overall_mean','nid_mean','teds_mean','mhs_mean']]
        if engine=='bulkparse':v=[f'**{s}**' for s in v]
        table+=f"| {names[engine]} | {' | '.join(v)} | {r['median_seconds']:.3f} s | {200-len(r['failed_documents_last_pass'])}/200 |\n"
    times='| Engine | Five measured full-corpus times (seconds) | Median |\n| --- | --- | ---: |\n'
    for r in rows:times+=f"| {r['engine']} | {' / '.join(f'{t:.6f}' for t in r['measured_seconds'])} | {r['median_seconds']:.6f} |\n"
    m=by['bulkparse']['metrics'];d=by['docling']['metrics']
    text=f'''# OCR routing and table alignment — 11 November 2026

BulkParse now flags the outlined-text infographic for OCR and improves table extraction with general geometric rules. On this observed 200-PDF corpus, its table score rises from **{previous['teds_mean']:.6f} to {m['teds_mean']:.6f}**, compared with **{d['teds_mean']:.6f} for Docling** in the fresh same-machine rerun. This is a benchmark result for these versions and export profiles, not a claim of universal superiority.

## Fresh same-machine comparison

{table}
Scores are 0–1. NID has 200 observations, TEDS 42 documents with table references, and MHS 107. Overall averages available metrics within each document, then across documents. All 200 inputs remain in each engine's evaluation. AnyDoc's three failures are empty predictions, with their processing time retained. BulkParse returns partial-coverage results for all 200. Returned output does not prove complete extraction.

All three use the same Apple M3 Max, 36 GiB RAM, sequential document processing in one persistent process, OCR off, one excluded full-corpus warm-up and five measured full-corpus runs. Engine order rotates. BulkParse and AnyDoc use one Rayon thread; Docling uses four configured CPU threads. File reads, conversion, default Markdown export, writes and diagnostic bookkeeping are timed. Startup, model initialization, compilation, IPC and evaluation are excluded. There are no concurrent benchmark conversions or builds during this comparison. Warm caches, no CPU pinning or dedicated-host guarantee; this is not a cold-start or GPU benchmark.

{times}
Docling 2.150.0 / docling-parse 7.23.0 runs Heron layout and TableFormer V1 accurate tables with cell matching enabled. Picture classification/description, code/formula enrichment, OCR and remote services are disabled. AnyDoc 0.2.4 uses its locked pdf-inspector 1.14.2 dependency. The original upstream evaluator and corpus revision `340f25d70f5b2dbc4bc1cc6b154769f74b1fc745` are unchanged. All six predictions per engine are deterministic; the saved checker replays all 600 document scores.

See [full comparison artifacts](../same-machine-ocr-table-alignment-2026-11-11/), [raw comparison](../same-machine-ocr-table-alignment-2026-11-11/comparison.json), and [validation](../same-machine-ocr-table-alignment-2026-11-11/validation.json). Source, installed-package, model, input and evaluator hashes identify the exact run. The [previous comparison](../same-machine-pdf-2026-11-11/README.md) is preserved.

## General rules and Docling review

- **Outlined text:** the content walker records bounds of filled curved paths. With sparse native text, repeated similarly sized, horizontally aligned shapes provide evidence for OCR. Overprinting, tiny shapes, off-page shapes, ordinary rectangles, a single short line, and a substantial native text layer do not satisfy the new rule. Path count alone is insufficient. The API exposes `outlined_text_with_sparse_native_text`; Node and WebAssembly types include it.
- **Numeric tables:** repeated values can align on their left edge, centre or right edge. Header and body whitespace jointly place column boundaries. Separate tables can share line widths. Two-row numeric bodies are supported when painted header/body boundaries and repeated populated columns agree. Numeric body rows cannot masquerade as headers.
- **Partially ruled columns:** explicit borders can separate groups while recurring numeric whitespace separates subcolumns. A painted border already inside that whitespace remains a single boundary. Both sides of a new split must repeatedly contain numeric values within the same painted group.
- **Open paragraph tables:** several horizontal rules and long internal dividers can establish a grid without an outside frame. Divider endpoints bound the final row, populated columns validate it, and a nearby aligned header can be included. Text crossing a proposed divider rejects the repair.

The exact installed Docling implementation was inspected in `table_structure_model.py`, `tf_cell_matcher.py`, and `matching_post_processor.py`. Docling predicts structure with TableFormer, matches native PDF tokens to cell geometry, estimates left/centre/right alignment and adjusts matches. The useful design ideas here are alignment across multiple rows and fine-grained geometric text assignment. BulkParse's implementation remains native and model-free; it does not invoke TableFormer. No document names, IDs, hashes, reference text, or benchmark labels occur in the new parser rules. The rules do use ordinary geometric and numeric thresholds, tested on observed examples; that does not by itself prove generalization.

## OCR correction

Document `01030000000141` contains extensive visible text but only seven extractable non-whitespace characters (`and .org`). BulkParse and independent PDFium extraction agree on that count. The page has 10,015 painted paths. It previously routed to native; it now routes to OCR with `ocr_status: not_requested`. This benchmark does not run OCR or synthesize the missing content, so its native Markdown remains unchanged.

The other two AnyDoc rejections, `01030000000018` and `01030000000113`, contain usable native text and remain native in BulkParse. Inspection decisions are heuristic. This repair does not establish 99% real-world OCR-routing accuracy, nor does a successful OCR flag establish OCR transcription quality. Text converted to one large path, difficult clipping/transparency, diagrams that resemble glyph lines, and mixed pages with substantial native text remain limitations.

## Regression results

| Check | Before | After | Result |
| --- | ---: | ---: | --- |
| Original 200, table TEDS | {previous['teds_mean']:.6f} | {m['teds_mean']:.6f} | Four improved documents; no per-document score regressions |
| Original 200, table structure | {previous['teds_s_mean']:.6f} | {m['teds_s_mean']:.6f} | Same four improvements |
| ICDAR, end-to-end TEDS | {icdar['before']['end_to_end']['teds']:.6f} | {icdar['current']['end_to_end']['teds']:.6f} | Two improved annotated pages; 126 unchanged |
| ICDAR, structure | {icdar['before']['end_to_end']['teds_s']:.6f} | {icdar['current']['end_to_end']['teds_s']:.6f} | No page regressions |
| DocLayNet, 57 pages | F1 {q['heading_fuzzy']['f1']:.6f} | F1 {q['heading_fuzzy']['f1']:.6f} | All JSON outputs identical |
| Native preservation, 304 PDFs | Baseline | Preserved | One intentional OCR-route change; one retained encrypted-PDF failure |
| Authored table checks | 9 | 9 pass | Unchanged expected structures |

ICDAR covers 67 PDFs, 128 annotated pages and 158 gold table regions. Its custom end-to-end aggregation penalizes missing/extra regions and is not comparable to the original 42-document TEDS mean. DocLayNet has no table-structure gold in this study. Its heading precision is {q['heading_fuzzy']['precision']:.6f}, recall {q['heading_fuzzy']['recall']:.6f}; token recall and order checks are unchanged. All three corpora have now informed development and must be treated as observed regression sets.

The original improved documents are `01030000000064`, `01030000000182`, `01030000000189`, and `01030000000190`. ICDAR improvements are `us-001`, pages 1 and 3. Some of these tables remain imperfect. No heading classifier changed; heading scores improve where table text stops being misclassified as headings. 138 core tests and strict Clippy pass, plus six comparison-accounting tests. The strict preservation check intentionally reports a failure for the changed OCR route; its untouched output is preserved in `preservation-strict.json`. The separate [study validation](validation.json) verifies and explicitly accepts that one change, while requiring unchanged native characters and all other inspections.

## Historical July baseline

The supplied July 2026 pdf-inspector 0.2.6 baseline is overall 0.875, NID 0.915, TEDS 0.814, MHS 0.788, with 0.470 seconds on an Apple M4 Pro. Current BulkParse scores are {m['overall_mean']:.3f}, {m['nid_mean']:.3f}, {m['teds_mean']:.3f}, {m['mhs_mean']:.3f}. The original benchmark runner replays the preserved historical predictions exactly. Cross-hardware timing is not a controlled speed comparison; that old pdf-inspector version is also distinct from AnyDoc's 1.14.2 dependency.

## Reproduction and evidence

[Download the study bundle](../ocr-table-alignment-2026-11-11.zip). It contains the source snapshots, patch, predictions, unchanged evaluator, options, timings, checks and source/input hashes. Downloaded corpus PDFs, model weights, installed environments and binaries are excluded. Restore those from the recorded manifests; use the included authored fixtures for core tests.

The original run command is in `../opendataloader-ocr-table-alignment-2026-11-11/environment.json`; paired regression commands are in each study's `identity.json`. Build the CLI from the same source snapshot, then run the same-machine harness with `--bulk-report reports/opendataloader-ocr-table-alignment-2026-11-11` and `--bulk-worker target/opendataloader-build/opendataloader-ocr-table-alignment-2026-11-11/release/bulkparse-opendataloader-bench`, using a fresh output directory. The harness verifies source and worker hashes. Use `scripts/check_same_machine_results.py` on that directory. The original [reproduction protocol](../../benchmarks/same-machine/README.md) records model provisioning and exact profiles.

Before/after native and OCR-routing evidence is retained here. Detailed quality results are in [original 200](../opendataloader-ocr-table-alignment-2026-11-11/), [DocLayNet](../independent-ocr-table-alignment-2026-11-11/), and [ICDAR](../icdar-ocr-table-alignment-2026-11-11/). Use the source snapshots to reproduce historical runs; do not overwrite old evidence or bypass hash checks.
'''
    (REPORT/'README.md').write_text(text)
    (COMPARE/'README.md').write_text('# Fresh same-machine comparison\n\n'+table+'\n'+times+'\nSee the [full study](../ocr-table-alignment-2026-11-11/README.md) for exact profiles, methodology, failure accounting, limitations, validation and reproduction.\n')
    p=ROOT/'README.md';s=p.read_text();start=s.index('| Engine | Overall |',s.index('## PDF inspector benchmarks'));end=s.index('\nBulkParse and AnyDoc',start);s=s[:start]+table+s[end:]
    s=s.replace('BulkParse has the highest overall score in this run; **Docling has the highest table score**.','BulkParse has the highest overall, reading-order, table and heading scores in this run.')
    s=s.replace('End-to-end TEDS **0.688**; structure-only TEDS **0.715**','End-to-end TEDS **0.690**; structure-only TEDS **0.716**')
    s=s.replace('No native-character, inspection, or status regressions; 9 authored table checks pass','Native characters preserved; one corrected OCR route; 9 authored table checks pass')
    s=s.replace('**OCR routing is separate:** the earlier dedicated test matched all 20 controlled OCR-candidate cases.','**OCR routing is separate:** a missed outlined-text infographic now routes to OCR using repeated text-like vector shapes. OCR remains off in this benchmark. The earlier dedicated test matched all 20 controlled OCR-candidate cases.')
    s=s.replace('reports/same-machine-pdf-2026-11-11/README.md','reports/ocr-table-alignment-2026-11-11/README.md').replace('reports/same-machine-pdf-2026-11-11.zip','reports/ocr-table-alignment-2026-11-11.zip');p.write_text(s)
    p=ROOT/'benchmarks/README.md';s=p.read_text().replace('../reports/same-machine-pdf-2026-11-11/README.md','../reports/ocr-table-alignment-2026-11-11/README.md');p.write_text(s)
if __name__=='__main__':main()
