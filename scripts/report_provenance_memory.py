#!/usr/bin/env python3
"""Report compact provenance, allocation cleanup and the fresh competitor run."""
import json
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT/'reports/provenance-memory-2026-11-13'


def read(path):
    return json.loads(path.read_text())


def main():
    paired = read(OUT/'accepted-paired/summary.json')
    assert not any(paired['differences'].values())
    assert all(p['returned']==112 and p['truncated']==0 for p in paired['profiles'].values())
    assert paired['profiles']['after_structured']['json_validated']==112
    assert (OUT/'schema-before.json').read_bytes()==(OUT/'schema-accepted.json').read_bytes()
    drift = read(OUT/'accepted-drift-check/summary.json')
    assert not drift['differences']
    assert read(OUT/'comparison/verification.json')['complete']
    engines = ['bulkparse','anydoc','docling']
    summary = read(OUT/'comparison/summary.json')
    rows = {e:{r['id']:r for r in read(OUT/f'comparison/{e}-round-1.json')['documents']} for e in engines}
    baseline_rows = {r['id']:r for r in read(OUT/'plain-before/bulkparse-round-1.json')['documents']}
    assert rows['bulkparse'].keys() == baseline_rows.keys()
    for key, row in rows['bulkparse'].items():
        for field in ['ok', 'status', 'output_sha256', 'output_bytes', 'diagnostics', 'render_diagnostics']:
            assert row.get(field) == baseline_rows[key].get(field), (key, field)
    common = [i for i in rows['bulkparse'] if all(rows[e][i]['ok'] for e in engines)]
    both = [i for i in rows['bulkparse'] if rows['bulkparse'][i]['ok'] and rows['anydoc'][i]['ok']]
    profiles = {}
    table = '| Parser | Returned / 112 | Corpus seconds | CPU seconds | Shared median ms | Peak worker MiB |\n| --- | ---: | ---: | ---: | ---: | ---: |\n'
    for e in engines:
        s = summary[e]
        v = profiles[e] = dict(returned=s['returned_all'],seconds=s['corpus_seconds'][0],cpu_seconds=s['corpus_cpu_seconds'][0],
            rss_bytes=s['resources']['lifetime_peak_rss_bytes'],shared_median_ms=statistics.median(rows[e][i]['elapsed_ns']/1e6 for i in common))
        table += f'| {e} | {v["returned"]} | {v["seconds"]:.3f} | {v["cpu_seconds"]:.3f} | {v["shared_median_ms"]:.3f} | {v["rss_bytes"]/2**20:.1f} |\n'
    s = read(OUT/'plain-before/summary.json')['bulkparse']
    before = dict(seconds=s['corpus_seconds'][0],cpu_seconds=s['corpus_cpu_seconds'][0],rss_bytes=s['resources']['lifetime_peak_rss_bytes'])
    gain = {k:100*(1-profiles['bulkparse'][k]/before[k]) for k in before}
    pair_table = '| Mode | Seconds before → after | CPU seconds before → after | Peak worker MiB before → after |\n| --- | ---: | ---: | ---: |\n'
    for mode,label in [('markdown','Streamed Markdown'),('structured','Structured + full JSON verification')]:
        b,a = (paired['profiles'][k+'_'+mode] for k in ['before','after'])
        pair_table += f'| {label} | {b["seconds"]:.3f} → {a["seconds"]:.3f} | {b["cpu_seconds"]:.3f} → {a["cpu_seconds"]:.3f} | {b["worker_peak_rss_bytes"]/2**20:.1f} → {a["worker_peak_rss_bytes"]/2**20:.1f} |\n'
    allocations = {}
    for label in ['before','after']:
        rs = read(OUT/f'allocations-{label}/summary.json')['rows']
        largest = next(r for r in rs if Path(r['path']).stem=='xlsm-1c382d45c18005cd')
        allocations[label] = dict(peak_live=max(r['transport']['peak'] for r in rs),allocated=sum(r['transport']['allocated'] for r in rs),
            calls=sum(r['transport']['calls'] for r in rs),released_live=rs[-1]['released']['live'],reserved=rs[-1]['released']['system']['reserved'],
            largest_workbook_live=largest['parsed']['live'])
    caps = {k:{Path(r['path']).name:r for r in [json.loads(line) for line in (OUT/f'capacity-{name}.jsonl').read_text().splitlines()]} for k,name in [('before','before'),('after','accepted')]}
    assert all(caps['before'][i]['sizes']==caps['after'][i]['sizes'] for i in caps['before'])
    b,a = (caps[k]['xlsm-1c382d45c18005cd.xlsm'] for k in ['before','after'])
    assert b['sizes']['byte_ranges']==0
    saved = sum((b['bytes_per_slot'][singular]-a['bytes_per_slot'][singular])*a['sizes'][plural][1] for singular,plural in [('block','blocks'),('cell','cells')])
    subset = {e:sum(rows[e][i]['elapsed_ns'] for i in both)/1e9 for e in ['bulkparse','anydoc']}
    result = dict(profiles=profiles,before_plain=before,ordinary_improvement_percent=gain,paired=paired['profiles'],drift_control=drift['profiles'],
        allocations=allocations,source_layout_before=b['bytes_per_slot'],source_layout_after=a['bytes_per_slot'],largest_workbook_layout_saving_bytes=saved,
        common_documents=len(common),both_bulkparse_anydoc=len(both),subset_seconds=subset,
        fresh_comparison_outputs_unchanged=len(baseline_rows))
    (OUT/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    db,da = (drift['profiles'][k] for k in ['before','after'])
    (OUT/'README.md').write_text(f'''# Provenance and worker memory — November 13, 2026

BulkParse still returns **112/112** documents by default, with complete structured
output and Markdown. All full JSON hashes, Markdown outputs, statuses and
diagnostics match the baseline. There are still 13 complete and 99 partial-fidelity
statuses, with no resource-limit prefixes. This is preservation of current
extraction quality, not a new quality score or a claim of perfect fidelity.

## What still drove the peak

The largest XLSM workbook produces {a['sizes']['cells'][0]:,} cells and
{a['sizes']['blocks'][0]:,} blocks. Together they contain
{a['sizes']['sources']:,} source-location records, none with a byte range.
Those records nevertheless reserved inline space for the optional pair of offsets.
The full model is large in its own right; allocator-retained memory and Markdown
transport also contribute to the process peak.

The native source record now occupies **{b['bytes_per_slot']['source']} → {a['bytes_per_slot']['source']} bytes**;
a block **{b['bytes_per_slot']['block']} → {a['bytes_per_slot']['block']} bytes** and
a cell **{b['bytes_per_slot']['cell']} → {a['bytes_per_slot']['cell']} bytes**.
That removes **{saved/2**20:.1f} MiB** of retained layout capacity in the largest
workbook alone, without dropping any cells, blocks or source information.

## Implementation

- Optional byte ranges use a shared, separately allocated fixed array. Absent
  ranges need only a pointer-sized slot; cloning a present range shares its data.
- Shared XLSX text runs are borrowed from the existing bounded cache until needed.
  Plain cells copy only the final text; rich cells still retain their original
  run boundaries and formatting. General/text numeric formats move the existing
  value instead of copying it. Cache limits are unchanged.
- ODF style definitions are grouped by family, letting lookups borrow keys instead
  of allocating temporary family/name strings. Inheritance and fallback rules stay
  the same.
- Already-counted node/cell budgets use constant-time accounting when the entire
  batch fits. The original sequential failure path preserves limit precedence and
  counter state; cancellation is checked around successful accounting. XML and
  cell content validation are unchanged. Boundary/overflow/cancellation tests
  compare errors and counters against the original per-item behavior.
- Block IDs use one exact-sized allocation with the same decimal representation.
  `itoa` was already resolved transitively and is now also a direct dependency;
  no new dependency package/version was introduced for this change.

These rules apply to content and representation, not document identities. Parsing
limits, worksheet/shared-string streaming and ODF list-capacity improvements remain
in place. No worker recycling or allocator purges are used.

### API compatibility

The **JSON Schema is byte-for-byte identical**, including the optional two-integer
`byte_range` array. Node/Python/WASM contracts remain unchanged. The pre-release
Rust field changes from `Option<[usize; 2]>` to `Option<Arc<[usize; 2]>>`: construct
it with `Some([start, end].into())` and read it with `.as_deref()`. See the
[Rust notes](../../docs/implementation-notes.md). Documents containing byte ranges
now allocate shared range objects, so their allocation patterns can differ.

## Performance and memory

{pair_table}
The ordinary worker measured **{before['rss_bytes']/2**20:.1f} → {profiles['bulkparse']['rss_bytes']/2**20:.1f} MiB**
peak RSS ({gain['rss_bytes']:.1f}% lower). Corpus time was
**{before['seconds']:.3f} → {profiles['bulkparse']['seconds']:.3f} s** and CPU time
**{before['cpu_seconds']:.3f} → {profiles['bulkparse']['cpu_seconds']:.3f} s**.
The final ordinary-worker measurement is from the fresh competitor run below.

The [interleaved drift-control check](accepted-drift-check/summary.json) ran each
version once, reversing before/after order on alternating documents. It measured
{db['seconds']:.3f} → {da['seconds']:.3f} s elapsed,
{db['cpu_seconds']:.3f} → {da['cpu_seconds']:.3f} s CPU and
{db['peak_rss_bytes']/2**20:.1f} → {da['peak_rss_bytes']/2**20:.1f} MiB RSS.
This diagnoses small timing differences; it is not substituted for the sequential
competitor protocol. Earlier variants showed small slowdowns and are retained in
`paired`, `final-paired`, `optimized-paired`, `reduced-paired` and their associated
drift checks. Only the accepted version is reported as current.

Each measured profile has one sequential corpus pass and no warm-up. Full JSON
serialization/validation/hashing is excluded from paired parse/Markdown time, but
included in that worker's memory lifetime. Instrumented allocation runs are separate
from performance measurements. RSS varies between runs; no universal memory ceiling
or statistical speed guarantee follows from these development checks.

[Paired results](accepted-paired/summary.json) · [Ordinary baseline](plain-before/summary.json) ·
[Source/binary identity](accepted-paired/identity.json)

## Fresh comparison

{table}
Shared medians use the same **{len(common)}** documents returned by all three.
On the **{len(both)}** inputs both BulkParse and AnyDoc return, total conversion time
was **{subset['bulkparse']:.3f} versus {subset['anydoc']:.3f} s**. Failed attempts
remain in full-corpus totals. A returned result does not establish equal quality.

Same Apple M3 Max, 36 GiB RAM, macOS, AC power; all 112 public inputs across 13
formats. Each converter runs sequentially in one persistent process for one pass,
with OCR disabled. Initialization/imports and protocol serialization are excluded
from conversion time. RSS includes worker initialization/transport and excludes an
aggregate process-tree peak. Other initialized workers may remain idle between
engine runs. No heavy builds or other parsing jobs overlapped timing.
BulkParse retains the complete model plus Markdown; AnyDoc uses its Markdown API
and Docling its configured conversion/export. These APIs retain different data.
These results do not establish Linux production RAM use or typical per-document
usage. The corpus has informed development and is no longer an unseen holdout.

[Raw comparison](comparison/summary.json) · [Settings](comparison/identity.json) ·
[Docling options](comparison/effective-options.json) · [Dependency freeze](comparison/docling-requirements.txt)

## Allocation evidence and regression checks

Instrumented peak live Rust allocations changed from
{allocations['before']['peak_live']/2**20:.1f} to {allocations['after']['peak_live']/2**20:.1f} MiB.
After the corpus, live bytes returned to {allocations['before']['released_live']:,}
before and {allocations['after']['released_live']:,} after; allocator-reserved
memory was {allocations['before']['reserved']/2**20:.1f} and
{allocations['after']['reserved']/2**20:.1f} MiB. Completed models were not retained
in this trace. Requested bytes exclude allocator slack, non-Rust allocations and
transient realloc overlap, and are not RSS.
[Before allocation trace](allocations-before/summary.json) · [After allocation trace](allocations-after/summary.json)

[Validation](validation.json) records all 112 exact-output checks, the historical
failure suite, 200 unchanged PDFs, Rust tests, and binding parity. Expected outputs
were not relaxed. There were no paid judging jobs or new aggregate quality scores.

## Reproduce

```sh
cargo build --offline --release -p bulkparse --example office_bench
python3 scripts/benchmark_parser_speed.py \\
  --before reports/provenance-memory-2026-11-13/before-office-bench \\
  --out /tmp/bulkparse-provenance-paired
python3 scripts/benchmark_realworld_comparison.py --out /tmp/bulkparse-provenance-comparison
```

Use new output directories. Preserved executables are macOS-specific; source
snapshots support rebuilding elsewhere with a separate baseline build directory.
`accepted-office-bench` is the immutable final measured binary. Diagnostic examples
are excluded from speed measurements.
''')
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    main()
