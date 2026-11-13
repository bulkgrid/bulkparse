#!/usr/bin/env python3
"""Report a quality-preserving parser optimization, separating rejected inputs."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT/'reports/parser-speed-2026-11-13'


def read(path):
    return json.loads(path.read_text())


def main():
    paired=read(OUT/'final-paired/summary.json')
    assert not any(paired['differences'].values())
    p=paired['profiles']
    assert all(r['returned']==112 and r['truncated']==0 for r in p.values())
    assert p['after_structured']['json_validated']==112
    run=OUT/'comparison'
    verification=read(run/'verification.json')
    assert verification['complete']
    engines=('bulkparse','anydoc','docling')
    rows={e:{r['id']:r for r in read(run/f'{e}-round-1.json')['documents']} for e in engines}
    summary=read(run/'summary.json')
    final=OUT/'final'
    assert read(final/'verification.json')['complete']
    rows['bulkparse']={r['id']:r for r in read(final/'bulkparse-round-1.json')['documents']}
    summary['bulkparse']=read(final/'summary.json')['bulkparse']
    before_plain=read(OUT/'plain-baseline/bulkparse-round-1.json')['documents']
    before_plain_summary=read(OUT/'plain-baseline/summary.json')['bulkparse']
    assert all(r['output_sha256']==rows['bulkparse'][r['id']]['output_sha256'] and r['diagnostics']==rows['bulkparse'][r['id']]['diagnostics'] for r in before_plain)
    original=ROOT/'reports/fidelity-gaps-2026-11-13/comparison'
    old={e:{r['id']:r for r in read(original/f'{e}-round-1.json')['documents']} for e in ('bulkparse','anydoc')}
    failed_old=[i for i,r in old['anydoc'].items() if not r['ok']]
    gap=sum(r['elapsed_ns'] for r in old['bulkparse'].values())-sum(r['elapsed_ns'] for r in old['anydoc'].values())
    reject_gap=sum(old['bulkparse'][i]['elapsed_ns']-old['anydoc'][i]['elapsed_ns'] for i in failed_old)
    same=[i for i,r in rows['anydoc'].items() if r['ok'] and rows['bulkparse'][i]['ok']]
    rejected=[i for i,r in rows['anydoc'].items() if not r['ok']]
    comparison={}
    table='| Converter | Returned / 112 | Whole-corpus seconds | CPU seconds | Worker peak RSS MiB |\n| --- | ---: | ---: | ---: | ---: |\n'
    for e in engines:
        s=summary[e]
        comparison[e]=dict(returned=s['returned_all'],seconds=s['corpus_seconds'][0],cpu_seconds=s['corpus_cpu_seconds'][0],rss_bytes=s['resources']['lifetime_peak_rss_bytes'])
        v=comparison[e]
        cpu=f'{v["cpu_seconds"]:.3f}' if v['cpu_seconds'] is not None else 'unavailable'
        rss=f'{v["rss_bytes"]/1048576:.1f}' if v['rss_bytes'] is not None else 'unavailable'
        table+=f'| {e} | {v["returned"]} | {v["seconds"]:.3f} | {cpu} | {rss} |\n'
    subsets={}
    subset_table='| Input subset | Documents | BulkParse seconds | AnyDoc seconds |\n| --- | ---: | ---: | ---: |\n'
    for label,ids in [('Both return output',same),('AnyDoc rejects',rejected)]:
        values={e:sum(rows[e][i]['elapsed_ns'] for i in ids)/1e9 for e in ('bulkparse','anydoc')}
        subsets[label]=dict(ids=ids,seconds=values)
        subset_table+=f'| {label} | {len(ids)} | {values["bulkparse"]:.3f} | {values["anydoc"]:.3f} |\n'
    improvements={}
    paired_table='| Mode | Seconds before → after | CPU seconds before → after | Worker peak RSS MiB before → after |\n| --- | ---: | ---: | ---: |\n'
    for mode,label in [('markdown','Streamed Markdown'),('structured','Full structured model + Markdown')]:
        b,a=p['before_'+mode],p['after_'+mode]
        improvements[mode]={key:100*(1-a[key]/b[key]) for key in ('seconds','cpu_seconds','worker_peak_rss_bytes')}
        paired_table+=f'| {label} | {b["seconds"]:.3f} → {a["seconds"]:.3f} | {b["cpu_seconds"]:.3f} → {a["cpu_seconds"]:.3f} | {b["worker_peak_rss_bytes"]/1048576:.1f} → {a["worker_peak_rss_bytes"]/1048576:.1f} |\n'
    plain_before=dict(seconds=before_plain_summary['corpus_seconds'][0],cpu_seconds=before_plain_summary['corpus_cpu_seconds'][0],rss_bytes=before_plain_summary['resources']['lifetime_peak_rss_bytes'])
    allocations=read(OUT/'allocation-results.json')['profiles']
    allocation_saving=100*(1-allocations['after']['total_allocated_bytes']/allocations['before']['total_allocated_bytes'])
    result=dict(original_total_gap_seconds=gap/1e9,original_gap_explained_by_rejections_percent=100*reject_gap/gap,
        profiles=comparison,subsets=subsets,paired_improvement_percent=improvements,
        plain_before=plain_before,allocations=allocations,full_json_equal=112,markdown_equal=112)
    (OUT/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    (OUT/'README.md').write_text(f'''# Parser speed investigation — November 13, 2026

BulkParse still completes **112/112** documents by default. The optimization
preserves **every Markdown byte, complete JSON hash, status and diagnostic** across
the corpus. The recent text-box, merged-table and auxiliary-story quality fixes
remain intact. There are still 13 complete and 99 partial-fidelity statuses.

## Why AnyDoc's total was lower

In the preceding run, {100*reject_gap/gap:.1f}% of the {gap/1e9:.3f}-second total-time
gap came from three large spreadsheets that AnyDoc rejects but BulkParse completes.
The other 109 documents took BulkParse 7.074 s and AnyDoc 6.192 s. A lower
whole-corpus time therefore did not represent the same amount of completed work.
The rejected inputs remain included, and BulkParse still returns their full output.

ODS documents accounted for 6.183 s of BulkParse's previous 13.083 s total. Targeted
phase measurements and CPU stack samples show parsing, rather than Markdown
rendering, dominates the largest spreadsheets. The main costs include XML
validation/partitioning, semantic extraction, ordered cell indexes and allocation.
The [ODS](ods-b3185d3043cb268a-sample-before.txt) and
[XLSM](xlsm-1c382d45c18005cd-sample-before.txt) samples are diagnostic runs built
with symbols. Repeated sampling inputs are not presented as benchmark passes.

## Changes

- Private temporary ODF row records use short field names and omit absent values.
  This reduces serialization, decoding and temporary-file traffic. Public JSON
  and source XML validation are unchanged.
- Cached worksheet rows and shared-string records are borrowed directly during
  decoding instead of copied into newly allocated byte buffers. Records exceeding
  the existing 64 KiB window remain owned and uncached. Lifetimes prevent the
  read window from changing while a caller is using it.

No larger caches, parallel workers, reduced extraction, disabled validation,
increased limits, input-specific rules or new runtime dependencies were introduced.
The streaming worksheet/shared-string paths remain in use.

## Paired speed and memory check

{paired_table}
Observed structured CPU time fell {improvements['structured']['cpu_seconds']:.1f}%
and peak RSS fell {improvements['structured']['worker_peak_rss_bytes']:.1f}%.
Streamed Markdown CPU fell {improvements['markdown']['cpu_seconds']:.1f}% and peak
RSS fell {improvements['markdown']['worker_peak_rss_bytes']:.1f}%.
Each version ran one complete sequential pass per mode, with no warm-up, on the
same M3 Max. The baseline was preserved before editing; it was not rerun during
optimization. These are one-pass observations, not statistically established
speed guarantees or hard memory limits.

Full JSON serialization, validation to EOF and hashing are excluded from the
parse/Markdown timing, but included in the process's memory lifetime. Those extra
checks took {p['before_structured']['json_validation_seconds']:.3f} s before and
{p['after_structured']['json_validation_seconds']:.3f} s after. Do not compare this
RSS directly with a worker that does not perform JSON verification.

[Raw paired results](final-paired/summary.json) ·
[Source and binary identities](final-paired/identity.json) ·
[Complete baseline](baseline/summary.json)

## Fresh AnyDoc/Docling comparison

{table}
{subset_table}
One sequential measured pass for each listed converter/version, no warm-up; all 112 public inputs
remain in the set. Worker imports/initialization are excluded from conversion
time. Thread settings, actual failures and versions are retained. Worker-lifetime
RSS includes initialization and output transport; it is not aggregate concurrent
process-tree memory. Tool APIs and retained representations differ: BulkParse
returns the full model plus Markdown, while AnyDoc's adapter calls its Markdown
API. Neither a returned result nor a shared input establishes identical quality.
AnyDoc and Docling were measured once in the initial comparison. The final
BulkParse binary was measured separately after the memory investigation; their
unchanged comparator results are reused, rather than repeating the slow run.
No heavy build or other parsing workload ran alongside measured passes.

The same native worker was also checked without JSON verification using the
preserved old binary: {plain_before['seconds']:.3f} s and
{plain_before['rss_bytes']/1048576:.1f} MiB before, versus
{comparison['bulkparse']['seconds']:.3f} s and
{comparison['bulkparse']['rss_bytes']/1048576:.1f} MiB after. This separate
baseline uses the same flags as the competitor run; its implementation sources
are preserved under `before/source-snapshot`.
**Ordinary-worker RSS increased by
{100*(comparison['bulkparse']['rss_bytes']/plain_before['rss_bytes']-1):.1f}% in this
comparison. A consistent reduction in whole-worker RSS is not established.**

[Final BulkParse measurements](final/summary.json) ·
[Comparator measurements](comparison/summary.json) ·
[BulkParse settings](final/identity.json) · [Comparator settings](comparison/identity.json) ·
[Effective Docling configuration](comparison/effective-options.json) ·
[Preserved-binary plain baseline](plain-baseline/summary.json)

### Memory investigation and discarded variants

The first prototype also combined two ODF indexes and changed ordered table
traversal. Its verified-JSON and writer peaks fell, but ordinary-worker memory did
not consistently improve. Those two changes were removed. The unselected runs
are retained under `first-comparison`, `comparison`, `plain-after`, `odf-only`
and `compact`; their timings must not be presented as the final implementation.

RSS also varies with allocator retention: post-response traces of the unchanged
baseline and compact-record variant reached
{max(r['rss_bytes'] for r in read(OUT/'retention-before.json')['rows'])/1048576:.1f}
and {max(r['rss_bytes'] for r in read(OUT/'retention-compact.json')['rows'])/1048576:.1f}
MiB, respectively, reversing their earlier ordinary-worker ordering. Those traces
include process sampling and are not speed measurements. All observations are
retained; these results do not establish a universal memory ceiling.

A separate instrumented pass isolates Rust-requested allocations from allocator
retention. Peak live bytes were identical:
**{allocations['before']['peak_live_bytes']:,} bytes before and after**.
Total allocated bytes fell **{allocation_saving:.1f}%**, from
{allocations['before']['total_allocated_bytes']:,} to
{allocations['after']['total_allocated_bytes']:,}; allocation calls fell from
{allocations['before']['allocation_calls']:,} to {allocations['after']['allocation_calls']:,}.
These counters exclude transient realloc overlap and non-Rust allocations. They
are not process RSS, and instrumented runs are not speed measurements.
[Allocation evidence](allocation-results.json) retains both binary hashes;
the baseline was rebuilt from frozen sources in a separate build directory.

## Quality and regression validation

The exact-output comparison verifies all 112 full JSON SHA-256 hashes and Markdown
outputs, including provenance, block IDs and diagnostics. Both output modes return
all 112 without recovery or resource-limit prefixes. No reference expectations
were relaxed for this optimization.

The [previous targeted source checks](../fidelity-gaps-2026-11-13/source-audit.json)
remain applicable because outputs match exactly. They check specific omissions
and explicit table structure; they are not overall quality scores. Images, exact
visual layout and other fidelity warnings remain. The corpus has informed
development and is not an unseen holdout.

See [validation.json](validation.json) for the historical-failure suite, PDF
regression checks, unit tests and binding parity checks. No paid judging jobs or
new aggregate quality scores are involved.

## Reproduce

```sh
cargo build --release --locked -p bulkparse --example office_bench
python3 scripts/benchmark_parser_speed.py \\
  --before reports/parser-speed-2026-11-13/before-office-bench \\
  --out /tmp/bulkparse-speed-paired
python3 scripts/benchmark_realworld_comparison.py --out /tmp/bulkparse-speed-competitors
```

Use fresh output directories. The preserved executable is macOS-specific; source
snapshots, source/input hashes and raw predictions are retained for rebuilding.
The immutable executable used in the measured competitor run is
`final-office-bench`. Full-corpus timing was performed once per version/profile;
diagnostic stack sampling and output checks are recorded separately.

For allocation diagnostics, build `--example allocation_profile`, then use
`scripts/profile_worker_allocations.py --binary PATH --out NEW_DIRECTORY --passes 1 --structured`.
Use a separate `CARGO_TARGET_DIR` when rebuilding the frozen baseline sources.
''')
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    main()
