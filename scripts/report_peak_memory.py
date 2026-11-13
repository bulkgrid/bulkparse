#!/usr/bin/env python3
"""Report the ODF retained-capacity fix and fresh three-converter measurements."""
import json
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'reports/peak-memory-2026-11-13'


def read(path):
    return json.loads(path.read_text())


def main():
    paired = read(OUT / 'final-paired/summary.json')
    assert not any(paired['differences'].values())
    assert all(v['returned'] == 112 and not v['truncated'] for v in paired['profiles'].values())
    assert paired['profiles']['after_structured']['json_validated'] == 112
    assert read(OUT / 'comparison/verification.json')['complete']
    summary = read(OUT / 'comparison/summary.json')
    engines = ['bulkparse', 'anydoc', 'docling']
    rows = {e: {r['id']: r for r in read(OUT / f'comparison/{e}-round-1.json')['documents']} for e in engines}
    shared = [i for i in rows['bulkparse'] if all(rows[e][i]['ok'] for e in engines)]
    both = [i for i in rows['bulkparse'] if rows['bulkparse'][i]['ok'] and rows['anydoc'][i]['ok']]
    before_plain = read(OUT / 'plain-before/summary.json')['bulkparse']
    profiles = {}
    table = '| Converter | Returned / 112 | Corpus seconds | CPU seconds | Shared median ms | Peak worker MiB |\n| --- | ---: | ---: | ---: | ---: | ---: |\n'
    for e in engines:
        s = summary[e]
        v = profiles[e] = dict(returned=s['returned_all'], seconds=s['corpus_seconds'][0],
            cpu_seconds=s['corpus_cpu_seconds'][0], rss_bytes=s['resources']['lifetime_peak_rss_bytes'],
            shared_median_ms=statistics.median(rows[e][i]['elapsed_ns']/1e6 for i in shared))
        table += f'| {e} | {v["returned"]} | {v["seconds"]:.3f} | {v["cpu_seconds"]:.3f} | {v["shared_median_ms"]:.3f} | {v["rss_bytes"]/2**20:.1f} |\n'
    pair_table = '| Mode | Seconds before → after | CPU seconds before → after | Worker peak MiB before → after |\n| --- | ---: | ---: | ---: |\n'
    for mode, label in [('markdown', 'Streamed Markdown'), ('structured', 'Structured + full JSON verification')]:
        b, a = (paired['profiles'][k+'_'+mode] for k in ['before', 'after'])
        pair_table += f'| {label} | {b["seconds"]:.3f} → {a["seconds"]:.3f} | {b["cpu_seconds"]:.3f} → {a["cpu_seconds"]:.3f} | {b["worker_peak_rss_bytes"]/2**20:.1f} → {a["worker_peak_rss_bytes"]/2**20:.1f} |\n'
    allocation_rows = {k: read(OUT / f'allocations-{k}/summary.json')['rows'] for k in ['before', 'after']}
    allocation = {k: dict(peak_live=max(r['transport']['peak'] for r in rs),
        allocated=sum(r['transport']['allocated'] for r in rs), calls=sum(r['transport']['calls'] for r in rs)) for k, rs in allocation_rows.items()}
    cap = {k: {Path(r['path']).name:r for r in [json.loads(line) for line in (OUT / f'capacity-{name}.jsonl').read_text().splitlines()]} for k, name in [('before','before'),('after','final')]}
    large = 'ods-b3185d3043cb268a.ods'
    b, a = cap['before'][large], cap['after'][large]
    saved = sum((b['sizes'][key][1]-a['sizes'][key][1])*b['bytes_per_slot'][singular] for key, singular in [('blocks','block'),('inlines','inline'),('cells','cell')])
    assert all(b['sizes'][k][0] == a['sizes'][k][0] for k in b['sizes'])
    subset_seconds = {e: sum(rows[e][i]['elapsed_ns'] for i in both)/1e9 for e in ['bulkparse','anydoc']}
    before = dict(seconds=before_plain['corpus_seconds'][0], cpu_seconds=before_plain['corpus_cpu_seconds'][0], rss_bytes=before_plain['resources']['lifetime_peak_rss_bytes'])
    improvement = {k: 100*(1-profiles['bulkparse'][k]/before[k]) for k in before}
    result = dict(profiles=profiles, before_plain=before, ordinary_improvement_percent=improvement,
        shared_documents=len(shared), shared_bulkparse_anydoc_documents=len(both), subset_seconds=subset_seconds,
        allocation=allocation, largest_ods_unused_capacity_removed_bytes=saved)
    (OUT/'results.json').write_text(json.dumps(result, indent=2)+'\n')
    (OUT/'README.md').write_text(f'''# Peak memory reduction — November 13, 2026

BulkParse still returns **112/112** documents with default settings, complete
structured output and complete Markdown. All 112 full JSON hashes, Markdown
outputs, statuses and diagnostics match the preserved baseline. Thirteen outputs
have complete status and 99 retain fidelity warnings; no output is a resource-limit
prefix. This change does not claim to repair those remaining fidelity limitations.

## What changed

ODF paragraph and block lists commonly hold one item, especially in spreadsheets.
Their first append previously reserved four slots, leaving that spare capacity
alive in every cell. They now start at one slot and grow normally as needed.
Empty or unmapped content returns a zero-capacity empty list. There is no later
document-wide shrinking pass. Text, styles, merged cells, provenance, IDs,
validation and limit accounting are unchanged.

For the largest ODS document, retained block capacity fell from
{b['sizes']['blocks'][1]:,} to {a['sizes']['blocks'][1]:,} slots and inline capacity
from {b['sizes']['inlines'][1]:,} to {a['sizes']['inlines'][1]:,}, holding the same
{a['sizes']['blocks'][0]:,} blocks and {a['sizes']['inlines'][0]:,} inlines.
That removes **{saved/2**20:.1f} MiB of unused model capacity**. This is a general
allocation rule for ODF content, with no document names or identifiers in the parser.
Other formats, spreadsheet XML/shared-string streaming and the public schema
remain unchanged. No new dependencies, higher limits, allocator purges or worker
recycling are used.

## Before and after

{pair_table}
The ordinary structured/Markdown worker, without the extra JSON verification,
used **{before['rss_bytes']/2**20:.1f} → {profiles['bulkparse']['rss_bytes']/2**20:.1f} MiB**
peak RSS ({improvement['rss_bytes']:.1f}% lower). Its conversion time was
**{before['seconds']:.3f} → {profiles['bulkparse']['seconds']:.3f} s** and CPU time
**{before['cpu_seconds']:.3f} → {profiles['bulkparse']['cpu_seconds']:.3f} s**.

The baseline was frozen before editing and measured once per mode. The final
ordinary-worker result comes from the fresh competitor run below. Extra JSON
serialization, validation to EOF and hashing are excluded from paired conversion
timing but included in that worker's memory lifetime. Do not compare peaks across
these different verification profiles as if their work were identical.
The writer was already releasing rows as it rendered them; its measured peak
stayed around 0.46 GiB (466.2 → 471.7 MiB). The substantial reduction is in the
worker retaining the full structured model. Per-document times can still vary.

[Paired measurements](final-paired/summary.json) ·
[Ordinary baseline](plain-before/summary.json) ·
[Final source and binary identities](final-paired/identity.json)

## Fresh competitor comparison

{table}
The shared median uses the same **{len(shared)}** inputs returned by all three
tools. On the **{len(both)}** inputs both BulkParse and AnyDoc return, total time
was **{subset_seconds['bulkparse']:.3f} s versus {subset_seconds['anydoc']:.3f} s**.
Failed attempts remain in the full-corpus times. Returning a result does not
establish equivalent extraction quality, and status conventions differ.

Apple M3 Max, 36 GiB RAM, macOS; AC power. All 112 public inputs across 13 formats
are retained. Each converter runs sequentially in one persistent process, one
measured pass with no warm-up. OCR is disabled. Worker setup/import time and
protocol serialization are excluded from conversion time. Peak RSS includes the
worker's lifetime and transport; it is not an aggregate concurrent process-tree
peak. Other initialized workers may remain idle between engine runs. There were
no concurrent heavy builds or other parsing jobs during these measurements.

BulkParse produces the complete structured model plus Markdown. AnyDoc's adapter
uses its Markdown API; Docling uses its configured document conversion/export.
The exact options, dependency freeze, failures, input/source/binary hashes and
per-document predictions are retained. These one-pass development results do not
establish a universal speed guarantee, memory ceiling or Linux deployment result.
Allocator retention explains why the fresh baseline peak differs from earlier
runs. This corpus has informed development and is not an unseen holdout.

[Raw comparison](comparison/summary.json) · [Settings](comparison/identity.json) ·
[Effective Docling options](comparison/effective-options.json) ·
[Docling dependencies](comparison/docling-requirements.txt)

## Allocation diagnostics and validation

Instrumented Rust-requested peak live allocations changed from
{allocation['before']['peak_live']/2**20:.1f} to {allocation['after']['peak_live']/2**20:.1f} MiB.
These counters exclude non-Rust allocations, allocator slack and transient
reallocation overlap; they are not RSS. Their timings are not benchmark results.
Full per-document stage measurements are retained in
[before](allocations-before/summary.json) and [after](allocations-after/summary.json).
The capacity inspection example only observes the returned model and does not
change parser behavior.

[Validation](validation.json) records the historical-failure suite, all 200 PDF
output comparisons, Rust tests and Node/Python/WASM/CLI parity. Expected outputs
were not relaxed. No paid judging or new aggregate quality score is involved.
The initial candidate measurements under `paired` and `plain-after` predate the
empty-list guard; the final measurements above supersede them.

## Reproduce

```sh
cargo build --offline --release -p bulkparse --example office_bench
python3 scripts/benchmark_parser_speed.py \\
  --before reports/peak-memory-2026-11-13/before-office-bench \\
  --out /tmp/bulkparse-memory-paired
python3 scripts/benchmark_realworld_comparison.py --out /tmp/bulkparse-memory-comparison
```

Use new output directories. Preserved executables are macOS-specific. Source
snapshots permit rebuilding on other systems; use a separate build directory
for the baseline. `final-office-bench` is the immutable measured final executable.
`model_capacity` and `allocation_profile` are diagnostic examples, excluded from
timed benchmarks.
''')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
