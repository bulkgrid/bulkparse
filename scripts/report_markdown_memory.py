#!/usr/bin/env python3
"""Verify the Markdown allocation follow-up and generate its measured report."""
import hashlib
import json
from pathlib import Path
import re
import statistics

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'reports/markdown-memory-2026-11-12'
PAIRS = [('before', 'default'), ('before_bounded', 'bounded'),
         ('before_stream', 'stream'), ('before_stream_bounded', 'stream_bounded'),
         ('before_stream_2', 'stream_2'), ('before_stream_3', 'stream_3')]


def read(name):
    return json.loads((OUT / name).read_text())


def main():
    v = read('benchmark-final/summary.json')['versions']
    sources = read('benchmark-final/source-manifest.json')
    assert all(hashlib.sha256((ROOT / p).read_bytes()).hexdigest() == h for p, h in sources.items())
    identity = read('benchmark-final/identity.json')
    assert all(hashlib.sha256(Path(cmd[0]).read_bytes()).hexdigest() == identity['binary_sha256'][name]
               for name, cmd in identity['commands'].items())
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
                    differences.append(dict(run=run, profile=b, id=x['id'], fields=changed))
    assert not differences, differences
    checks = read('regression-checks.json')
    assert checks['limits_unchanged']
    assert all(not r['issues'] for r in checks['source_checks'])
    assert all(r['unchanged'] for rows in checks['development'].values() for r in rows)
    assert not read('pdf-regression-checks.json')['changed_documents']
    before = {(r['mode'], r['id']): r for r in read('per-document-before/summary.json')}
    after = {(r['mode'], r['id']): r for r in read('per-document-after/summary.json')}
    assert before.keys() == after.keys()
    assert len(before) == 448
    for name, profile in [('before', 'before'), ('after', 'default')]:
        assert read(f'per-document-{name}/identity.json')['binary_sha256'] == identity['binary_sha256'][profile]
    assert all(all(a.get(k) == before[key].get(k) for k in ['ok', 'status', 'output_sha256', 'error'])
               for key, a in after.items())
    tests = {}
    assert 'Ran 4 tests' in (OUT / 'python-tests.log').read_text()
    assert '\nOK\n' in (OUT / 'python-tests.log').read_text()
    assert 'pass 6' in (OUT / 'node-tests.log').read_text()
    assert 'fail 0' in (OUT / 'node-tests.log').read_text()
    assert 'WASM PDF inspection tests passed' in (OUT / 'wasm-tests.log').read_text()
    assert '27 cases match' in (OUT / 'binding-parity.log').read_text()
    assert '3 PDF inspection cases match' in (OUT / 'binding-parity.log').read_text()
    for name in ['tests-final', 'minimal-tests']:
        results = re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed', (OUT / (name + '.log')).read_text())
        assert results and all(r[0] == 'ok' and r[2] == '0' for r in results)
        tests[name] = sum(int(r[1]) for r in results)
    repetition = {}
    for name in ['before_stream', 'stream']:
        profiles = [v[name + suffix] for suffix in ['', '_2', '_3']]
        peaks = [r['lifetime_peak_rss_bytes'] / 2**20 for r in profiles]
        times = [r['median_corpus_seconds'] for r in profiles]
        repetition[name] = dict(peaks_mib=peaks, median_peak_mib=statistics.median(peaks),
                                corpus_seconds=times, median_seconds=statistics.median(times))
    delta = {}
    labels = ['Structured', 'Structured + recovery', 'Markdown file', 'Markdown file + recovery']
    table = '| Output | Before seconds / 112 | After seconds / 112 | Before RSS MiB | After RSS MiB |\n| --- | ---: | ---: | ---: | ---: |\n'
    for label, (a, b) in zip(labels, PAIRS):
        old, new = v[a], v[b]
        t0, t1 = old['median_corpus_seconds'], new['median_corpus_seconds']
        r0, r1 = old['lifetime_peak_rss_bytes']/2**20, new['lifetime_peak_rss_bytes']/2**20
        if b == 'stream':
            t0, t1 = (repetition[k]['median_seconds'] for k in [a, b])
            r0, r1 = (repetition[k]['median_peak_mib'] for k in [a, b])
        table += f'| {label} | {t0:.3f} | {t1:.3f} | {r0:.1f} | {r1:.1f} |\n'
        delta[b] = dict(time_percent=100*(t1/t0-1), rss_percent=100*(r1/r0-1))
    repeats = '| Independent worker pair | Before seconds | After seconds | Before peak MiB | After peak MiB |\n| --- | ---: | ---: | ---: | ---: |\n'
    a, b = repetition['before_stream'], repetition['stream']
    control = read('baseline-build-control/summary.json')
    assert not control['differences']
    assert control['binary_sha256']['preserved'] == identity['binary_sha256']['before']
    assert control['binary_sha256']['current'] == identity['binary_sha256']['default']
    cv = control['versions']
    control_table = '| Build control (one worker each) | Median seconds / 112 | Peak RSS MiB |\n| --- | ---: | ---: |\n'
    for name, label in [('preserved', 'Preserved baseline executable'), ('rebuilt', 'Fresh build of identical baseline source'), ('current', 'Final parser')]:
        control_table += f"| {label} | {cv[name]['median_seconds']:.3f} | {cv[name]['peak_mib']:.1f} |\n"
    for i in range(3):
        repeats += f"| {i+1} | {a['corpus_seconds'][i]:.3f} | {b['corpus_seconds'][i]:.3f} | {a['peaks_mib'][i]:.1f} | {b['peaks_mib'][i]:.1f} |\n"
    selected = [('stream', 'xls-aed2154ce0e620bd'), ('stream', 'xls-ad544845941ae38f'),
                ('stream', 'xls-9e13fd1af7af5a9f'), ('stream', 'xls-589ace4188fb7ccc'),
                ('stream', 'ods-20f58e9c6ef54e2c'), ('stream', 'xlsx-43eab8ba38301e43'),
                ('stream', 'xlsm-c6ee0b16ff832902'), ('stream_bounded', 'xlsx-3d7484ae52ab377f'),
                ('stream_bounded', 'xls-9e13fd1af7af5a9f'), ('bounded', 'csv-fbeb7b5e9b4f3ed9')]
    peaks = '| Document | Mode | Returned | Before MiB | After MiB |\n| --- | --- | --- | ---: | ---: |\n'
    for key in selected:
        old, new = before[key], after[key]
        peaks += f"| `{key[1]}` | `{key[0]}` | {'yes' if new['ok'] else 'no'} | {old['peak_rss_bytes']/2**20:.1f} | {new['peak_rss_bytes']/2**20:.1f} |\n"
    slow = []
    for oldname, newname in PAIRS:
        old = {r['id']: r for r in v[oldname]['documents']}
        slow.extend(dict(profile=newname, id=r['id'], before_ms=old[r['id']]['median_ms'], after_ms=r['median_ms'])
                    for r in v[newname]['documents'])
    for name, data in [('comparison', dict(profiles=delta, markdown_repetitions=repetition)),
                       ('all-pass-regressions', dict(passes=5, comparisons=PAIRS, documents=112, differences=differences)),
                       ('per-document-timing-comparison', slow)]:
        (OUT / (name + '.json')).write_text(json.dumps(data, indent=2) + '\n')
    body = f'''# Markdown memory follow-up — November 12, 2026

This compares the previous speed-optimized build with the final allocation changes. It uses the same 112 public documents, original limits and OCR-disabled configuration. No document-specific rules, worker recycling, allocator changes or benchmark-output shortcuts were introduced.

## Matched results

{table}
Each worker runs one excluded full-corpus warm-up followed by five sequential full-corpus passes. Times are medians of the five corpus totals. Before/after order alternates. Only one pair is resident at a time, reducing the benchmark's background memory load. Non-recovery Markdown is repeated with three independent worker pairs; its row reports the median of the three timing medians and three lifetime RSS peaks. Other rows are one worker pair each. RSS values are measured process high-water marks, not memory bounds.

Hardware: Apple M3 Max, 36 GiB RAM, 14 logical CPUs, macOS, on AC power. Builds and other verification jobs did not overlap timed passes. Parsing, input reads and Markdown rendering are timed. File-output modes also time temporary staging and output-file writes. Verification file readback and JSON serialization are excluded from time but included in process memory. Compare each output mode with itself; their contracts differ.

## Repeated non-recovery Markdown measurements

{repeats}
The repeated median peak is **{a['median_peak_mib']:.1f} → {b['median_peak_mib']:.1f} MiB**; the median corpus time is **{a['median_seconds']:.3f} → {b['median_seconds']:.3f} seconds** ({delta['stream']['time_percent']:+.1f}%). The peak ranges are **{min(a['peaks_mib']):.1f}–{max(a['peaks_mib']):.1f} MiB** before and **{min(b['peaks_mib']):.1f}–{max(b['peaks_mib']):.1f} MiB** after. Three workers provide a variability check, not a statistical guarantee.

The earlier report's 141.0 → 165.7 MiB increase was a comparison of single worker-lifetime peaks. It did not reproduce consistently: preliminary post-response RSS traces for the unchanged optimized build and the earlier slower build overlapped and reversed their ordering. Those traces are diagnostic, not peak measurements. Consequently, the exact historic difference cannot be attributed to a persistent leak. Temporary allocation size and allocator/OS retention both affect RSS; the isolated measurements below establish the concrete savings.

The battery-affected diagnostic run was interrupted and excluded. Exact-size compound-stream and record-index reservations lowered some isolated peaks but increased the persistent Markdown worker's median RSS in repeated candidate runs, so they were removed. A ZIP-member reservation candidate was also removed; the EPUB slowdown persisted without it, so it was not a confirmed cause. The final build separately avoids temporary per-byte strings when creating EPUB anchors. Candidate runs and their snapshots remain available for auditing; only `benchmark-final` and `per-document-after` describe the final code. Historical build timings in previous reports are not directly comparable with this rerun's paired measurements.

### Baseline build control

The preserved baseline source was rebuilt with the current toolchain and the same release/example command. Its complete source manifest was checked against the frozen snapshot. A separate diagnostic runs the preserved executable, rebuilt baseline and final parser through one warm-up and five sequential passes each, rotating their order. Outputs, statuses, diagnostics and errors match on all 112 inputs.

{control_table}
This is one worker per executable and does not replace the three-pair result above. The rebuilt baseline and final parser are close in this control; the preserved executable's peak differs even though both baseline builds have identical source. This does not separate build layout from runtime/allocator variation, but it argues against treating the historic single-peak difference as proof of a source-level leak. Raw passes and binary hashes are in `baseline-build-control`.

## What changed

- A bounded BIFF header preflight checks the existing node budget before allocating the record index. Oversized XLS files can fail without building an index that is immediately discarded. Successful and recovered indexes and compound-document input streams retain their established allocation strategy, since exact-size reservations increased worker-level RSS in candidate measurements. Truncation, zero padding, cancellation checks, malformed-input error precedence and completed-sheet recovery boundaries are retained.
- The plain-text table preflight also checks the bounding box of merged grids before creating cell paragraphs. This preserves paragraph-budget and malformed-merge error precedence. It protects overlarge merged extents generally; the targeted sample did not show a peak-memory reduction from this guard alone. Aggregate cell/node budgets and merge validation still apply during table construction.
- EPUB anchors append hexadecimal characters directly instead of allocating a temporary formatted string for every UTF-8 byte. IDs, link destinations and Unicode handling are unchanged. This targets unnecessary allocation work in the format that repeatedly slowed down in the candidate comparisons; it does not establish the cause of the historic memory difference.
- Native worksheet XML streaming, shared-string storage, row buffering and Markdown recovery streaming remain enabled. Limits and output semantics are unchanged.

## Fresh-process document peaks

All 112 documents were measured separately in each of the four modes, before and after: 896 process measurements. These include parser input, Markdown and verification serialization, with no previous document retained in the worker. Selected large legacy inputs and previously improved spreadsheet/CSV cases are shown below; all rows are available in the JSON files.

{peaks}
Rejected documents remain in the scoring and timing set. Lower memory when rejecting a document does not imply successful extraction or better quality. Each isolated measurement is one process and may also vary slightly.

## Regression checks and limitations

- All 112 outputs, statuses, diagnostics, rendering diagnostics, error codes and error messages match across all five measured passes and all six before/after worker pairs.
- 81 Office fixtures match in all four output modes; 200 PDFs match in structured and Markdown output.
- Independent source checks validate retained spreadsheet cells and CSV records; raw counts and scope are in `regression-checks.json`. Existing resource limits match byte-for-byte.
- {tests['tests-final']} full-feature Rust test executions and {tests['minimal-tests']} minimal-feature tests passed. New tests cover BIFF budget preservation, zero padding, malformed/limit precedence, recovery truncation, merged-grid bounds and Unicode anchor IDs. Six Node tests, four Python tests, WASM smoke/inspection checks and 27 parsing plus three inspection cross-binding comparisons passed. Binding test logs are retained beside this report.
- Default parsing still returns 97/112; recovery returns 112/112 including 15 resource-limited prefixes. Only 11 results have complete status. No new quality score is claimed, and the eight-converter blind quality review remains unfinished.
- Input buffers, shared metadata and some worksheet models remain in memory. These improvements do not make every format constant-memory or establish Linux production memory use. Individual timings and peaks can increase even when aggregate results improve; all per-document timing comparisons are retained.

## Reproduce

```sh
cargo test --offline -p bulkparse -p bulkparse-cli -p bulkparse-pool
cargo build --offline --release -p bulkparse --example office_bench
python3 scripts/benchmark_markdown_memory.py --out /tmp/bulkparse-markdown-benchmark
python3 scripts/profile_parser_memory.py --binary reports/markdown-memory-2026-11-12/before-office-bench --out /tmp/bulkparse-memory-before --modes default bounded stream stream_bounded
python3 scripts/profile_parser_memory.py --binary target/release/examples/office_bench --out /tmp/bulkparse-memory-after --modes default bounded stream stream_bounded
cargo build --offline --locked --release --manifest-path reports/markdown-memory-2026-11-12/before/source-snapshot/Cargo.toml --target-dir target/markdown-memory-baseline -p bulkparse --example office_bench
python3 scripts/check_markdown_baseline_build.py --out /tmp/bulkparse-build-control
```

Use fresh output directories. The preserved before executable is for this macOS machine; rebuild from `before/source-snapshot` on other platforms. `benchmark-final` contains source snapshots, input and executable hashes, per-pass results and predictions. `per-document-before` and `per-document-after` contain isolated RSS measurements and output hashes. `comparison.json` contains computed deltas; `all-pass-regressions.json` and the Office/PDF check files contain verification results.
'''
    (OUT / 'README.md').write_text(body)
    section = f'''<!-- realworld-benchmark:start -->
### Real-world large-document handling

**112 public documents across 13 formats.** Native spreadsheet parsing streams large worksheet XML and shared strings. Markdown recovery streams preview rows; legacy spreadsheet parsing checks record budgets before allocating indexes that would be discarded. EPUB anchor generation also avoids temporary strings for each encoded byte.

{table}
Matched workloads on an Apple M3 Max, OCR disabled, on AC power: one excluded warm-up and five alternating sequential full-corpus passes. Markdown-file values are medians across three independent worker pairs; other modes use one pair. Peak RSS is a measured process high-water mark, not a memory guarantee. The full report shows repeat ranges, isolated document peaks and timing tradeoffs. The previously reported Markdown memory increase did not reproduce consistently; these results do not establish a persistent leak or a universal speedup.

Default parsing returns **97/112**; explicit recovery returns **112/112**, including **15 partial prefixes**. Only 11 results have complete status. All 112 outputs, statuses and diagnostics remain unchanged; 81 Office fixtures and 200 PDFs also pass regression checks. This corpus is regression evidence. The broader eight-converter blind quality review remains unfinished; no quality leaderboard is claimed.

[Measurements, causes and limitations](reports/markdown-memory-2026-11-12/README.md) · [Streaming API](docs/streaming-markdown.md) · [Corpus and study status](benchmarks/realworld/README.md)
<!-- realworld-benchmark:end -->'''
    path = ROOT / 'README.md'
    text = path.read_text()
    start, end = text.index('<!-- realworld-benchmark:start -->'), text.index('<!-- realworld-benchmark:end -->') + len('<!-- realworld-benchmark:end -->')
    path.write_text(text[:start] + section + text[end:])
    print(json.dumps(dict(profiles=delta, markdown_repetitions=repetition), indent=2))


if __name__ == '__main__':
    main()
