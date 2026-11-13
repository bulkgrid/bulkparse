#!/usr/bin/env python3
"""Publish the measured parser-fix results without inventing quality scores."""
import collections
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'reports/realworld-fixes-2026-11-12'


def main():
    s = json.loads((OUT / 'benchmark-final/summary.json').read_text())
    checks = json.loads((OUT / 'regression-checks.json').read_text())
    assert checks['csv_checks_passed'] and checks['development_outputs_unchanged']
    assert not s['conversion_regressions'] and not s['previously_successful_outputs_changed']
    table = ['| BulkParse version | Returned / 112 | Median seconds / 112 attempts |',
             '| --- | ---: | ---: |']
    for key, label in [('before', 'Before fixes'), ('after', 'After fixes')]:
        v = s['versions'][key]
        table.append(f'| {label} | {v["returned_all_passes"]}/112 | {v["median_corpus_seconds"]:.3f} |')
    common = s['common_successful_median_document_ms']
    lines = ['# Real-world conversion fixes — November 12, 2026', '',
             '**12 additional documents now return output: 85/112 → 97/112.** The 85 previously successful real-world documents and 81 separate Office development documents retain byte-identical Markdown. Output returned is not a whole-document quality judgment.', '',
             '## General fixes', '',
             '- **Legacy CSV encodings:** preserve valid UTF-8 and BOM-aware UTF-16; attempt Windows-1252 for invalid UTF-8 without a BOM/override, with an explicit encoding-inferred diagnostic. Undefined bytes and binary control characters remain errors. An explicit `csv_encoding: "utf8"` keeps strict decoding; `windows1252` supports known legacy exports. No new dependency was added.',
             '- **Embedded XLS charts:** honor nested BIFF BOF/EOF boundaries so chart cache cells cannot collide with worksheet coordinates, and extraction continues after the chart. Actual duplicate worksheet cells still fail. Omitted chart/object substreams are diagnosed.',
             '- **RTF parsing:** retain only styles and math in the preliminary tree instead of copying every body token. Retained output inlines, line breaks and table definitions still consume node budget; depth, text and other limits remain enforced.', '',
             'These rules contain no document IDs, filenames, publishers or benchmark text. Original resource-limit defaults are unchanged.', '',
             '## Paired timing', '', *table, '',
             '**Apple M3 Max, 36 GiB RAM.** Each version processes all 112 documents sequentially in its own persistent process. One complete warm-up per version is excluded; five measured passes alternate version order. Reads, conversion and Markdown rendering are timed. Worker startup and output serialization are excluded. OCR is disabled.', '',
             'The updated version completes more work, so the full-corpus totals are not equal-success workload comparisons. On the identical 85 documents both versions returned in all passes, median per-document time was '
             f'**{common["before"]:.3f} ms before and {common["after"]:.3f} ms after**. This small timing difference is descriptive, not a general speedup claim. Resource sampling did not provide peak RSS in this run; no before/after memory claim is made.', '',
             'Every measured pass has the same outcomes and output hashes within each version. Input, executable and source hashes were checked throughout the paired run. [Raw summary and all timings](benchmark-final/summary.json) · [Machine, binary identities and configuration](benchmark-final/identity.json) · [Before/after outputs](benchmark-final/predictions/).', '',
             '## Validation', '',
             '- 196 Rust core/CLI tests pass, including new tests for legacy encoding, nested chart substreams and bounded RTF allocations.',
             '- Five regression tests also pass with all optional parser features disabled; CSV decoding does not depend on Office support.',
             '- All 368 checked records across the eight recovered CSV files match the frozen source records, checking every column and trimming surrounding whitespace only.',
             '- All 85 previously successful real-world documents and all 81 separate Office development fixtures retain identical Markdown. No conversion regressions in either set.', '',
             '[Recorded regression checks](regression-checks.json) · [Rust test output](validation/rust.log) · [Minimal-feature test output](validation/no-default-features.log). These checks do not establish complete fidelity for recovered RTF/XLS documents, charts or later unreviewed pages.', '',
             '## Remaining failures and limitations', '',
             '| Remaining limit | Documents |', '| --- | ---: |']
    for error, n in collections.Counter(r['error'] for r in s['remaining_failures']).items():
        lines.append(f'| {error.removeprefix("resource limit exceeded: ")} | {n} |')
    lines += ['', 'The 15 remaining failures are spreadsheets or a large CSV. Defaults remain 200,000 nodes, 100,000 table cells and 64 MiB expanded data. Some inputs contain hundreds of thousands of populated cells; two ODS content streams expand to approximately 88–91 MB. These are explicit resource-limit failures, not successful partial conversions. Larger configured budgets can increase memory use; bounded/streaming extraction is a further engineering task.', '',
              'Encoding inference cannot resolve every legacy or mixed-encoding file. One recovered CSV already contains UTF-8 replacement sequences alongside legacy bytes; the first 60 checked records match, but later mixed-encoding content is not certified. Users who know the source encoding should set it explicitly. RTF notes/list structure and XLS chart reconstruction remain existing limitations.', '',
              'This corpus was independently selected before the initial benchmark, but it has now informed fixes. The after-fix results are development/regression evidence, not an unseen holdout estimate.', '',
              '## Eight-converter study and quality review', '',
              'The earlier eight-converter timing run was stopped when the user prioritized fixes. Every converter completed warm-up and three measured passes; some also completed a fourth. It is **not** a completed five-pass comparison. [Preserved baseline status](../realworld-2026-11-12/conversion/STOPPED.md). Do not combine its partial timing run with this paired benchmark or publish it as a finished eight-converter leaderboard.', '',
              'The user selected a smaller, fixed-seed quality sample of 37 documents spanning all 13 formats. Its selection and existing anonymous assessments are preserved, but the review is unfinished. No converter identities have been revealed for aggregation, no quality scores are published here, and no paid API jobs were submitted. Any eventual same-chat assessment must be labeled assistant-reviewed, not independent blind judging.', '',
              '## Reproduce', '',
              'Retrieve the exact [112-input manifest](../realworld-2026-11-12/corpus-manifest.json) with `scripts/restore_realworld_corpus.py`. The original executable and source snapshot remain in the baseline conversion directory; build the changed core from this checkout. From the repository root:', '',
              '```sh', 'cargo test -p bulkparse -p bulkparse-cli --offline',
              'cargo test -p bulkparse --no-default-features --test regressions --offline',
              'cargo build -p bulkparse --example office_bench --release --offline',
              'python3 scripts/benchmark_realworld_fixes.py --out reports/realworld-fixes-rerun',
              'target/competitors/docling-venv/bin/python scripts/validate_realworld_fixes.py',
              '```', '',
              'The validation script checks the saved `benchmark-final` run and current binary against its recorded hash; it fails if the binary changes. To validate a separate rerun, use its artifacts after explicitly updating that path. No network/model/judge calls are involved. Do not overwrite completed measurement directories.', '']
    (OUT / 'README.md').write_text('\n'.join(lines))
    section = ['<!-- realworld-benchmark:start -->', '### Real-world robustness: 112 documents', '',
               '**A broader corpus exposed gaps the small development fixtures missed.** On 112 public documents across 13 formats, fixes to legacy CSV decoding, embedded XLS charts and RTF parsing increased returned outputs from **85 to 97**.', '',
               *table, '',
               'One excluded warm-up and five alternating sequential passes per version on the same Mac. The updated version completes more work; these totals are not an equal-success speed comparison. The 85 previously successful documents retain identical Markdown, as do 81 separate Office regression fixtures.', '',
               '**15 documents still exceed the default resource limits.** Returning output does not establish complete extraction quality. This corpus has now informed fixes, so these are regression results. The eight-converter comparison and the smaller assistant quality review remain unfinished; no quality leaderboard is claimed.', '',
               '[Fixes, measured results and remaining failures](reports/realworld-fixes-2026-11-12/README.md) · [Corpus and study status](benchmarks/realworld/README.md)',
               '<!-- realworld-benchmark:end -->', '']
    readme = ROOT / 'README.md'
    text = readme.read_text()
    start, end = '<!-- realworld-benchmark:start -->', '<!-- realworld-benchmark:end -->'
    if start in text:
        text = text[:text.index(start)] + '\n'.join(section) + text[text.index(end) + len(end):]
    else:
        text = text.replace('### Office and EPUB benchmark', '\n'.join(section) + '\n### Office and EPUB development benchmark')
    text = text.replace('Each PDF and Office configuration receives one excluded full warm-up and five measured sequential passes, with rotating engine order.',
                        'The completed PDF and Office development comparisons below use one excluded full warm-up and five measured sequential passes, with rotating engine order.')
    text = text.replace('No parser rules were changed for this comparison.',
                        'The real-world fix comparison is labeled separately; it measures changes prompted by newly discovered failures.')
    readme.write_text(text)


if __name__ == '__main__':
    main()
