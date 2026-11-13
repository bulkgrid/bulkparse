#!/usr/bin/env python3
"""Verify and report the worksheet XML / shared-string streaming measurements."""
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'reports/worksheet-streaming-2026-11-12'


def read(name):
    return json.loads((OUT / name).read_text())


def main():
    summary = read('benchmark-final/summary.json')
    versions = summary['versions']
    checks = read('regression-checks.json')
    pdf = read('pdf-regression-checks.json')
    assert checks['limits_unchanged'] and not pdf['changed_documents']
    assert all(not row['issues'] for row in checks['source_checks'])
    assert all(row['unchanged'] for rows in checks['development'].values() for row in rows)
    pairs = [('before', 'default'), ('before_bounded', 'bounded'),
             ('before_stream', 'stream'), ('before_stream_bounded', 'stream_bounded')]
    fields = ['ok', 'status', 'output_sha256', 'diagnostics', 'render_diagnostics', 'error_code']
    differences = []
    for run in range(1, 6):
        for previous, current in pairs:
            a = read(f'benchmark-final/{previous}-pass-{run}.json')['documents']
            b = read(f'benchmark-final/{current}-pass-{run}.json')['documents']
            assert len(a) == len(b) == 112
            for x, y in zip(a, b):
                assert x['id'] == y['id']
                changed = [k for k in fields if x.get(k) != y.get(k)]
                if changed:
                    differences.append(dict(run=run, profile=current, id=x['id'], fields=changed))
    assert not differences, differences
    (OUT / 'all-pass-regressions.json').write_text(json.dumps({
        'measured_passes': 5, 'profile_comparisons': pairs,
        'documents_per_pass': 112, 'changed': differences,
    }, indent=2) + '\n')
    before = {(r['mode'], r['id']): r for r in read('per-document-before/summary.json')}
    after = {(r['mode'], r['id']): r for r in read('per-document-after/summary.json')}
    assert before.keys() == after.keys()
    for key, a in before.items():
        b = after[key]
        assert all(a[k] == b[k] for k in ['ok', 'status', 'output_sha256'])

    labels = {'before': 'Before: structured', 'default': 'After: structured',
              'before_bounded': 'Before: structured + recovery', 'bounded': 'After: structured + recovery',
              'before_stream': 'Before: Markdown file', 'stream': 'After: Markdown file',
              'before_stream_bounded': 'Before: Markdown file + recovery', 'stream_bounded': 'After: Markdown file + recovery'}
    table = '| Profile | Returned / 112 | Median seconds / 112 attempts | Peak worker RSS MiB |\n| --- | ---: | ---: | ---: |\n'
    for previous, current in pairs:
        for key in [previous, current]:
            r = versions[key]
            table += f"| {labels[key]} | {r['returned_all_passes']}/112 | {r['median_corpus_seconds']:.3f} | {r['lifetime_peak_rss_bytes']/2**20:.1f} |\n"
    deltas = {b: {
        'corpus_time_change_percent': 100 * (versions[b]['median_corpus_seconds'] / versions[a]['median_corpus_seconds'] - 1),
        'peak_rss_change_percent': 100 * (versions[b]['lifetime_peak_rss_bytes'] / versions[a]['lifetime_peak_rss_bytes'] - 1),
    } for a, b in pairs}
    (OUT / 'comparison.json').write_text(json.dumps(deltas, indent=2) + '\n')
    examples = [('stream', 'xlsm-c6ee0b16ff832902'), ('stream', 'ods-20f58e9c6ef54e2c'),
                ('stream', 'xlsx-43eab8ba38301e43'), ('stream_bounded', 'xlsm-1c382d45c18005cd'),
                ('stream_bounded', 'xlsx-3d7484ae52ab377f'), ('bounded', 'csv-fbeb7b5e9b4f3ed9')]
    peaks = '| Source document | Mode | Before MiB | After MiB |\n| --- | --- | ---: | ---: |\n'
    for mode, doc in examples:
        a, b = before[mode, doc], after[mode, doc]
        peaks += f"| {a['title']} (`{doc}`) | {mode} | {a['peak_rss_bytes']/2**20:.1f} | {b['peak_rss_bytes']/2**20:.1f} |\n"
    tests = {}
    for name in ['tests-final', 'minimal-tests']:
        rows = re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed', (OUT / (name + '.log')).read_text())
        assert rows and all(r[0] == 'ok' and r[2] == '0' for r in rows)
        tests[name] = sum(int(r[1]) for r in rows)
    body = f'''# Worksheet XML and shared-string streaming — November 12, 2026

Native XLSX/XLSM and ODS parsing now processes large worksheet XML parts as validated row fragments. Large XLSX shared-string tables use indexed temporary storage with a bounded rich-text cache. The previous Markdown writer already released completed sheets and CSV records; this change reduces the XML and shared-string allocations within a single sheet. Both structured and Markdown-file APIs benefit.

**The results are mixed:** several large spreadsheets use substantially less memory, but corpus time increased, and recovery-enabled persistent workers recorded higher peak RSS. Output and diagnostic checks found no regressions. This is not a blanket performance improvement.

## Matched before/after comparison

{table}
Structured default parsing took **{deltas['default']['corpus_time_change_percent']:.1f}% longer** with **{-deltas['default']['peak_rss_change_percent']:.1f}% lower** peak worker RSS. Markdown-file default parsing took **{deltas['stream']['corpus_time_change_percent']:.1f}% longer** with **{-deltas['stream']['peak_rss_change_percent']:.1f}% lower** peak RSS. With recovery enabled, structured and Markdown-file RSS increased by **{deltas['bounded']['peak_rss_change_percent']:.1f}%** and **{deltas['stream_bounded']['peak_rss_change_percent']:.1f}%**, respectively; time increased by **{deltas['bounded']['corpus_time_change_percent']:.1f}%** and **{deltas['stream_bounded']['corpus_time_change_percent']:.1f}%**. The recovery memory increase remains unresolved; lower allocations on individual spreadsheets do not guarantee lower process-lifetime RSS.

Each comparison uses the same output contract before and after. The structured API returns a model and Markdown string; the file API stages and writes Markdown. File writes and temporary spooling are timed. Verification readback and protocol serialization are excluded from elapsed time but included in lifetime RSS.

The frozen corpus contains 112 public documents across 13 formats. OCR is disabled. Each of eight profiles uses one persistent process, parsing documents sequentially. One full warm-up is excluded, followed by five full passes with rotating profile order; the reported time is the median corpus total. RSS is the OS high-water mark over the worker lifetime, including warm-up. The machine is an Apple M3 Max with 36 GiB RAM and 14 logical CPUs, running macOS. No builds or other test runs overlapped the timed passes. Corpus, source and binary hashes were verified. These measurements do not establish Linux production usage or a hard memory bound.

## Per-document memory

Each measurement starts a fresh process. These are single runs and include output serialization; they are separate from the five-pass persistent-worker measurements. Examples include gains, unchanged cases and a regression.

{peaks}
The successful spreadsheets retain their existing partial/fidelity status. Recovery cases still return bounded prefixes. The CSV recovery result also recorded a higher peak in this run, even though the new XML extraction path does not process CSV. These measurements establish the observed peaks, not their allocation-level cause. Repeated workload and allocator profiling is still needed before claiming recovery memory is improved.

[All before profiles](per-document-before/summary.json) · [All after profiles](per-document-after/summary.json)

## Implementation and boundaries

- XML parts above 256 KiB are scanned from the ZIP archive, checked against the existing node/depth budgets, then partitioned into row fragments. A small metadata projection and indexed spool replace a complete worksheet XML string and tree. The spool spills after 64 KiB. An individual large row/event still needs memory.
- Large XLSX shared-string tables are validated and mapped directly to stored rich-text runs; a roughly 256 KiB lookup cache avoids retaining the entire table or a duplicate plain-text table. Phonetic exclusions, explicit font flags and rich-text semantics are preserved.
- Namespace scope, merge metadata, hidden rows/columns, hyperlinks, repeated ODS cells/rows, headers, number/date formats and diagnostics remain supported. General structural fallbacks use the original tree path for unusual row-contained global metadata. No corpus names or document-specific rules were introduced.
- Small parts retain the original tree path. Native structured and writer APIs share the improvement; WASM retains its in-memory XML implementation. Input bytes, styles, merge/layout data and the worksheet cell model are still buffered. Other formats and bounded recovery keep their existing implementations. End-to-end constant-memory parsing is not implemented.
- The parser limits are unchanged. Temporary-disk access and fragment validation add work. File output remains transactional and refuses overwrite. See the [API and failure contract](../../docs/streaming-markdown.md).

## Regression evidence

- All five measured passes preserve Markdown hashes, success/failure, status, parser/render diagnostics and error codes for all 112 documents across four matched before/after profiles.
- All 81 Office fixtures match in four current profiles. All 200 opendataloader-bench PDFs preserve Markdown, statuses and diagnostics. No new PDF quality score was calculated.
- Independent source checks verified all 190,145 retained XLSX/XLSM/ODS cells and 5,555 complete CSV records. Four recovered XLS inputs still lack an independent per-cell audit.
- {tests['tests-final']} full-feature Rust test executions and {tests['minimal-tests']} minimal-feature executions passed. Six Node tests, four Python tests, WASM smoke/inspection tests and package installation tests passed. Binding parity covers 27 parsing cases and three PDF inspections across Node, Python, WASM and CLI.
- New fragment tests cover exact XML node accounting, namespace scope, invalid late XML and structural fallback. Integration tests compare streamed and small-tree paths on rich strings, phonetics, styles, links, merges and repeated ODS rows/headers across sheets. CLI checks verify the renamed command, file/stdin/stdout equality and no overwrite; the OCR-enabled CLI also builds.

Normal parsing still returns 97/112 documents; explicit recovery returns 112/112, including 15 resource-limited prefixes. Only 11 results have complete status; recovery has 101 partial results. This corpus has informed development and is regression evidence. The broader eight-converter blind quality review remains unfinished; this report supplies no quality leaderboard.

[All-pass checks](all-pass-regressions.json) · [Office/source checks](regression-checks.json) · [PDF checks](pdf-regression-checks.json) · [CLI checks](cli-checks.json) · [Timing summary](benchmark-final/summary.json)

## API names

Unreleased APIs were renamed directly, without compatibility aliases. Node exposes `parse(bytes)`, `parseFile(path)` and `parseFileToMarkdown(inputPath, outputPath)`. The first two return the structured result and Markdown; the last writes a file and returns metadata. Python exposes `parse` / `parse_file`, WASM exposes `parse`, and the CLI uses `bulkparse parse`. Rust exposes `parse` / `parse_json` and native `Parser::parse_file_to_markdown`, `parse_to_markdown`, `parse_reader_to_markdown` and `parse_to_markdown_with_cancel`.

```js
const {{parseFileToMarkdown}} = require('@bulkgrid/bulkparse');
const summary = await parseFileToMarkdown('workbook.xlsx', 'workbook.md');
```

## Reproduce

Keep the recorded corpus, baseline binary and before-source snapshot. Use fresh output directories; benchmark/profile runners refuse to overwrite results.

```sh
cargo build --offline --release -p bulkparse --example office_bench
python3 scripts/benchmark_worksheet_streaming.py --out reports/worksheet-recheck/benchmark-final
python3 scripts/profile_parser_memory.py --binary reports/worksheet-streaming-2026-11-12/before-office-bench --out reports/worksheet-recheck/per-document-before --modes default bounded stream stream_bounded
python3 scripts/profile_parser_memory.py --binary target/release/examples/office_bench --out reports/worksheet-recheck/per-document-after --modes default bounded stream stream_bounded
```

Validation for this recorded report root:

```sh
target/competitors/docling-venv/bin/python scripts/validate_large_documents.py --out reports/worksheet-streaming-2026-11-12
BULKPARSE_CHECK_REPORT=reports/worksheet-streaming-2026-11-12 python3 scripts/check_streaming_pdf.py
python3 scripts/report_worksheet_streaming.py
```

The source validator uses BeautifulSoup and markdown-it-py, not Docling conversion. For another report root, copy the preserved `before/source-snapshot` beside `benchmark-final` before validation. PDF validation expects `before-office-bench` in that root. `/usr/bin/time -l` is macOS-specific and may require permission to inspect process memory. Earlier prototype profiles and speed probes are retained separately and are not the reported final measurements.

[Configuration and binary hashes](benchmark-final/identity.json) · [Source snapshot](benchmark-final/source-snapshot/) · [Before snapshot](before/source-snapshot/)
'''
    (OUT / 'README.md').write_text(body)
    concise = '| Markdown file output | Returned / 112 | Median seconds / 112 attempts | Peak worker RSS MiB |\n| --- | ---: | ---: | ---: |\n'
    for key in ['before_stream', 'stream', 'before_stream_bounded', 'stream_bounded']:
        r = versions[key]
        concise += f"| {labels[key]} | {r['returned_all_passes']}/112 | {r['median_corpus_seconds']:.3f} | {r['lifetime_peak_rss_bytes']/2**20:.1f} |\n"
    section = f'''<!-- realworld-benchmark:start -->
### Real-world large-document handling

**112 public documents across 13 formats exposed limits the smaller fixtures missed.** Earlier encoding and legacy-format fixes increased returned outputs from 85 to 97. Explicit bounded recovery returns partial data from the other 15. The latest native spreadsheet changes stream large worksheet XML and shared strings, complementing CSV-record and completed-sheet output streaming without raising limits.

{concise}
These are matched before/after file-output workloads on the same Apple M3 Max: one excluded warm-up and five rotating sequential full-corpus passes, OCR disabled. Default peak RSS fell, but parsing took longer and recovery-enabled peak RSS increased. Individual spreadsheet savings do not imply a lower memory peak for every workload. The report includes all eight structured/file profiles and the unresolved performance tradeoffs.

**The 15 resource-limit recoveries are partial.** Only 11 results have complete status. All 112 documents preserve output, statuses and diagnostics; 81 Office fixtures and 200 PDFs also show no output regressions. This corpus is now regression evidence. The broader eight-converter blind quality review remains unfinished; no quality leaderboard is claimed.

[Measurements and limitations](reports/worksheet-streaming-2026-11-12/README.md) · [Streaming API](docs/streaming-markdown.md) · [Corpus and study status](benchmarks/realworld/README.md)
<!-- realworld-benchmark:end -->'''
    path = ROOT / 'README.md'
    text = path.read_text()
    start = text.index('<!-- realworld-benchmark:start -->')
    end = text.index('<!-- realworld-benchmark:end -->', start) + len('<!-- realworld-benchmark:end -->')
    path.write_text(text[:start] + section + text[end:])
    print('Verified all passes and wrote the report and README benchmark section.')


if __name__ == '__main__':
    main()
