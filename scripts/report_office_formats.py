#!/usr/bin/env python3
"""Render the measured multiformat report; never synthesize quality scores."""
import json,statistics
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'reports/multiformat-2026-11-11';RUN=OUT/'benchmark-final'
def main():
    s=json.loads((RUN/'summary.json').read_text());env=json.loads((RUN/'environment.json').read_text());v=json.loads((OUT/'validation/summary.json').read_text())
    lines=['# BulkParse-owned format readers — 11 November 2026','',
    'Implemented eleven new canonical format readers and their aliases in BulkParse: XLSX/XLSM, PPTX/PPTM/PPSX/PPSM, XLS, XLSB, ODT, ODS, ODP, EPUB, RTF, DOC and PPT/PPS/POT. DOCX/DOCM already existed. AnyDoc and Docling are separate benchmark competitors, never BulkParse runtime dependencies. See the [format contracts](../../docs/office-formats.md).','',
    '**Experimental fidelity:** BulkParse returned output for all 81 conversion inputs, but **67 were partial** and 14 had no detected content gaps. All 11 authored text-preservation checks and 17 structural/resource tests pass. This is not an 81/81 full-fidelity score, an LLM judgement, or proof of production readiness.','',
    '## Same-machine measurements','',
    f"Machine: {env['machine']}, {env['platform']}. BulkParse 0.1.0 (schema 0.4), AnyDoc 0.2.4, Docling {env['versions']['docling']}. All files were local. No OCR, PDF, language-model or remote-service pipeline was requested.",'',
    '| Engine | Returned / attempted | Median full-corpus conversion time | Lifetime peak RSS |',
    '| --- | ---: | ---: | ---: |']
    for e in ['bulkparse','anydoc','docling']:
        r=s['engines'][e];rss=s['resources'][e].get('lifetime_peak_rss_bytes');lines.append(f"| {e} | {r['returned']}/{r['attempted']} | {r['median_corpus_ms']:.3f} ms | {rss/2**20:.1f} MiB |")
    lines+=['','**The corpus differs for Docling.** BulkParse and AnyDoc ran the same 81 inputs. Docling ran the 31 XLSX/XLSM/PPTX inputs it supports. It rejected six upstream hand-built PPTX fixtures; failures remain in its timings. Those fixtures include incomplete/stale content-type metadata and strict/alternate package structures, so these failures are not a representative real-world failure rate.','',
    'One full warm-up was excluded, then five complete passes were measured with rotating engine order. Each persistent process handled documents sequentially. Rayon/OMP/OpenBLAS thread settings were one; there were no concurrent benchmark conversions. Per-document clocks include file reads, parsing and default Markdown export. The corpus time sums those clocks; startup, inter-process JSON, output writes and scoring are excluded. Every engine produced identical success/failure outcomes and Markdown across its five measured passes.','',
    'RSS comes from macOS `/usr/bin/time -l` and includes startup, warm-up and all measured passes. For the Rust workers it also includes the separate robustness pass. It is a process-lifetime peak, not incremental per-document memory. Lifetime CPU totals (including startup) are recorded in the raw logs; they are not presented as warm conversion CPU measurements.','',
    'For the **same 25 files returned by all three tools**, summed conversion-time medians are:','']
    common={r['path'] for r in json.loads((RUN/'docling-pass-1.json').read_text())['documents'] if r['ok']}
    for e in ['bulkparse','anydoc','docling']:
        times=[sum(r['elapsed_ns'] for r in json.loads((RUN/f'{e}-pass-{i}.json').read_text())['documents'] if r['path'] in common)/1e6 for i in range(1,6)]
        lines.append(f'- {e}: {statistics.median(times):.3f} ms.')
    lines+=['','This does not equalize output fidelity. BulkParse omits visual content that competitors may process; one EMF-containing workbook accounts for much of Docling’s corpus time. Smaller output scope contributes to speed differences. No universal speed or quality ranking is claimed.','',
    '## Per-format results','',
    'Median document time: first take the median of five timings for each input, then the median across inputs of that format. Numbers include failed attempts. XLSM is shown separately as an extension alias, not counted as an additional parser.','',
    '| Format | Inputs | BulkParse median ms | AnyDoc median ms | Docling median ms (returned) | BulkParse partial |',
    '| --- | ---: | ---: | ---: | ---: | ---: |']
    for f,eng in s['per_format'].items():
        b=eng['bulkparse'];a=eng['anydoc'];d=eng.get('docling');dt=f"{d['median_document_ms']:.3f} ({d['returned']}/{d['attempted']})" if d else '—'
        lines.append(f"| {f.upper()} | {b['attempted']} | {b['median_document_ms']:.3f} | {a['median_document_ms']:.3f} | {dt} | {b['partial']} |")
    lines+=['','AnyDoc’s API does not expose a comparable complete/partial status; a successful return is not labelled complete here.','',
    '## Quality, robustness and regressions','',
    '- Corpus: 70 frozen AnyDoc/Docling reference fixtures plus 11 BulkParse-authored files, covering all eleven new canonical readers. These are development fixtures, many deliberately synthetic, not 81 randomly sampled customer documents.',
    '- Authored semantic checks: BulkParse **11/11**, AnyDoc **11/11**, Docling **2/2** on its applicable formats. These checks assert selected required/forbidden text, not whole-document layout or formatting quality.',
    '- Seventeen Rust Office tests additionally check sheet/slide ordering, inherited title styles, merged geometry, source coordinates, date epochs, cached/missing formulas, Unicode, BIFF continuation encoding, binary records, encryption, cancellation and resource limits.',
    '- Nine separate robustness files: BulkParse rejected eight with errors and recovered the unbalanced RTF as partial. AnyDoc rejected seven and recovered the broken-persistence PPT plus the RTF. BulkParse deliberately refuses to scan obsolete PowerPoint records when active persistence is damaged. No crash occurred in this set.',
    f"- Full Rust workspace, all features: **{v['rust_tests_passed']['final-tests']} tests passed**; minimal build **{v['rust_tests_passed']['minimal']}**, office-only build **{v['rust_tests_passed']['only']}**. Clippy with warnings denied and formatting checks pass.",
    '- Node and Python: four smoke tests each. Standard WASM smoke checks pass. Nineteen cases, including all eleven new formats, match exactly across native Node, Python, WASM and CLI; three PDF inspection cases also match.',
    '- All **200 opendataloader PDF Markdown predictions are byte-for-byte unchanged** against the previous OCR/table-alignment snapshot. Existing NID/TEDS/MHS scores therefore have unchanged inputs; scoring and PDF timing were not rerun for this format-only change. PDF parser source files are unchanged.',
    '- Broader PDF/WASM limitation: default Node 24.21.0 hit `Fatal process out of memory: Zone` in its optimizing compiler. The saved **pre-change** WASM build reproduces this after the same large PDFs. With `--liftoff-only`, all 26 extended conversion cases and three inspection cases match. This is a remaining runtime limitation, not a clean default-runtime pass; browser and Linux validation remain outstanding.','',
    'There is no NID/TEDS/MHS or LLM-judge score for this mixed-format corpus. AnyDoc’s published 100-document benchmark used a different, nonredistributable corpus and judging protocol. These measurements cannot reproduce or substantiate its published quality score.','',
    '## Implementation limits','',
    'The readers use general container, relationship, style and binary-record rules. Production code contains no benchmark filenames, document IDs, fixture text or corpus-specific branches. Tests and fixture generators contain expected content. Package libraries handle ZIP/XML/CFB/encoding; BulkParse handles extraction, bounds, diagnostics, provenance and rendering. Design references and license attribution are in [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md).','',
    'The largest remaining gaps are legacy DOC/PPT formatting, CSS/ODF styling and visibility, nested lists, charts/SmartArt/images, advanced number formats and complex legacy tables. Cached formulas are not recalculated. Details and behavior differences are documented in [the format contracts](../../docs/office-formats.md).','',
    '## Reproduction and evidence','',
    '- [Protocol and commands](../../benchmarks/multiformat/README.md)',
    '- [Final manifest](benchmark-final/manifest.json), [summary](benchmark-final/summary.json), [environment](benchmark-final/environment.json), and [source hashes](benchmark-final/source-manifest.json)',
    '- [Validation](validation/summary.json) and [200-PDF equality check](pdf-regression.json)',
    '- `benchmark-final/*-warmup.json` and `*-pass-1.json` through `*-pass-5.json` contain outcomes, timings, diagnostics and complete predictions. Failed predictions are empty. RSS/CPU evidence is in the corresponding stderr logs.',
    '- [Download source and evidence ZIP](../multiformat-2026-11-11.zip). Third-party input documents, dependencies, models and compiled binaries are excluded; use the frozen corpus manifest to identify and verify local reference files.','',
    'Earlier `initial`, `benchmark` and `benchmark-v2` directories are debugging records, not reported results. Two authored package metadata omissions were fixed before the final run. The earlier system-restricted run could not collect memory; the final run successfully collected it.']
    (OUT/'README.md').write_text('\n'.join(lines)+'\n')
if __name__=='__main__':main()
