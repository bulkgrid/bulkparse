#!/usr/bin/env python3
"""Reproducible report for source-backed Office content recovery."""
import json,hashlib
from pathlib import Path
from report_diagnostic_categories import summarize
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'reports/content-recovery-2026-11-13'
def read(p):return json.loads(p.read_text())
def main():
 rows=read(OUT/'release-benchmark/bulkparse-round-1.json')['documents'];old={r['id']:r for r in read(ROOT/'reports/diagnostic-categories-2026-11-13/benchmark/bulkparse-round-1.json')['documents']}
 assert all(r['ok'] and r['status']==old[r['id']]['status'] for r in rows)
 quality=read(OUT/'source-quality-checks.json');changed=[r['id'] for r in rows if r['output_sha256']!=old[r['id']]['output_sha256']]
 assert set(changed)=={r['id'] for r in quality['changed_documents']}
 binary=hashlib.sha256((OUT/'release-office-bench').read_bytes()).hexdigest()
 pair=read(OUT/'release-paired/summary.json');assert pair['binary_sha256']['after']==binary==quality['binary_sha256']
 assert read(OUT/'release-benchmark/verification.json')['complete']
 counts=summarize(rows);(OUT/'categories.json').write_text(json.dumps(counts,indent=2)+'\n')
 profiles={}
 for label in ['before','after','benchmark','reverse-control','release-benchmark']:
  s=read(OUT/label/'summary.json')['bulkparse'];profiles[label]=dict(seconds=s['corpus_seconds'][0],cpu_seconds=s['corpus_cpu_seconds'][0],rss_gib=s['resources']['lifetime_peak_rss_bytes']/2**30)
 p=profiles['release-benchmark'];b,a=pair['profiles']['before'],pair['profiles']['after'];c=counts['exclusive_counts'];
 source=quality['source_checks'];checked=sum(s['checked'] for d in source for s in d.get('stories',[]));novel=sum(s['new_paragraphs'] for d in source for s in d.get('stories',[]))
 summary=dict(returned=112,changed_documents=changed,unchanged_markdown=112-len(changed),word_source_paragraphs_checked=checked,new_word_source_paragraphs=novel,categories=c,sequential_profiles=profiles,matched_profiles=pair['profiles'],interpretation='General extraction improvements; no new visual quality score. Matched elapsed time is essentially unchanged; latest matched RSS is higher. Other controls vary, so the strict no-memory-increase condition is not established.')
 (OUT/'results.json').write_text(json.dumps(summary,indent=2)+'\n')
 (OUT/'README.md').write_text(f'''# Source-backed content recovery — November 13, 2026

BulkParse returns **112/112** documents. **{len(changed)} outputs gain source-backed
content; {112-len(changed)} Markdown outputs remain byte-identical**. Main-body lines
were preserved in changed DOC/XLS files; the two small DOCX title diffs were reviewed
against source attributes. This does not establish an overall quality score.

## What changed

- **Six legacy Word files:** recover footnotes, headers/footers and text-box stories
  from the existing binary piece table. Decode intersecting story ranges only;
  no image assets or second full document model are loaded. Field instructions are
  excluded while cached results survive. Auxiliary text is appended by story with
  provenance. Individual note links, layout and rich styles remain explicitly partial.
- **Three XLS files:** recover two chart/source text boxes and three cell comments.
  BIFF continuation decoding handles encoding changes and surrogate pairs, stops
  before formatting records, and respects existing text/depth/cancellation limits.
  Comments use their object IDs and NOTE records for cell provenance. Chart cell
  data still never enters the worksheet grid. Malformed optional text keeps a warning.
- **Two DOCX files:** retain distinct image titles as well as descriptions. Text-box
  descriptions that repeat visible text are suppressed using general punctuation/
  case-insensitive comparison. The initial variant duplicated four visible captions;
  it was rejected. Distinct metadata remains covered by an authored test.

These rules depend on format structures, never document names or benchmark IDs.
No OCR/model, asset-byte retention, larger limits or truncation was introduced.
Worksheet/shared-string streaming remains intact. The schema remains 0.7.

## Source checks and remaining gaps

The independent compound-file audit verified **{checked} auxiliary Word paragraphs**;
**{novel}** were not found in the prior Markdown. All five XLS text objects are
present. See [source evidence and exact output diffs](source-quality-checks.json).
Counts are based on normalized text, not visual fidelity or correct note placement.

All **66 previously flagged documents were triaged** using diagnostics, package
inventories and targeted binary/XML inspection; this was not a full page-by-page
visual assessment. [Per-document investigation](investigation.json)

| Current warning outcome (exclusive) | Documents |
| --- | ---: |
| Detected omitted content, including image/object payloads | {c.get('detected_missing_content',0)} |
| Unverified content risk, without a detected omission | {c.get('unverified_content_risk_only',0)} |
| Formatting limitations, without a content warning | {c.get('formatting_without_content_warning',0)} |
| Informational only | {c.get('informational_only',0)} |
| No diagnostics | {c.get('no_diagnostics',0)} |

The previous breakdown was 40 detected / 26 uncertain. Reading an auxiliary Word
header exposed an existing embedded object that was previously hidden behind a
broad warning. It is now detected; this does not mean extraction lost new content.
The recovered text does not eliminate image, object or layout limitations, so warning
counts are not a quality-improvement score. Legacy statuses remain 13 complete / 99
partial. [All current diagnostic evidence](categories.json)

Remaining work includes image/embedded-asset export, unsupported OLE content,
auxiliary-story layout and note links, and visual chart/presentation structure.
AnyDoc 0.2.4 retains supported asset bytes in its model but emits embedded images'
alt text in Markdown; therefore image warnings alone do not establish worse Markdown.
BulkParse does not yet offer equivalent asset-byte retention. Adding it indiscriminately
would conflict with the memory constraint; bounded/streamed asset export is a separate
API/design task. No external asset URLs were fetched or embedded macros executed.

## Speed and memory

The final sequential pass took **{p['seconds']:.3f} s**, **{p['cpu_seconds']:.3f} CPU s**,
with **{p['rss_gib']:.3f} GiB** peak worker RSS. Same 112 inputs/default settings,
Apple M3 Max / 36 GiB / macOS, OCR off, one pass without warm-up. Imports/setup and
protocol serialization are excluded from conversion time; RSS includes the worker
lifetime/transport, not aggregate process-tree usage. No builds/parsing jobs overlapped
performance measurements. AnyDoc/Docling were not rerun; this is a BulkParse change
and regression check, with the [previous comparison](../provenance-memory-2026-11-13/README.md)
kept separate.

| Matched interleaved check | Before | Current recovery build |
| --- | ---: | ---: |
| Corpus elapsed seconds | {b['seconds']:.3f} | {a['seconds']:.3f} |
| Corpus CPU seconds | {b['cpu_seconds']:.3f} | {a['cpu_seconds']:.3f} |
| Peak worker GiB | {b['peak_rss_bytes']/2**30:.3f} | {a['peak_rss_bytes']/2**30:.3f} |

Elapsed speed was essentially unchanged and CPU time was slightly lower. However,
**matched RSS rose by {(a['peak_rss_bytes']-b['peak_rss_bytes'])/2**20:.1f} MiB
({100*(a['peak_rss_bytes']/b['peak_rss_bytes']-1):.1f}%)**. Earlier recovery variants
showed lower matched RSS, so the memory evidence is inconsistent. Standalone results varied: the **unchanged baseline**
measured {profiles['before']['seconds']:.3f} s / {profiles['before']['rss_gib']:.3f} GiB before development and
{profiles['reverse-control']['seconds']:.3f} s / {profiles['reverse-control']['rss_gib']:.3f} GiB when repeated immediately
after the final run to investigate conflicting RSS readings. The accepted version
measured {p['seconds']:.3f} s / {p['rss_gib']:.3f} GiB. We do not claim statistically proven
speed/memory improvement or a fixed ceiling. **The strict no-memory-increase condition
is not established**: the latest matched result is higher, and the standalone final
peak exceeds the earlier task's 1.35 GiB reading. The content fixes are implemented;
flat memory use must not be claimed. All measurements, including the higher-RSS initial variant and
rejected caption duplication, remain preserved.

[Final sequential benchmark](release-benchmark/summary.json) · [Matched control](release-paired/summary.json) ·
[Reverse baseline control](reverse-control/summary.json) · [Source/input/binary verification](release-benchmark/verification.json)

## Validation and reproducibility

[Validation](validation.json) includes Rust/minimal builds, Node/Python/WASM checks,
cross-binding parity and 200 unchanged PDF Markdown predictions. The six-mode
historical suite checked 162 outcomes. Only source-verified caption additions,
clarified XLS warning wording and category metadata changed expectations; all old
outcomes and original body lines were retained. The previous expectations and
[explicit review](regressions/review.json) are preserved.

```sh
cargo build --offline --release -p bulkparse --example office_bench
python3 scripts/benchmark_realworld_comparison.py --engines bulkparse --out /tmp/content-recovery
python3 scripts/check_realworld_failures.py --out /tmp/content-regressions
```

The final immutable executable is `release-office-bench`; the final benchmark has
source snapshots and hashes. `before-office-bench` comes from the prior diagnostic
categories report. Earlier binaries (`office-bench`, `final-office-bench`) are
superseded experiments, not the accepted implementation.
''')
 print(json.dumps(summary,indent=2))
if __name__=='__main__':main()
