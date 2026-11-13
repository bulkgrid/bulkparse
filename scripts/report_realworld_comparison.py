#!/usr/bin/env python3
"""Validate all five passes and publish the three-tool real-world comparison."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import statistics

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT/'reports/realworld-comparison-2026-11-13'
RUN = OUT/'conversion'
ENGINES = ['bulkparse', 'anydoc', 'docling']
NAMES = {'bulkparse':'BulkParse', 'anydoc':'AnyDoc', 'docling':'Docling',
         'bulkparse_markdown':'BulkParse Markdown file', 'bulkparse_recovery':'BulkParse + recovery',
         'bulkparse_markdown_recovery':'BulkParse Markdown file + recovery'}


def read(path):
    return json.loads(path.read_text())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def median(values):
    return statistics.median(values) if values and all(v is not None for v in values) else None


def fmt(value, places=2):
    return f'{value:.{places}f}' if value is not None else 'unavailable'


def main():
    identity = read(RUN/'identity.json')
    verification = read(RUN/'verification.json')
    assert verification['complete'] and not verification['smoke'] and identity['measured_passes'] == 5
    assert all(sha(ROOT/p) == h for p,h in read(RUN/'source-manifest.json').items())
    assert all(sha(Path(cmd[0])) == identity['binary_sha256'][e] for e,cmd in identity['commands'].items())
    corpus = read(RUN/'manifest.json')
    assert len(corpus) == len({d['sha256'] for d in corpus}) == 112
    assert all(sha(ROOT/d['path']) == d['sha256'] for d in corpus)
    historical = read(ROOT/'benchmarks/realworld-failures/manifest.json')
    hard_ids = {d['id'] for d in historical}
    assert len(hard_ids) == 27 and hard_ids <= {d['id'] for d in corpus}
    expected = read(ROOT/'benchmarks/realworld-failures/expected.json')
    modes = {'bulkparse':'default', 'bulkparse_markdown':'stream', 'bulkparse_recovery':'bounded', 'bulkparse_markdown_recovery':'stream_bounded'}
    runs, changes = {}, []
    for e in identity['commands']:
        runs[e] = []
        for index in range(6):
            rows = read(RUN/f'{e}-round-{index}.json')['documents']
            assert [r['id'] for r in rows] == [d['id'] for d in corpus]
            for row in rows:
                p = RUN/'prediction'/e/f'round-{index}'/(row['id']+'.md')
                assert sha(p) == row['output_sha256'] and p.stat().st_size == row['output_bytes']
                assert row['elapsed_ns'] >= 0
                assert row.get('cpu_seconds') is None or row['cpu_seconds'] >= 0
                if e in modes and row['id'] in hard_ids:
                    wanted = expected[modes[e]]['documents'][row['id']]
                    fields = [k for k,v in wanted.items() if row.get(k) != v]
                    if fields:
                        changes.append(dict(engine=e, run=index, id=row['id'], fields=fields))
            runs[e].append(rows)
    assert not changes, changes
    summary = read(RUN/'summary.json')
    docs = {e:{d['id']:d for d in s['documents']} for e,s in summary.items()}
    common = [d['id'] for d in corpus if all(docs[e][d['id']]['nonempty_all'] for e in ENGINES)]
    hard_common = [d for d in common if d in hard_ids]
    data = dict(common_nonempty_ids=common, historical_failure_common_nonempty_ids=hard_common, profiles={})
    overview = '| Converter / mode | Returned / 112 | Nonempty / 112 | Median ms / document | Median seconds / corpus | CPU seconds / corpus | Worker peak MiB |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: |\n'
    common_table = '| Converter | Same-subset median ms | Same-subset median CPU ms | Historical failures returned / 27 |\n| --- | ---: | ---: | ---: |\n'
    for e,s in summary.items():
        measured = runs[e][1:]
        hard_returned = sum(docs[e][i]['returned_all'] for i in hard_ids)
        hard_nonempty = sum(docs[e][i]['nonempty_all'] for i in hard_ids)
        stat = dict(returned_all=s['returned_all'], nonempty_all=s['nonempty_all'],
            median_ms=s['median_document_ms'], corpus_seconds=median(s['corpus_seconds']),
            corpus_cpu_seconds=median(s['corpus_cpu_seconds']), peak_mib=s['resources']['lifetime_peak_rss_bytes']/2**20,
            common_median_ms=median([docs[e][i]['median_ms'] for i in common]),
            common_median_cpu_ms=median([docs[e][i]['median_cpu_ms'] for i in common]),
            historical_returned=hard_returned, historical_nonempty=hard_nonempty,
            first_measured_statuses=dict(Counter(r.get('status', 'returned_unverified' if r['ok'] else 'error') for r in measured[0])),
            nondeterministic_ids=[r['id'] for r in s['documents'] if not r['deterministic']])
        data['profiles'][e] = stat
        overview += f'| {NAMES[e]} | {s["returned_all"]} | {s["nonempty_all"]} | {stat["median_ms"]:.2f} | {stat["corpus_seconds"]:.3f} | {fmt(stat["corpus_cpu_seconds"],3)} | {stat["peak_mib"]:.1f} |\n'
        if e in ENGINES:
            common_table += f'| {NAMES[e]} | {fmt(stat["common_median_ms"])} | {fmt(stat["common_median_cpu_ms"])} | {hard_returned} |\n'
    (OUT/'comparison.json').write_text(json.dumps(data,indent=2)+'\n')
    per_format = {}
    format_table = '| Format | Inputs | BulkParse returned | AnyDoc returned | Docling returned | Common nonempty inputs | BulkParse ms | AnyDoc ms | Docling ms |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n'
    for kind in sorted({d['format'] for d in corpus}):
        ids = [d['id'] for d in corpus if d['format']==kind]
        shared = [i for i in common if i in ids]
        counts = [sum(docs[e][i]['returned_all'] for i in ids) for e in ENGINES]
        times = [median([docs[e][i]['median_ms'] for i in shared]) for e in ENGINES]
        per_format[kind] = dict(inputs=ids, common_nonempty_ids=shared, profiles={e:dict(
            returned=sum(docs[e][i]['returned_all'] for i in ids),
            common_median_ms=median([docs[e][i]['median_ms'] for i in shared]),
            common_median_cpu_ms=median([docs[e][i]['median_cpu_ms'] for i in shared])) for e in ENGINES})
        format_table += f'| {kind} | {len(ids)} | '+ ' | '.join(map(str,counts))+f' | {len(shared)} | '+' | '.join(fmt(t) for t in times)+' |\n'
    (OUT/'per-format.json').write_text(json.dumps(per_format,indent=2)+'\n')
    first = {e:{r['id']:r for r in passes[1]} for e,passes in runs.items()}
    hard_table = '| Historical failure | BulkParse default | BulkParse recovery | AnyDoc | Docling |\n| --- | --- | --- | --- | --- |\n'
    def outcome(e, doc_id):
        row = first[e][doc_id]
        if not row['ok']:
            return 'failed: '+row.get('error','unknown').replace('|','\\|').replace('\n',' ')[:100]
        if not row['output_bytes']:
            return 'returned empty'
        if any(d.get('code')=='resource_limit_partial' for d in row.get('diagnostics',[])):
            return '**partial prefix**'
        return row.get('status', 'returned; completeness unverified')
    for d in historical:
        hard_table += '| `'+d['id']+'` | '+' | '.join(outcome(e,d['id']) for e in ['bulkparse','bulkparse_recovery','anydoc','docling'])+' |\n'
    (OUT/'historical-failures.md').write_text('# All 27 historical failures\n\n'+hard_table+'\nStatuses are converter reports, not independently verified quality grades. Full-corpus timing includes each input once.\n')
    audits = read(OUT/'csv-source-audit.json')
    assert len(audits['documents']) == 9
    audit_table = '| CSV input | Source records | BulkParse matched | AnyDoc matched | Docling matched | BulkParse recovery matched |\n| --- | ---: | ---: | ---: | ---: | ---: |\n'
    for d in audits['documents']:
        cells = []
        for e in ['bulkparse','anydoc','docling','bulkparse_recovery']:
            r = d['engines'][e]
            assert r['output_sha256'] == first[e][d['id']]['output_sha256']
            cells.append(str(r['matched_source_rows']) if r['auditable'] else 'needs review')
        source_count = f'{d["source_rows"]:,}' if d['source_rows'] is not None else 'needs review'
        audit_table += f'| `{d["id"]}` | {source_count} | '+' | '.join(cells)+' |\n'
    tests = re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed', (OUT/'rust-tests.log').read_text())
    assert tests and all(x=='ok' and z=='0' for x,y,z in tests)
    test_count = sum(int(y) for x,y,z in tests)
    checks = read(OUT/'failure-regressions/results.json')
    assert checks['checks']==108 and not checks['differences']
    assert checks['binary_sha256']==identity['binary_sha256']['bulkparse']
    assert read(RUN/'effective-options.json')['docling']['ready']
    docling_version = read(RUN/'effective-options.json')['docling']['versions']['docling']
    anydoc_version = re.search(r'^version\s*=\s*"([^"]+)"',(ROOT/'anydoc-main/Cargo.toml').read_text(),re.M)[1]
    power_before = re.search(r'(\d+)%;',identity['power_before'])[1]
    power_after = re.search(r'(\d+)%;',verification['power_after'])[1]
    power_samples = [identity['power_before'], verification['power_after']]
    power_samples += [e['power'] for e in read(RUN/'progress.json')['events']]
    power_sources = ', '.join(sorted({re.search(r"Now drawing from '([^']+)'", text)[1] for text in power_samples}))
    b = data['profiles']['bulkparse']
    body = f'''# Real-world comparison including historical failures — November 13, 2026

All **112 original documents** were rerun through BulkParse, AnyDoc and Docling. The **27 documents BulkParse originally failed** are now a permanent regression suite and remain included once each in the full corpus. Twelve of those now return output under default settings; fifteen still hit limits. Recovery is separately labeled and returns partial prefixes for those fifteen.

## Coverage and performance

{overview}
“Returned” means the converter returned without an error on all five measured passes. “Nonempty” additionally requires nonempty Markdown on every pass. Neither means complete or correct extraction. All three primary tools attempt the same 112 inputs across 13 formats. Failed attempts remain included in the full-cohort timing; a fast rejection is not successful parsing. Additional BulkParse writer/recovery rows are separate modes, measured after the main three-tool comparison.

### Identical subset with nonempty output

The following comparison uses the **same {len(common)} documents** that all three primary tools returned nonempty on all five measured passes. It avoids comparing speed on different document sets, but does not establish equivalent completeness or quality. Full subset IDs are in `comparison.json`.

{common_table}
BulkParse's first measured default pass has {b['first_measured_statuses'].get('complete',0)} complete, {b['first_measured_statuses'].get('partial',0)} partial and {112-b['returned_all']} failed results. Other tools expose different status conventions: AnyDoc does not report completeness here; Docling's returned status is not an independent completeness assessment. Do not interpret fewer reported warnings as higher quality.

### Per-format coverage and matching-subset timing

{format_table}
Returned counts cover every input in the format; the three timing columns use only that format's common nonempty subset. Per-format CPU measurements and exact subset IDs are in `per-format.json`. A single PPT and other small format samples are weak evidence for generalization. DOCM is absent; this corpus covers 13 formats, not 14.

## All historical failures stay visible

The [27-case outcome table](historical-failures.md) lists defaults and recovery side by side for every original failure, with AnyDoc and Docling results. The [permanent suite](../../benchmarks/realworld-failures/README.md) retains source URLs, licenses, hashes and original errors. No inputs were duplicated to overweight difficult cases, removed for failing, or replaced by easier variants.

## Narrow source-content check

For the nine historically failing CSVs, a separate audit reads the original CSV records using strict Unicode decoding and a detected delimiter and compares complete table rows in each saved Markdown output. Cell order and duplicate-row multiplicity must match; Unicode and whitespace are normalized. Headers count as source records. This is an exact-row preservation check, not a general quality score or a measure of row ordering, layout or formatting. Non-tabular outputs require direct review instead of receiving an invented score. Sources that fail strict Unicode decoding are also unscored for every engine until their intended encoding is independently established; fallback decoding is not treated as ground truth.

{audit_table}
The raw audit records source settings, output hashes and examples of unmatched rows. A failed conversion has zero returned source rows. Recovery prefixes can match the source accurately while still omitting most of a document. The broader agreed 37-document assistant quality review remains unfinished, and this report does not claim BulkParse has higher quality than its competitors.

## Method and scope

- Apple M3 Max, 36 GiB RAM, 14 logical CPUs, macOS. Charge was {power_before}% at the start and {power_after}% at completion; observed power sources: {power_sources}. Raw power settings and snapshots are saved. These are same-machine observations, not Linux production measurements.
- AnyDoc {anydoc_version} from the pinned vendored source; Docling {docling_version} from the installed isolated environment; BulkParse's source snapshot and executable hashes are recorded. No parser production logic was changed for this comparison. Adapter changes add CPU accounting only.
- One complete warm-up is excluded, followed by five measured full-corpus passes. Documents are sequential and the three converters rotate order. Workers persist between passes; imports/initialization are excluded from elapsed conversion time. Input reading, parsing and Markdown export are included. Child process launches inside conversion are included. Thread environment settings are recorded, but are not proof that every backend honors them.
- No OCR or model downloads are enabled for this Office/EPUB/CSV corpus. This is separate from the 200-PDF layout/OCR benchmarks. No new NID/TEDS/MHS or LLM-judge scores were generated.
- BulkParse default timing covers `parse_bytes` plus Markdown rendering while retaining the returned structured result; destruction of that caller-owned result occurs after its timed interval. Markdown-file mode releases internal table rows within its timed API. Other converter APIs have different result lifetimes. These measurements describe the recorded API boundaries, not identical internal work.
- Median milliseconds per document is the median across documents of each document's five-pass median. Corpus seconds and CPU seconds are medians of five complete pass totals. Per-format and per-document data are retained; corpus shape can affect aggregate rankings.
- CPU is user plus system time measured inside each worker with `getrusage(SELF)` plus waited-for `CHILDREN`, including native threads. Imports, verification/readback and response serialization are excluded. CPU in live/unreaped subprocesses is not included; timed-out workers have missing CPU, never zero. CPU time is distinct from elapsed time and is not an energy measurement. Microsecond resolution and the small accounting overhead matter for very fast inputs.
- Peak RSS is the OS worker-lifetime high-water mark, including initialization, warm-up and result transport. It is **not aggregate process-tree memory** and is not a concurrency budget. In particular, child-process memory is outside that reported scope. No lower total-memory claim is made from these worker-only values.
- The 90-second document deadline, installed converter defaults and BulkParse resource limits are retained. Recovery does not silently replace failed default output. Markdown-file mode includes staging/file writes; readback for verification is excluded from timing but remains in peak memory.
- The remaining five converters from the original eight-tool study were deferred at the user's choice. Their older timings were not mixed into this fresh comparison. This corpus now informs development and is not an unseen holdout.

## Verification

All inputs, binaries, source snapshots and saved prediction hashes were verified. All 27 historical cases matched their frozen outcomes, content and diagnostics in four BulkParse modes, including all six benchmark rounds. The standalone suite passed 108 checks; {test_count} Rust test executions passed. Three-converter smoke checks were completed before timing. CPU instrumentation was checked for nonnegative per-document readings.

Strict Clippy still reports four pre-existing warnings in unchanged parser files; their identity against the previous source snapshot is recorded in `preexisting-lints.json`. A scoped benchmark check passes with only those existing lint categories allowed. This report does not claim a clean strict-lint run.

## Reproduce

```sh
python3 scripts/restore_realworld_corpus.py
cargo build --release --locked -p bulkparse --example office_bench
CARGO_TARGET_DIR=target/multiformat-anydoc cargo build --release --locked --manifest-path benchmarks/multiformat/anydoc/Cargo.toml
python3 scripts/check_realworld_failures.py --out /tmp/bulkparse-failure-check
python3 scripts/benchmark_realworld_comparison.py --out /tmp/bulkparse-three-tool-run
target/competitors/docling-venv/bin/python scripts/check_realworld_csv_content.py --run /tmp/bulkparse-three-tool-run --out /tmp/bulkparse-csv-audit.json
```

Use fresh output directories and the pinned environment recorded in `conversion/docling-requirements.txt`; do not rebuild or edit measured source while a run is active. `conversion` retains warm-up and all five rounds, predictions, errors, CPU/elapsed times, process logs, effective options, power snapshots, source and binary hashes. Regenerate this report from the saved final run with `python3 scripts/report_realworld_comparison.py`.
'''
    (OUT/'README.md').write_text(body)
    print(json.dumps(dict(common_documents=len(common), profiles=data['profiles'], checks=108, rust_tests=test_count),indent=2))


if __name__ == '__main__':
    main()
