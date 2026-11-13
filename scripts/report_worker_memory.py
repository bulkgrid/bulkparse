#!/usr/bin/env python3
"""Verify and publish the measured whole-worker memory follow-up."""
import hashlib
import json
from pathlib import Path
import re
import statistics

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'reports/worker-memory-2026-11-13'
PAIRS = [('before', 'default'), ('before_bounded', 'bounded'),
         ('before_stream', 'stream'), ('before_stream_bounded', 'stream_bounded'),
         ('before_stream_2', 'stream_2'), ('before_stream_3', 'stream_3')]


def read(name):
    return json.loads((OUT / name).read_text())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(name, value):
    (OUT / name).write_text(json.dumps(value, indent=2) + '\n')


def main():
    identity = read('benchmark-final/identity.json')
    sources = read('benchmark-final/source-manifest.json')
    assert all(sha(ROOT / p) == h for p, h in sources.items())
    assert all(sha(Path(cmd[0])) == identity['binary_sha256'][k] for k, cmd in identity['commands'].items())
    fields = ['ok', 'status', 'output_sha256', 'diagnostics', 'render_diagnostics', 'error_code', 'error']
    differences = []
    for run in range(1, 6):
        for a, b in PAIRS:
            old = read(f'benchmark-final/{a}-pass-{run}.json')['documents']
            new = read(f'benchmark-final/{b}-pass-{run}.json')['documents']
            assert len(old) == len(new) == 112
            for x, y in zip(old, new):
                assert x['id'] == y['id']
                changed = [k for k in fields if x.get(k) != y.get(k)]
                if changed:
                    differences.append(dict(run=run, reference=a, profile=b, id=x['id'], fields=changed))
    assert not differences, differences
    save('all-pass-regressions.json', dict(passes=5, pairs=PAIRS, documents=112, differences=differences))
    checks = read('regression-checks.json')
    assert checks['limits_unchanged']
    assert all(not r['issues'] for r in checks['source_checks'])
    assert all(len(rows) == 81 and all(r['unchanged'] for r in rows) for rows in checks['development'].values())
    assert not read('pdf-regression-checks.json')['changed_documents']
    before = {(r['mode'], r['id']): r for r in read('per-document-before/summary.json')}
    after = {(r['mode'], r['id']): r for r in read('per-document-after/summary.json')}
    assert before.keys() == after.keys() and len(before) == 448
    assert all(all(a.get(k) == before[key].get(k) for k in ['ok', 'status', 'output_sha256', 'error']) for key, a in after.items())
    for name, mode in [('before', 'before'), ('after', 'default')]:
        assert read(f'per-document-{name}/identity.json')['binary_sha256'] == identity['binary_sha256'][mode]
    tests = {}
    for name in ['tests-final', 'minimal-tests']:
        results = re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed', (OUT / (name + '.log')).read_text())
        assert results and all(r[0] == 'ok' and r[2] == '0' for r in results)
        tests[name] = sum(int(r[1]) for r in results)
    assert 'Ran 4 tests' in (OUT / 'python-tests.log').read_text() and '\nOK\n' in (OUT / 'python-tests.log').read_text()
    assert 'pass 6' in (OUT / 'node-tests.log').read_text() and 'fail 0' in (OUT / 'node-tests.log').read_text()
    assert 'WASM PDF inspection tests passed' in (OUT / 'wasm-tests.log').read_text()
    assert '27 cases match' in (OUT / 'binding-parity.log').read_text() and '3 PDF inspection cases match' in (OUT / 'binding-parity.log').read_text()
    v = read('benchmark-final/summary.json')['versions']
    repetition = {}
    for name in ['before_stream', 'stream']:
        profiles = [v[name + suffix] for suffix in ['', '_2', '_3']]
        peaks = [r['lifetime_peak_rss_bytes']/2**20 for r in profiles]
        times = [r['median_corpus_seconds'] for r in profiles]
        repetition[name] = dict(peaks_mib=peaks, median_peak_mib=statistics.median(peaks),
                                corpus_seconds=times, median_seconds=statistics.median(times))
    a, b = repetition['before_stream'], repetition['stream']
    table = '| Output | Before seconds / 112 | After seconds / 112 | Before peak RSS MiB | After peak RSS MiB |\n| --- | ---: | ---: | ---: | ---: |\n'
    delta = {}
    for label, (old, new) in zip(['Structured', 'Structured + recovery', 'Markdown file', 'Markdown file + recovery'], PAIRS):
        t0, t1 = v[old]['median_corpus_seconds'], v[new]['median_corpus_seconds']
        r0, r1 = v[old]['lifetime_peak_rss_bytes']/2**20, v[new]['lifetime_peak_rss_bytes']/2**20
        if new == 'stream':
            t0, t1, r0, r1 = a['median_seconds'], b['median_seconds'], a['median_peak_mib'], b['median_peak_mib']
        table += f'| {label} | {t0:.3f} | {t1:.3f} | {r0:.1f} | {r1:.1f} |\n'
        delta[new] = dict(time_percent=100*(t1/t0-1), rss_percent=100*(r1/r0-1))
    repeats = '| Independent worker pair | Before seconds | After seconds | Before peak MiB | After peak MiB |\n| --- | ---: | ---: | ---: | ---: |\n'
    for i in range(3):
        repeats += f"| {i+1} | {a['corpus_seconds'][i]:.3f} | {b['corpus_seconds'][i]:.3f} | {a['peaks_mib'][i]:.1f} | {b['peaks_mib'][i]:.1f} |\n"
    save('comparison.json', dict(profiles=delta, markdown_repetitions=repetition))
    timings = []
    for old, new in PAIRS:
        reference = {r['id']: r for r in v[old]['documents']}
        timings.extend(dict(id=r['id'], profile=new, before_ms=reference[r['id']]['median_ms'], after_ms=r['median_ms']) for r in v[new]['documents'])
    save('per-document-timing-comparison.json', timings)
    assert read('allocation-profile-identity.json')['baseline_source_verified']
    allocation = {name: read(f'allocations-{name}/summary.json')['rows'] for name in ['before', 'after']}
    assert all(len(rows) == 336 for rows in allocation.values())
    last = {name: [rows[i]['released'] for i in [111, 223, 335]] for name, rows in allocation.items()}
    alloc_table = '| After each 112-document pass | Before reserved MiB | After reserved MiB | Before live Rust KiB | After live Rust KiB |\n| --- | ---: | ---: | ---: | ---: |\n'
    for i in range(3):
        x, y = last['before'][i], last['after'][i]
        alloc_table += f"| {i+1} | {x['system']['reserved']/2**20:.1f} | {y['system']['reserved']/2**20:.1f} | {x['live']/1024:.1f} | {y['live']/1024:.1f} |\n"
    allocations = []
    for x, y in zip(allocation['before'], allocation['after']):
        assert (x['id'], x['run']) == (y['id'], y['run'])
        allocations.append(dict(id=x['id'], run=x['run'], before_peak=x['parsed']['peak'], after_peak=y['parsed']['peak'], before_calls=x['parsed']['calls'], after_calls=y['parsed']['calls']))
    save('allocation-comparison.json', dict(end_of_passes=last, documents=allocations))
    peaks = '| Document | Mode | Returned | Before MiB | After MiB |\n| --- | --- | --- | ---: | ---: |\n'
    selected = [('stream','xls-aed2154ce0e620bd'), ('stream','xls-589ace4188fb7ccc'), ('stream','ods-20f58e9c6ef54e2c'),
                ('stream','xlsm-1c382d45c18005cd'), ('stream','xlsx-43eab8ba38301e43'), ('stream','xlsm-c6ee0b16ff832902'),
                ('stream_bounded','xlsx-3d7484ae52ab377f'), ('bounded','csv-fbeb7b5e9b4f3ed9')]
    for key in selected:
        x, y = before[key], after[key]
        peaks += f"| `{key[1]}` | `{key[0]}` | {'yes' if y['ok'] else 'no'} | {x['peak_rss_bytes']/2**20:.1f} | {y['peak_rss_bytes']/2**20:.1f} |\n"
    charge = [re.search(r'(\d+)%;', (OUT / f'power-{name}.txt').read_text())[1] for name in ['before', 'after']]
    spreadsheet_cells = sum(r['checked'] for r in checks['source_checks'] if r['document_id'].split('-')[0] in ['xlsx','xlsm','ods'])
    csv_rows = sum(r['checked'] for r in checks['source_checks'] if r['document_id'].startswith('csv-'))
    body = f'''# Whole-worker memory — November 13, 2026

The repeated Markdown worker peak fell from **{a['median_peak_mib']:.1f} to {b['median_peak_mib']:.1f} MiB** ({-delta['stream']['rss_percent']:.1f}% lower). Median corpus time was **{a['median_seconds']:.3f} → {b['median_seconds']:.3f} seconds** ({delta['stream']['time_percent']:+.2f}%). The existing spreadsheet XML/shared-string streaming remains enabled. Limits, conversion outcomes and output content are unchanged.

## Measurements

{table}
112 public documents across 13 formats; OCR disabled. Each worker processes documents sequentially in one process: one excluded complete warm-up, then five measured full-corpus passes. Before/after order alternates. Only one pair is resident at a time. The non-recovery Markdown profile repeats with three independent pairs; its row is the median of their three corpus-time medians and three lifetime peak RSS values. Other rows have one worker pair each, so their memory differences are less robust.

Hardware: Apple M3 Max, 36 GiB RAM, 14 logical CPUs, macOS. This run was on battery ({charge[0]}% in the early power snapshot, {charge[1]}% after), standard power mode. Both builds ran under these conditions; no builds or other verification jobs overlapped timing. Compare the matched builds here, not historical AC-power timings. Raw power snapshots are retained.

Timing includes input reading, parsing and rendering. Markdown-file timing includes staging and file writes. Verification readback and JSON serialization are excluded from time but included in worker memory. The benchmark worker, allocator and transport are unchanged; workers are not recycled within a profile. The allocation-instrumented example is never used for these timing/RSS results.

### Repeated non-recovery Markdown workers

{repeats}
Before peaks ranged **{min(a['peaks_mib']):.1f}–{max(a['peaks_mib']):.1f} MiB**; after peaks ranged **{min(b['peaks_mib']):.1f}–{max(b['peaks_mib']):.1f} MiB**. The ranges do not overlap in this run. Three repetitions are evidence of improvement on this machine, not a universal memory bound or a guarantee for every document.

## What was retaining memory?

A separate diagnostic counts Rust allocation requests and samples `malloc_zone_statistics` from the installed macOS SDK. After each document it drops input, parser results, Markdown verification buffers and response data. Live allocations return to a small stable level, while the system allocator retains substantially more reserved memory. There was no rising live-allocation trend across three corpus passes in this trace. This points to temporary allocation patterns and allocator retention, rather than completed documents accumulating in the parser.

{alloc_table}
These are **allocator-reserved bytes, not RSS**. The small live counts include diagnostic bookkeeping. Rust counters omit non-Rust allocations and transient old/new overlap inside reallocations. The diagnostic changes allocation overhead, so its figures are explanatory evidence; the uninstrumented benchmark above establishes the actual process-memory improvement. The baseline diagnostic was built from the preserved baseline source with the same instrumentation source, verified in `allocation-profile-identity.json`.

Large ODS and XLS cell models were among the highest live-allocation peaks. XLSX also retained a second plain-text/inline representation for cells whose eventual representation was identical. The diagnostic records parser, file-readback, serialization and post-release stages separately, so verification transport is not silently attributed to parsing.

## Changes

1. **Release final table rows during Markdown output.** XLS, XLSB and native ODS now consume their sparse cell/block map one row at a time and release each final row after writing it. They no longer retain the complete final row/cell grid while rendering. Merged-cell content, HTML table selection, multiple header rows, rich/nested content and rendering order use the existing rendering logic. Structured APIs still return their full model.
2. **Preserve failure and recovery behavior.** Remaining table-cell/node budgets are checked before streaming table rows, and before flushing the queued heading in XLS. XLS recovery therefore retains only completed sheets; a failed sheet cannot leave a heading or partial row in the output. Original malformed-input checks and limit precedence are retained. The existing spool still protects caller output on parsing/strict failures.
3. **Release ODS template mappings earlier.** Cell-template coordinates are dropped once the rich cell map has been constructed, before the final table is assembled or rendered. Worksheet XML fragments, buffering and shared-string streaming are unchanged.
4. **Avoid duplicate plain XLSX decorations.** Plain cells use the exact inline already produced by the base table. Styles, rich-run boundaries and hyperlinks remain preserved; only redundant decoration copies are omitted.

No document identifiers, filenames or corpus-specific rules were added. No parsing limits were raised. No allocator purge calls, alternative allocator, worker recycling or benchmark serialization shortcut was introduced.

## Isolated document peaks

Each of the 112 documents was also measured in a fresh process in all four modes, before and after: 896 process measurements. The table includes large contributors and previously improved spreadsheet/CSV cases; complete data is retained.

{peaks}
These single-process values can vary slightly. Rejected inputs remain in the workload; lower memory on rejection is not successful extraction. Some input buffers and a worksheet's sparse cell/block map still exist. XLSX still materializes its final table, with fewer duplicate decorations. These changes do not make every format constant-memory.

## Validation and remaining limits

- All 112 outputs, statuses, diagnostics, rendering diagnostics, error codes and messages match across all five measured passes and all six worker pairs.
- 81 Office fixtures match in all four modes; all 200 PDF outputs and diagnostics match.
- Independent source checks validate {spreadsheet_cells:,} retained spreadsheet cells and {csv_rows:,} complete CSV records, with no differences. Default limits match the preserved implementation byte-for-byte.
- {tests['tests-final']} full-feature Rust test executions and {tests['minimal-tests']} minimal-feature tests pass. New coverage sweeps merged-table node/cell/text limits and atomic output, checks XLS recovery at a later sheet's cell limit, and preserves XLSX rich-run boundaries.
- Six Node tests, four Python tests, WASM smoke/inspection checks, 27 parsing comparisons and three PDF-inspection comparisons across bindings pass.
- Default parsing still returns 97/112 documents. Recovery returns 112/112, including 15 resource-limited prefixes; only 11 results have complete status. No new quality score or eight-converter quality leaderboard is claimed.
- Per-document timing tradeoffs are retained in `per-document-timing-comparison.json`; an aggregate improvement does not mean every input is faster. These macOS results do not establish Linux production memory use or a fixed concurrency budget.

## Reproduce

```sh
cargo test --offline -p bulkparse -p bulkparse-cli -p bulkparse-pool
cargo build --offline --release -p bulkparse --example office_bench --example allocation_profile
python3 scripts/benchmark_worker_memory.py --out /tmp/bulkparse-worker-benchmark
python3 scripts/profile_parser_memory.py --binary reports/worker-memory-2026-11-13/before-office-bench --out /tmp/bulkparse-worker-before --modes default bounded stream stream_bounded
python3 scripts/profile_parser_memory.py --binary target/release/examples/office_bench --out /tmp/bulkparse-worker-after --modes default bounded stream stream_bounded
python3 scripts/profile_worker_allocations.py --binary target/release/examples/allocation_profile --out /tmp/bulkparse-worker-allocations
```

Use fresh output directories. For the instrumented baseline, copy `before/source-snapshot` to a scratch workspace, add the identical `allocation_profile.rs` example, and build it there; the recorded command and source verification are in `allocation-profile-identity.json`. The preserved before binary is for this Mac. Source snapshots, input/binary hashes, predictions and all passes are in `benchmark-final`. This corpus is now development/regression evidence.
'''
    (OUT / 'README.md').write_text(body)
    section = f'''<!-- realworld-benchmark:start -->
### Real-world large-document handling

**112 public documents across 13 formats.** Native spreadsheet parsing streams worksheet XML and shared strings. XLS/XLSB/ODS Markdown output releases final table rows as it writes them, and XLSX avoids duplicate plain-cell decorations.

{table}
The repeated Markdown worker peak is **{-delta['stream']['rss_percent']:.1f}% lower**, with essentially unchanged corpus time. Each worker uses one warm-up and five alternating sequential full-corpus passes; Markdown-file figures are medians across three independent worker pairs. Other profiles use one pair. Same Apple M3 Max, standard battery power, OCR disabled. Peak RSS is a measured process high-water mark, not a memory guarantee; the report includes repeat ranges, allocation diagnostics and per-document tradeoffs.

Default parsing returns **97/112**; explicit recovery returns **112/112**, including **15 partial prefixes**. Only 11 results have complete status. All outputs and diagnostics remain unchanged, and 81 Office fixtures plus 200 PDFs pass regression checks. This corpus is regression evidence. The broader eight-converter blind quality review remains unfinished; no quality leaderboard is claimed.

[Measurements, causes and limitations](reports/worker-memory-2026-11-13/README.md) · [Streaming API](docs/streaming-markdown.md) · [Corpus and study status](benchmarks/realworld/README.md)
<!-- realworld-benchmark:end -->'''
    path = ROOT / 'README.md'
    text = path.read_text()
    start = text.index('<!-- realworld-benchmark:start -->')
    end = text.index('<!-- realworld-benchmark:end -->') + len('<!-- realworld-benchmark:end -->')
    path.write_text(text[:start] + section + text[end:])
    print(json.dumps(dict(profiles=delta, markdown_repetitions=repetition, tests=tests), indent=2))


if __name__ == '__main__':
    main()
