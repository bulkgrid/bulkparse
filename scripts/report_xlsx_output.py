#!/usr/bin/env python3
"""Verify and publish XLSX row-output streaming measurements."""
import hashlib
import json
from pathlib import Path
import re
import statistics

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'reports/xlsx-output-streaming-2026-11-13'
PAIRS = [('before', 'default'), ('before_bounded', 'bounded'),
         ('before_stream', 'stream'), ('before_stream_bounded', 'stream_bounded'),
         ('before_stream_2', 'stream_2'), ('before_stream_3', 'stream_3'),
         ('before_2', 'default_2'), ('before_3', 'default_3')]


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
    for name in ['before_stream', 'stream', 'before', 'default']:
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
        if new == 'default':
            x, y = repetition['before'], repetition['default']
            t0, t1, r0, r1 = x['median_seconds'], y['median_seconds'], x['median_peak_mib'], y['median_peak_mib']
        table += f'| {label} | {t0:.3f} | {t1:.3f} | {r0:.1f} | {r1:.1f} |\n'
        delta[new] = dict(time_percent=100*(t1/t0-1), rss_percent=100*(r1/r0-1))
    repeats = '| Output / independent pair | Before seconds | After seconds | Before peak MiB | After peak MiB |\n| --- | ---: | ---: | ---: | ---: |\n'
    for label, old, new in [('Markdown', 'before_stream', 'stream'), ('Structured', 'before', 'default')]:
        x, y = repetition[old], repetition[new]
        for i in range(3):
            repeats += f"| {label} {i+1} | {x['corpus_seconds'][i]:.3f} | {y['corpus_seconds'][i]:.3f} | {x['peaks_mib'][i]:.1f} | {y['peaks_mib'][i]:.1f} |\n"
    save('comparison.json', dict(profiles=delta, repetitions=repetition))
    timings = []
    for old, new in PAIRS:
        reference = {r['id']: r for r in v[old]['documents']}
        timings.extend(dict(id=r['id'], profile=new, before_ms=reference[r['id']]['median_ms'], after_ms=r['median_ms']) for r in v[new]['documents'])
    save('per-document-timing-comparison.json', timings)
    stress = read('stress-benchmark/summary.json')
    assert stress['binary_sha256'] == {'before': identity['binary_sha256']['before'], 'after': identity['binary_sha256']['default']}
    assert not stress['output_differences']
    assert read('stress-manifest.json') == read('stress-manifest.json.initial')
    assert all(sha(ROOT / d['path']) == d['sha256'] for d in read('stress-manifest.json'))
    stress_table = '| Generated XLSX | Cells | Before ms | After ms | Time change | Before peak MiB | After peak MiB | Memory change |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n'
    stress_comparison = []
    for kind, cells in [('plain', 24000), ('rich', 12000), ('merged', 12000)]:
        medians = {}
        for name in ['before', 'after']:
            rows = [r for r in stress['results'] if r['id'] == kind and r['version'] == name]
            assert len(rows) == 3
            medians[name] = dict(ms=statistics.median(r['median_ms'] for r in rows), rss=statistics.median(r['peak_rss_bytes']/2**20 for r in rows))
        x, y = medians['before'], medians['after']
        dt, dr = 100*(y['ms']/x['ms']-1), 100*(y['rss']/x['rss']-1)
        stress_table += f'| {kind} | {cells:,} | {x["ms"]:.2f} | {y["ms"]:.2f} | {dt:+.1f}% | {x["rss"]:.2f} | {y["rss"]:.2f} | {dr:+.1f}% |\n'
        stress_comparison.append(dict(kind=kind, **medians, time_percent=dt, rss_percent=dr))
        for stress_repeat in range(3):
            passes = read(f'stress-benchmark/{kind}-{stress_repeat}-passes.json')
            assert len(passes['before']) == len(passes['after']) == 6
            for x, y in zip(passes['before'], passes['after']):
                assert x['ok'] and all(x.get(k) == y.get(k) for k in fields)
    save('stress-comparison.json', stress_comparison)
    structured = read('structured-json-checks.json')
    assert structured['documents'] == 115 and len(structured['rows']) == 230
    assert not structured['differences'] and all(r['unchanged'] for r in structured['rows'])
    assert structured['binary_sha256'] == {'before': sha(OUT/'before-bulkparse'), 'after': sha(ROOT/'target/release/bulkparse')}
    peaks = '| Document | Mode | Returned | Before peak MiB | After peak MiB |\n| --- | --- | --- | ---: | ---: |\n'
    selected = [('stream', 'xlsx-43eab8ba38301e43'), ('stream', 'xlsm-1c382d45c18005cd'),
                ('stream', 'xlsm-c6ee0b16ff832902'), ('stream_bounded', 'xlsx-3d7484ae52ab377f'),
                ('stream', 'ods-20f58e9c6ef54e2c'), ('stream', 'xls-aed2154ce0e620bd'),
                ('bounded', 'csv-fbeb7b5e9b4f3ed9')]
    for key in selected:
        x, y = before[key], after[key]
        peaks += f'| `{key[1]}` | `{key[0]}` | {"yes" if y["ok"] else "no"} | {x["peak_rss_bytes"]/2**20:.1f} | {y["peak_rss_bytes"]/2**20:.1f} |\n'
    charge = [re.search(r'(\d+)%;', (OUT / f'power-{name}.txt').read_text())[1] for name in ['before', 'after']]
    spreadsheet_cells = sum(r['checked'] for r in checks['source_checks'] if r['document_id'].split('-')[0] in ['xlsx','xlsm','ods'])
    csv_rows = sum(r['checked'] for r in checks['source_checks'] if r['document_id'].startswith('csv-'))
    body = f'''# XLSX table-output streaming — November 13, 2026

XLSX/XLSM Markdown output now releases final table rows as it writes them, including rich text, hyperlinks and merged-cell content. Previously it retained the complete final table while rendering. The existing worksheet XML, shared-string and recovery streaming remains enabled.

## Real-world benchmark

{table}
For repeated non-recovery Markdown workers, time changed **{delta['stream']['time_percent']:+.2f}%** and peak RSS changed **{delta['stream']['rss_percent']:+.2f}%**. These are whole-worker results, including documents from other formats; the generated XLSX tests below isolate the changed path. Structured output's repeated median time increased **{delta['default']['time_percent']:.2f}%** and peak RSS increased **{delta['default']['rss_percent']:.2f}%**. Recovery's structured-worker peak also increased in its single pair. Output equivalence does not mean performance is unchanged in every profile.

112 public documents across 13 formats, OCR disabled. Each persistent worker processes documents sequentially: one excluded full-corpus warm-up and five measured full-corpus passes, alternating before/after order. One pair is resident at a time. Structured and Markdown without recovery each use three independent pairs; their rows are medians of three time medians and three lifetime RSS peaks. Recovery profiles have one pair, so their memory differences are less robust. Rejected inputs remain in the workload.

Hardware: Apple M3 Max, 36 GiB RAM, 14 logical CPUs, macOS; standard battery power ({charge[0]}% before, {charge[1]}% after). No builds or verification jobs overlapped the final timing run. An initial run started before binding verification reported completion and was excluded in full, retained with `.excluded` suffixes.

Timing includes reading, parsing and rendering; the Markdown writer includes staging and file writes. Verification readback and response serialization are excluded from time but included in lifetime RSS. Input, binary and source hashes are retained. No allocator changes or worker recycling were introduced.

### Independent worker pairs without recovery

{repeats}
Markdown peaks before the change ranged {min(a['peaks_mib']):.1f}–{max(a['peaks_mib']):.1f} MiB; after peaks ranged {min(b['peaks_mib']):.1f}–{max(b['peaks_mib']):.1f} MiB. These measurements do not establish a universal memory bound or Linux production performance. Small timing differences can include system noise; three repetitions are not a confidence interval.

## Generated large-sheet tests

{stress_table}
These deterministic fixtures are separate from the real-world corpus and quality scoring. `plain` has 3,000 rows × 8 cells; `rich` and `merged` have 1,500 × 8. Rich fixtures include multiple inline runs, Unicode, escaping and internal hyperlinks; merged fixtures add horizontal merges with covered-cell content. Each fixture uses three independent before/after worker pairs, each with one warm-up and five measured sequential conversions. Table values are medians across pairs. All outputs, statuses, diagnostics and errors match on every pass.

Memory decreases in these large-sheet cases, with a small timing cost shown explicitly above. This change is a memory improvement, not a claim that every spreadsheet is faster. The first implementation also repeated cell-budget checks; that duplicate pass was removed before the final measurements. Its exploratory results are retained under `stress-benchmark.initial` and are excluded from the table.

## Implementation and semantics

- The shared table builder accepts an inline-decoration callback. XLSX applies rich runs and hyperlinks to each completed row immediately before rendering, then releases it. Structured output still constructs and returns the full table.
- Decoration uses each paragraph's original source coordinates, including content moved into a merged anchor. The first matching hyperlink still wins; rich-run boundaries, local links, warnings, hidden-row behavior and HTML/GFM table selection are preserved.
- All remaining cell/table-node budgets are charged before rows or queued sheet headings are emitted. The row loop checks cancellation without charging those budgets again. Structured parsing retains its original error order; streaming and recovery preserve the same limit outcomes.
- Structured output releases table-layout maps before decorating rich text and links, preserving the previous allocation lifetime. An intermediate candidate retained those maps longer and showed a higher structured-worker RSS; its measurements are archived in `candidate-with-structured-overlap` and excluded from the final table.
- The existing staging spool discards failed attempts and protects caller output on parsing or strict-mode errors. A later hyperlink text-limit failure cannot publish an unfinished table.
- No document names, identifiers, special corpus rules or higher limits were added. Worksheet XML/shared-string storage and their bounded buffering remain unchanged.

The parser still retains input bytes, styles, merge metadata and a worksheet's sparse cell/block map. It now avoids retaining a second complete final row/cell model for XLSX Markdown output. This is not end-to-end constant-memory parsing; structured APIs intentionally retain the returned document.

## Isolated document peaks

All 112 inputs were also measured in fresh processes in four modes, before and after (896 measurements). These single-process peaks include transport and can vary; they are not repeated estimates.

{peaks}
Full per-document timings and peak measurements are retained alongside the report. Reduced memory on a rejected document does not imply successful extraction.

## Regression checks and limits

- All 112 outputs, statuses, diagnostics, render diagnostics, error codes and messages match across all five measured passes and eight worker pairs.
- Full structured CLI JSON, stderr and exit codes match byte-for-byte for 112 real-world and three generated documents, with recovery off and on: 230 comparisons.
- 81 Office fixtures match in all four modes; all 200 PDF outputs and diagnostics match. This is a PDF regression check, not new NID/TEDS/MHS scoring.
- Independent source checks verify {spreadsheet_cells:,} retained spreadsheet cells and {csv_rows:,} complete CSV records. Default limits are byte-for-byte unchanged.
- {tests['tests-final']} full-feature Rust test executions and {tests['minimal-tests']} minimal-feature tests pass. New XLSX coverage checks rich and merged cells, hyperlink precedence, source coordinates, hidden rows, warnings, atomic output and combined node/cell/text-limit sweeps with recovery on/off.
- Six Node tests, four Python tests, WASM smoke/inspection tests, 27 parsing comparisons and three PDF-inspection comparisons across bindings pass.
- Default parsing still returns 97/112. Explicit recovery returns 112/112, including 15 resource-limited prefixes; only 11 results have complete status. These remaining limitations are unchanged. This corpus is now development/regression evidence. The eight-converter blind quality review remains unfinished; no quality leaderboard is claimed.

## Reproduce

```sh
cargo test --offline -p bulkparse -p bulkparse-cli -p bulkparse-pool
cargo build --offline --release -p bulkparse --example office_bench
cargo build --offline --release -p bulkparse-cli
python3 scripts/benchmark_xlsx_output.py --out /tmp/bulkparse-xlsx-benchmark
python3 scripts/profile_parser_memory.py --binary reports/xlsx-output-streaming-2026-11-13/before-office-bench --out /tmp/bulkparse-xlsx-before --modes default bounded stream stream_bounded
python3 scripts/profile_parser_memory.py --binary target/release/examples/office_bench --out /tmp/bulkparse-xlsx-after --modes default bounded stream stream_bounded
python3 scripts/check_xlsx_output_json.py
```

Use fresh output directories and preserve the before binaries. They are built for this Mac; `before/source-snapshot` provides the baseline sources for another machine. `benchmark-final` contains final source snapshots, hashes, commands, predictions and all passes. To regenerate the stress fixtures and measurements, archive `stress-inputs`, `stress-manifest.json` and `stress-benchmark` first, then run `python3 scripts/benchmark_xlsx_output_stress.py`; never overwrite evidence. Run `python3 scripts/report_xlsx_output.py` to revalidate the saved evidence and regenerate this report and the README section.
'''
    (OUT / 'README.md').write_text(body)
    section = f'''<!-- realworld-benchmark:start -->
### Real-world large-document handling

**112 public documents across 13 formats.** Spreadsheet parsing streams large worksheet XML and shared strings. XLSX/XLSM, XLS/XLSB and native ODS Markdown output releases final table rows as it writes them.

{table}
Before/after comparison for XLSX table-output streaming. One warm-up and five alternating sequential full-corpus passes per worker; structured and Markdown-file figures without recovery are medians across three independent worker pairs. Recovery rows use one pair. Same Apple M3 Max, standard battery power, OCR disabled. Peak RSS includes the benchmark worker and is not a memory guarantee.

Separate generated large-sheet tests show lower peak memory with a small per-sheet timing cost; see the report for exact changes and repeat data. Outputs and diagnostics remain unchanged across the corpus, 81 Office fixtures and 200 PDFs; full structured JSON also matches in 230 comparisons.

Default parsing returns **97/112**; explicit recovery returns **112/112**, including **15 partial prefixes**. Only 11 results have complete status. This corpus is regression evidence. The broader eight-converter blind quality review remains unfinished; no quality leaderboard is claimed.

[Measurements, implementation and limitations](reports/xlsx-output-streaming-2026-11-13/README.md) · [Streaming API](docs/streaming-markdown.md) · [Corpus and study status](benchmarks/realworld/README.md)
<!-- realworld-benchmark:end -->'''
    path = ROOT / 'README.md'
    text = path.read_text()
    start = text.index('<!-- realworld-benchmark:start -->')
    end = text.index('<!-- realworld-benchmark:end -->') + len('<!-- realworld-benchmark:end -->')
    path.write_text(text[:start] + section + text[end:])
    print(json.dumps(dict(profiles=delta, repetitions=repetition, stress=stress_comparison, tests=tests), indent=2))


if __name__ == '__main__':
    main()
