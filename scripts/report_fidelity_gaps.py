#!/usr/bin/env python3
"""Report one-pass conversion results and separately scoped quality evidence."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import statistics

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT/'reports/fidelity-gaps-2026-11-13'
RUN = OUT/'comparison'
ENGINES = ('bulkparse', 'anydoc', 'docling')


def read(p):
    return json.loads(p.read_text())


def main():
    summary = read(RUN/'summary.json')
    identity = read(RUN/'identity.json')
    verification = read(RUN/'verification.json')
    assert verification['complete'] and identity['measured_passes']==1 and identity['warmup']==0
    rows = {e:read(RUN/f'{e}-round-1.json')['documents'] for e in ENGINES}
    assert all(len(r)==112 for r in rows.values())
    assert not any(d['code']=='resource_limit_partial' for r in rows['bulkparse'] for d in r.get('diagnostics',[]))
    for e, records in rows.items():
        for r in records:
            p=RUN/'prediction'/e/'round-1'/(r['id']+'.md')
            assert hashlib.sha256(p.read_bytes()).hexdigest()==r['output_sha256']
    before = {r['id']:r for r in read(ROOT/'reports/automatic-large-documents-2026-11-13/allocation-comparison/after_structured.json')['documents']}
    current = {r['id']:r for r in rows['bulkparse']}
    inventory = []
    for doc in read(RUN/'manifest.json'):
        old,new=before[doc['id']],current[doc['id']]
        inventory.append(dict(id=doc['id'],format=doc['format'],before_status=old['status'],after_status=new.get('status'),
            before_warnings=sorted({d['code'] for d in old.get('diagnostics',[])}),
            after_warnings=sorted({d['code'] for d in new.get('diagnostics',[])}),
            markdown_changed=old['output_sha256']!=new['output_sha256'],returned=new['ok']))
    (OUT/'document-inventory.json').write_text(json.dumps(inventory,indent=2)+'\n')
    changed=[d for d in inventory if d['markdown_changed']]
    statuses=Counter(d['after_status'] for d in inventory)
    counts=Counter(code for d in inventory for code in d['after_warnings'])
    common=set.intersection(*[{d['id'] for d in records if d['ok'] and d['output_bytes']} for records in rows.values()])
    profiles={}
    table='| Converter | Returned / 112 | Whole corpus seconds | CPU seconds | Same-subset median ms | Worker RSS MiB |\n| --- | ---: | ---: | ---: | ---: | ---: |\n'
    for e in ENGINES:
        s=summary[e]
        median=statistics.median(r['elapsed_ns']/1e6 for r in rows[e] if r['id'] in common)
        profiles[e]=dict(returned=s['returned_all'],seconds=s['corpus_seconds'][0],cpu_seconds=s['corpus_cpu_seconds'][0],common_median_ms=median,rss_bytes=s['resources']['lifetime_peak_rss_bytes'])
        p=profiles[e]
        cpu=f'{p["cpu_seconds"]:.3f}' if p['cpu_seconds'] is not None else 'unavailable'
        incomplete=any(r.get('status')=='timeout_or_worker_failure' for r in rows[e])
        rss=('≥ ' if incomplete else '')+f'{p["rss_bytes"]/1048576:.1f}' if p['rss_bytes'] is not None else 'unavailable'
        p['rss_incomplete_due_to_worker_failure']=incomplete
        table+=f'| {e} | {p["returned"]} | {p["seconds"]:.3f} | {cpu} | {median:.3f} | {rss} |\n'
    audit=read(OUT/'source-audit.json')['summary']
    quality='| Targeted source check | Before BulkParse | Current BulkParse | AnyDoc | Docling |\n| --- | ---: | ---: | ---: | ---: |\n'
    for kind,label in [('textbox','Text-box passages'),('auxiliary','Referenced header/footer passages'),('vertical_spans','Explicit vertical merge spans')]:
        a=audit[kind];total=a.get('checks',a.get('expected'));matched=a.get('found',a.get('matched'))
        quality+=f'| {label} | '+' | '.join(f'{matched[e]}/{total}' for e in ['before',*ENGINES])+' |\n'
    result=dict(profiles=profiles,common_nonempty_ids=sorted(common),statuses=dict(statuses),changed_documents=[d['id'] for d in changed],warnings_by_document=dict(counts),quality_score=None)
    (OUT/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    (OUT/'README.md').write_text(f'''# Fidelity-gap investigation and three-converter rerun

All 112 documents were rerun through BulkParse, AnyDoc and Docling on this machine.
BulkParse returns {profiles['bulkparse']['returned']}/112 by default, with {statuses.get('complete',0)} complete and
{statuses.get('partial',0)} partial-fidelity statuses. There are no resource-limit prefixes.
The previous 100 warnings were not 100 failed conversions. Many describe visual
formatting, cached formulas or inferred encodings. This change recovers concrete
missing content and table structure; it does not turn those other limitations
into claimed successes.

## Changes

- DOCX text boxes retain their paragraphs and nested tables. Compatibility
  branches are selected once, so modern shapes and VML fallbacks do not duplicate
  the same content. A shape's description is not repeated when its text box is
  extracted. Current inserted/moved text is retained; deleted/moved-away revisions
  are excluded.
- DOCX horizontal and vertical merges follow explicit grid positions and matching
  restart/continuation spans. Nonempty covered content survives in the anchor.
  Orphan or mismatched merges keep their text and an explicit diagnostic. Table
  grid gaps, false header flags and total cell budgets are respected.
- Referenced DOCX headers, footers, footnotes, endnotes and comments use their own
  package relationships and provenance. Notes receive anchors; missing notes are
  diagnosed. Unreferenced stories and note separators are not appended.
- RTF header/footer definitions are parsed in isolated formatting/table contexts
  and appended separately. They no longer disappear or leak into body paragraphs.
  Even an unclosed auxiliary group retains the saved body text.

These are BulkParse-owned rules based on document structure, with synthetic
regression tests. No document filenames, corpus hashes or text markers select
parser behavior. AnyDoc's existing text-box/merge handling informed the feature
review; its implementation was not imported. No new runtime dependency is added.
This work does not modify spreadsheet parsing or its streaming storage.

## Same-machine one-pass comparison

{table}
The matching-subset median uses the same **{len(common)} nonempty documents** for
all three tools. Whole-corpus figures include failed attempts, which may return
quickly and are not equivalent successful work. All 112 remain in the denominator.
One sequential pass per converter, no warm-up, no repeated timing, Apple M3 Max
with 36 GiB RAM. Thread settings and effective options are preserved. Worker
startup/imports are excluded from conversion time; RSS is worker-lifetime peak,
not an aggregate subprocess-tree peak. A ≥ mark means a killed worker did not
emit its memory high-water measurement, so the reported peak is only a lower bound.
CPU totals are unavailable when a timed-out attempt lacks CPU accounting.
Treat these as development diagnostics,
not stable marketing speed claims or a quality leaderboard.

[Raw measurements](comparison/summary.json) · [Configuration](comparison/identity.json) ·
[Docling versions/options](comparison/effective-options.json) · [Verification](comparison/verification.json)

## Source-checked quality changes

The following checks inspect all 12 DOCX files for the targeted structures. They
were selected after identifying gaps. They are not random sampling, blind judging,
an overall 0–100 score, or evidence that one tool is better on every format.

{quality}
Passage checks ignore markup, punctuation and whitespace and require at least 12
alphanumeric characters. Presence does not prove placement or uniqueness. Merge
checks count matching explicit HTML rowspan/colspan pairs, not all cell-label
relationships. A converter that flattens merges into pipe tables can retain the
text while failing this explicit-structure check. Every checked passage and
per-document result is recorded in [source-audit.json](source-audit.json).

For example, the appeal form `docx-0ccd420844268ba1` previously omitted its grounds
of appeal, certification and supporting-document instructions stored in text
boxes. Those are now retained. `docx-74df23a8ae8850b9` retains the steps and guidance
inside its flowchart shapes. Five DOCX forms contain the vertical-merge cases.

{len(changed)} Markdown outputs changed. The [112-document inventory](document-inventory.json)
records every before/after warning, status and content-change flag, including all
100 previously partial documents. The [warning triage](triage.md) records each
warning category and the remaining work. [Targeted visual inspection](manual-review.json)
checks selected source pages; all 112 were not visually reviewed. Unchanged hashes are useful regression evidence;
they are not proof that the original extraction was perfect. All original paragraph
text in the 24 Word/RTF documents remains present; one RTF first-page header moves
from the body into the separate auxiliary section. See [body-preservation.json](body-preservation.json)
and the reviewed movement in [validation.json](validation.json).

## Remaining gaps and interpretation

- Images, charts without useful text/caches, floating geometry, exact pagination
  and complex visual styles remain incomplete. Text-box placement follows the
  anchor paragraph rather than reconstructing the rendered page.
- Auxiliary stories are grouped after the body. DOCX uses each referenced part
  once; RTF retains each definition once, so repeated section variants can repeat
  header text. This preserves content but can add clutter, and is diagnosed.
- Legacy DOC/XLS formatting, full RTF list numbering, custom DOCX numbering,
  advanced EPUB CSS and some embedded-object content still need work.
- Formula caches are never recalculated. Hidden-sheet policy and ambiguous CSV
  encoding remain explicitly reported. Suppressing those warnings would not
  improve extraction quality.

No paid judging jobs were submitted. The earlier broad blind-quality review
remains unfinished; this report provides targeted source checks and assistant
inspection, not an independent quality score. The corpus has informed development.

## Reproduce

```sh
cargo test --locked -p bulkparse -p bulkparse-cli -p bulkparse-pool
cargo build --release --locked -p bulkparse --example office_bench
python3 scripts/benchmark_realworld_comparison.py --out /tmp/bulkparse-fidelity-run
python3 scripts/audit_fidelity_gaps.py --run /tmp/bulkparse-fidelity-run \\
  --out /tmp/bulkparse-fidelity-source-audit.json
```

The run used the preserved `after-office-bench` executable to avoid shared-build
output changes. The corpus manifest, hashes, source snapshot, raw predictions and
failed attempts are retained under `comparison/`. Concurrent work changed two OCR
source files during the run; the executable hashes and preserved source snapshot
remained unchanged, as disclosed in the verification record. OCR is not used by
this Office/EPUB/CSV corpus. The source audit also reads the
preserved previous BulkParse predictions. See [validation.json](validation.json)
for regression-suite and binding checks.
''')
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    main()
