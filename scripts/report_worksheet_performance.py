#!/usr/bin/env python3
"""Verify matched outputs and publish the worksheet performance follow-up."""
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'reports/worksheet-performance-2026-11-12'


def read(name):
    return json.loads((OUT / name).read_text())


def main():
    v = read('benchmark-final/summary.json')['versions']
    source = read('benchmark-final/source-manifest.json')
    assert all(hashlib.sha256((ROOT / p).read_bytes()).hexdigest() == h for p, h in source.items())
    pairs = [('before', 'default'), ('before_bounded', 'bounded'),
             ('before_stream', 'stream'), ('before_stream_bounded', 'stream_bounded'),
             ('original', 'default'), ('original_bounded', 'bounded'),
             ('original_stream', 'stream'), ('original_stream_bounded', 'stream_bounded')]
    fields = ['ok', 'status', 'output_sha256', 'diagnostics', 'render_diagnostics', 'error_code', 'error']
    differences = []
    for run in range(1, 6):
        for a, b in pairs:
            old = read(f'benchmark-final/{a}-pass-{run}.json')['documents']
            new = read(f'benchmark-final/{b}-pass-{run}.json')['documents']
            assert len(old) == len(new) == 112
            for x, y in zip(old, new):
                assert x['id'] == y['id']
                changed = [k for k in fields if x.get(k) != y.get(k)]
                if changed:
                    differences.append(dict(run=run, profile=b, reference=a, id=x['id'], fields=changed))
    assert not differences, differences
    checks = read('regression-checks.json')
    assert checks['limits_unchanged']
    assert all(not r['issues'] for r in checks['source_checks'])
    assert all(r['unchanged'] for rows in checks['development'].values() for r in rows)
    assert not read('pdf-regression-checks.json')['changed_documents']
    before = {(r['mode'], r['id']): r for r in read('per-document-before/summary.json')}
    after = {(r['mode'], r['id']): r for r in read('per-document-after/summary.json')}
    assert before.keys() == after.keys()
    assert all(all(a[k] == before[key][k] for k in ['ok', 'status', 'output_sha256']) for key, a in after.items())
    (OUT / 'all-pass-regressions.json').write_text(json.dumps(dict(passes=5, comparisons=pairs, documents=112, differences=differences), indent=2) + '\n')
    labels = {'default': 'Structured', 'bounded': 'Structured + recovery',
              'stream': 'Markdown file', 'stream_bounded': 'Markdown file + recovery'}
    stages = [('original', 'before', 'default'), ('original_bounded', 'before_bounded', 'bounded'),
              ('original_stream', 'before_stream', 'stream'), ('original_stream_bounded', 'before_stream_bounded', 'stream_bounded')]
    table = '| Output | Original seconds | Regressed seconds | Fixed seconds | Original RSS MiB | Regressed RSS MiB | Fixed RSS MiB |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: |\n'
    delta = {}
    for original, regressed, fixed in stages:
        a, b, c = (v[k] for k in [original, regressed, fixed])
        table += f"| {labels[fixed]} | {a['median_corpus_seconds']:.3f} | {b['median_corpus_seconds']:.3f} | {c['median_corpus_seconds']:.3f} | {a['lifetime_peak_rss_bytes']/2**20:.1f} | {b['lifetime_peak_rss_bytes']/2**20:.1f} | {c['lifetime_peak_rss_bytes']/2**20:.1f} |\n"
        delta[fixed] = {name: {'time_percent': 100 * (c['median_corpus_seconds'] / ref['median_corpus_seconds'] - 1),
                             'rss_percent': 100 * (c['lifetime_peak_rss_bytes'] / ref['lifetime_peak_rss_bytes'] - 1)}
                        for name, ref in [('vs_regressed', b), ('vs_original', a)]}
    (OUT / 'comparison.json').write_text(json.dumps(delta, indent=2) + '\n')
    peaks = '| Document | Profile | Regressed MiB | Fixed MiB |\n| --- | --- | ---: | ---: |\n'
    selected = [('stream', 'xlsm-c6ee0b16ff832902'), ('stream', 'ods-20f58e9c6ef54e2c'),
                ('stream', 'xlsx-43eab8ba38301e43'), ('stream_bounded', 'xlsx-3d7484ae52ab377f'),
                ('bounded', 'csv-fbeb7b5e9b4f3ed9'), ('stream_bounded', 'xlsm-1c382d45c18005cd')]
    for mode, doc in selected:
        a, b = before[mode, doc], after[mode, doc]
        peaks += f"| {a['title']} (`{doc}`) | {mode} | {a['peak_rss_bytes']/2**20:.1f} | {b['peak_rss_bytes']/2**20:.1f} |\n"
    tests = {}
    for name in ['tests-final', 'minimal-tests']:
        results = re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed', (OUT / (name + '.log')).read_text())
        assert results and all(r[0] == 'ok' and r[2] == '0' for r in results)
        tests[name] = sum(int(r[1]) for r in results)
    # Report every remaining per-document slowdown, rather than hiding it in a corpus total.
    slow = []
    for original, regressed, fixed in stages:
        a = {r['id']: r for r in v[original]['documents']}
        b = {r['id']: r for r in v[regressed]['documents']}
        for r in v[fixed]['documents']:
            slow.append(dict(id=r['id'], profile=fixed, original_ms=a[r['id']]['median_ms'],
                             regressed_ms=b[r['id']]['median_ms'], fixed_ms=r['median_ms']))
    (OUT / 'per-document-timing-comparison.json').write_text(json.dumps(slow, indent=2) + '\n')
    body = f'''# Worksheet performance follow-up — November 12, 2026

This follow-up compares the original parser, the worksheet-streaming build that introduced the regressions, and the optimized build on the same machine. No parsing limits were increased, no OCR was used, and no document-specific rules were added.

## Results

{table}
Times are the median of five sequential 112-document corpus totals, after one excluded full warm-up. Peak RSS is the worker's process-lifetime high-water mark, including warm-up and verification transport. Each profile has its own persistent worker. All twelve profiles rotate order between passes. Builds and other test jobs did not overlap timed passes. Hardware: Apple M3 Max, 36 GiB RAM, 14 logical CPUs, macOS.

For default structured parsing, elapsed time changed by **{delta['default']['vs_regressed']['time_percent']:+.1f}%** versus the regressed build and **{delta['default']['vs_original']['time_percent']:+.1f}%** versus the original. Markdown-file recovery time changed by **{delta['stream_bounded']['vs_regressed']['time_percent']:+.1f}%** and **{delta['stream_bounded']['vs_original']['time_percent']:+.1f}%**, respectively. Positive percentages mean an increase. The table deliberately retains the original baseline: improvements versus the regressed build do not establish that every workload has recovered its original speed.

Recovery peak RSS changed by **{delta['bounded']['vs_regressed']['rss_percent']:+.1f}%** for structured output and **{delta['stream_bounded']['vs_regressed']['rss_percent']:+.1f}%** for Markdown files versus the regressed build. Memory did not improve uniformly: the non-recovery Markdown worker increased from **{v['before_stream']['lifetime_peak_rss_bytes']/2**20:.1f} to {v['stream']['lifetime_peak_rss_bytes']/2**20:.1f} MiB**, while remaining below the original build's **{v['original_stream']['lifetime_peak_rss_bytes']/2**20:.1f} MiB**. The fresh-process measurements below separately show that the spreadsheet XML memory savings were retained.

Structured and file-output profiles have different contracts; compare each with the same profile in the other builds. File timings include temporary storage and output-file writes. Verification readback and JSON protocol serialization are outside the elapsed timer but inside lifetime RSS. These are macOS results, not Linux production guarantees or hard memory bounds.

## What caused the overhead, and what changed?

1. **Repeated XML work.** CPU sampling and code inspection identified row parsing, namespace handling and fragment storage as hot paths. XLSX rows are now decoded during validation and replayed as compact extraction records. ODS records retain planning data, avoiding a separate XML parse just for repeats, cached values and cell bounds. ODS rich-content construction still parses its row XML. Consecutive rows share immutable namespace wrappers, element-name checks avoid allocating temporary strings, and depth validation walks tree edges once rather than recounting every ancestor chain.
2. **Many small temporary-file operations.** The old fragment store sought before every append and sought/read each record separately. It now batches writes and uses a fixed 64 KiB read window. Both buffers remain bounded. The shared-string cache and worksheet streaming remain enabled.
3. **Recovery retained unnecessary structures.** Markdown-only XLSX/ODS recovery previously built the entire coordinate/value preview table before rendering it. It now emits and releases each preview row. CSV parsing first checks the exact cell/text/node budgets with a reusable record, avoiding a failed full-table allocation or staged output followed by a second pass for recovery. Unmerged spreadsheet tables also check paragraph and grid budgets before allocating a model that would be discarded. Structured recovery still returns its model, but releases failed-attempt context before retrying. Known-width CSV rows and unmerged spreadsheet grids reserve their required cell slots within the existing limits instead of growing geometrically; merged rows keep their existing allocation strategy.
4. **Rendering churn.** Plain table cells previously allocated intermediate inline, block and replacement strings. A general plain-text path now escapes directly to the output writer, preserving whitespace, Unicode, pipes, newlines and Markdown escaping. Rich cells retain the general renderer.

The diagnostic post-response RSS trace rose to 205.6 MiB after two regressed recovery passes versus 188.2 MiB for the original build. That trace samples memory *after* each response, not peak live parser allocations. It supports investigating retained allocations between documents; it does not prove a leak or uniquely attribute the previous 31% lifetime peak increase. The fixes remove identifiable allocation sources, and their effect is measured above.

[Symbolized CPU sample](ods-symbols-sample.txt) · [Regressed memory trace](retention-regressed.json) · [Original memory trace](retention-original.json)

## Per-document memory

Fresh process per document/profile, one run each; these complement the persistent-worker measurements above. All input and executable hashes are recorded.

{peaks}
[All before profiles](per-document-before/summary.json) · [All after profiles](per-document-after/summary.json) · [All per-document timing comparisons](per-document-timing-comparison.json)

## Correctness and limits

- All 112 documents preserve Markdown hashes, success/failure, status, parser/render diagnostics, error codes and error messages across all five measured passes, against both preserved builds.
- All 81 Office fixtures pass in four output/recovery profiles. All 200 opendataloader-bench PDFs preserve output, statuses and diagnostics. No new PDF quality score is inferred.
- Independent source checks verify 190,145 retained spreadsheet cells and 5,555 complete CSV records. Four recovered XLS inputs still lack an independent per-cell source audit.
- {tests['tests-final']} full-feature Rust test executions and {tests['minimal-tests']} minimal-feature executions pass. Node, Python and WASM tests pass; 27 parsing cases and three PDF inspections agree across the four bindings.
- Added tests exercise buffered-store spill, random reads, cache boundaries, appending after reads, recovery across sheets and exact stopping points, plain-cell rendering equivalence for Unicode, whitespace and escaping, and CSV cell/text/node limit ordering.

Default parsing still returns 97/112 documents; explicit recovery returns 112/112, including 15 bounded prefixes. Only 11 results have complete status; recovery has 101 partial results. Limits and recovery coverage did not change. Input bytes, styles, layouts and successful worksheet cell models remain buffered. Some individual spreadsheets remain slower than the original full-tree implementation; consult the per-document table. This corpus has informed development and is regression evidence. The broader eight-converter blind quality review remains unfinished.

[Five-pass checks](all-pass-regressions.json) · [Office/source checks](regression-checks.json) · [PDF checks](pdf-regression-checks.json) · [Configuration and hashes](benchmark-final/identity.json) · [Final source snapshot](benchmark-final/source-snapshot/)

## Reproduce

Preserve both baseline binaries and their source snapshots, and the hash-pinned corpus. Use fresh output directories:

```sh
cargo build --offline --release -p bulkparse --example office_bench
python3 scripts/benchmark_worksheet_performance.py --out reports/worksheet-performance-recheck/benchmark-final
python3 scripts/profile_parser_memory.py --binary target/release/examples/office_bench --out reports/worksheet-performance-recheck/per-document-after --modes default bounded stream stream_bounded
```

For this recorded report root:

```sh
target/competitors/docling-venv/bin/python scripts/validate_large_documents.py --out reports/worksheet-performance-2026-11-12
BULKPARSE_CHECK_REPORT=reports/worksheet-performance-2026-11-12 python3 scripts/check_streaming_pdf.py
python3 scripts/report_worksheet_performance.py
```

The validator uses BeautifulSoup and markdown-it-py, not Docling conversion. Revalidation elsewhere requires `before/source-snapshot` and `before-office-bench` beside the benchmark directory. Memory measurement uses macOS `/usr/bin/time -l`. Intermediate probes are retained separately; only `benchmark-final` and `per-document-after` provide final measurements.
'''
    (OUT / 'README.md').write_text(body)
    concise = '| Output | Regressed seconds / 112 | Fixed seconds / 112 | Regressed RSS MiB | Fixed RSS MiB |\n| --- | ---: | ---: | ---: | ---: |\n'
    for _, old, new in stages:
        a, b = v[old], v[new]
        concise += f"| {labels[new]} | {a['median_corpus_seconds']:.3f} | {b['median_corpus_seconds']:.3f} | {a['lifetime_peak_rss_bytes']/2**20:.1f} | {b['lifetime_peak_rss_bytes']/2**20:.1f} |\n"
    section = f'''<!-- realworld-benchmark:start -->
### Real-world large-document handling

**112 public documents across 13 formats.** Native spreadsheet parsing streams large worksheet XML and shared strings. The latest follow-up reduces repeated XML work and temporary allocations, and streams Markdown recovery previews without retaining the whole preview table.

{concise}
Matched workloads on the same Apple M3 Max, OCR disabled: one excluded warm-up and five rotating sequential full-corpus passes. The full report also compares the earlier parser and lists per-document tradeoffs; a corpus improvement does not mean every spreadsheet is faster. Peak RSS is a measured process high-water mark, not a memory guarantee.

Default parsing returns **97/112**; explicit recovery returns **112/112**, including **15 partial prefixes**. Only 11 results have complete status. All 112 outputs, statuses and diagnostics remain unchanged; 81 Office fixtures and 200 PDFs also pass regression checks. This corpus is regression evidence. The broader eight-converter blind quality review remains unfinished; no quality leaderboard is claimed.

[Measurements, causes and limitations](reports/worksheet-performance-2026-11-12/README.md) · [Streaming API](docs/streaming-markdown.md) · [Corpus and study status](benchmarks/realworld/README.md)
<!-- realworld-benchmark:end -->'''
    path = ROOT / 'README.md'
    text = path.read_text()
    start = text.index('<!-- realworld-benchmark:start -->')
    end = text.index('<!-- realworld-benchmark:end -->', start) + len('<!-- realworld-benchmark:end -->')
    path.write_text(text[:start] + section + text[end:])
    print('All comparisons verified; report and README updated.')


if __name__ == '__main__':
    main()
