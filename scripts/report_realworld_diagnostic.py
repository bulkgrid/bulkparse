#!/usr/bin/env python3
"""Publish the first complete measured pass after repeated timing was stopped."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import statistics

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT/'reports/realworld-comparison-2026-11-13'
RUN = OUT/'conversion'
ENGINES = ['bulkparse', 'anydoc', 'docling']
NAMES = {'bulkparse':'BulkParse', 'anydoc':'AnyDoc', 'docling':'Docling'}


def read(path):
    return json.loads(path.read_text())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    stop = read(RUN/'STOPPED.json')
    assert stop['analysis_rounds'] == [1]
    identity = read(RUN/'identity.json')
    for name,h in read(RUN/'source-manifest.json').items():
        assert sha(RUN/'source-snapshot'/name) == h
    assert sha(OUT/'baseline-office-bench') == identity['binary_sha256']['bulkparse']
    assert sha(OUT/'baseline-anydoc-bench') == identity['binary_sha256']['anydoc']
    corpus = read(RUN/'manifest.json')
    assert len(corpus) == len({d['sha256'] for d in corpus}) == 112
    assert all(sha(ROOT/d['path'])==d['sha256'] for d in corpus)
    hard = read(ROOT/'benchmarks/realworld-failures/manifest.json')
    hard_ids = {d['id'] for d in hard}
    rows = {}
    for e in ENGINES:
        data = read(RUN/f'{e}-round-1.json')['documents']
        assert [r['id'] for r in data] == [d['id'] for d in corpus]
        for r in data:
            p = RUN/'prediction'/e/'round-1'/(r['id']+'.md')
            assert sha(p) == r['output_sha256'] and p.stat().st_size == r['output_bytes']
        rows[e] = {r['id']:r for r in data}
    expected = read(ROOT/'benchmarks/realworld-failures/expected.json')['default']['documents']
    assert all(all(rows['bulkparse'][i].get(k)==v for k,v in want.items()) for i,want in expected.items())
    common = [d['id'] for d in corpus if all(rows[e][d['id']]['ok'] and rows[e][d['id']]['output_bytes'] for e in ENGINES)]
    summary = {'analysis_round':1, 'measured_passes_used':1, 'warmup_preceded_this_pass':True,
               'common_nonempty_ids':common, 'quality_score':None, 'peak_memory':None,
               'memory_reason':'Workers were interrupted before OS high-water statistics were emitted.', 'profiles':{}}
    table = '| Converter | Returned / 112 | Historical failures returned / 27 | Seconds / 112 | CPU seconds / 112 | Same-subset median ms | Same-subset median CPU ms |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: |\n'
    for e in ENGINES:
        rs = list(rows[e].values())
        values = dict(returned=sum(r['ok'] for r in rs), nonempty=sum(bool(r['ok'] and r['output_bytes']) for r in rs),
                      historical_returned=sum(rows[e][i]['ok'] for i in hard_ids),
                      seconds=sum(r['elapsed_ns'] for r in rs)/1e9, cpu_seconds=sum(r['cpu_seconds'] for r in rs),
                      common_median_ms=statistics.median(rows[e][i]['elapsed_ns']/1e6 for i in common),
                      common_median_cpu_ms=statistics.median(rows[e][i]['cpu_seconds']*1000 for i in common),
                      statuses=dict(Counter(r.get('status','returned_unverified' if r['ok'] else 'error') for r in rs)))
        summary['profiles'][e]=values
        table += f'| {NAMES[e]} | {values["returned"]} | {values["historical_returned"]} | {values["seconds"]:.3f} | {values["cpu_seconds"]:.3f} | {values["common_median_ms"]:.3f} | {values["common_median_cpu_ms"]:.3f} |\n'
    (OUT/'diagnostic-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    detail = '| Historical failure | BulkParse | AnyDoc | Docling |\n| --- | --- | --- | --- |\n'
    for d in hard:
        result=[]
        for e in ENGINES:
            r=rows[e][d['id']]
            result.append(r.get('status','returned; completeness unverified') if r['ok'] else 'failed')
        detail += '| `'+d['id']+'` | '+' | '.join(result)+' |\n'
    (OUT/'historical-failures.md').write_text('# All 27 historical failures\n\n'+detail)
    body=f'''# Real-world diagnostic comparison — November 13, 2026

The 27 documents BulkParse originally failed are now a [permanent regression suite](../../benchmarks/realworld-failures/README.md). All remain in the original 112-document corpus, counted once each. Twelve now return output under default settings; fifteen still fail. Recovery returns partial prefixes for those fifteen, not full documents.

## One-pass diagnostic results

{table}
The matching-subset columns use the same **{len(common)} documents** that all three tools returned nonempty in the first complete measured pass. Corpus totals include failures. Returning output does not prove complete or correct extraction, and faster rejection is not a parsing win. These are exploratory measurements, not a validated product-superiority claim.

BulkParse reports 11 complete and 86 partial results, plus 15 failures. AnyDoc does not report completeness in this adapter; Docling's success status is not independent quality verification. Different status conventions must not be treated as quality scores.

## What still needs fixing

- BulkParse's 15 default failures are 11 node limits, two cell limits and two expanded-data limits. These require better large-document handling and an explicit resource policy; merely returning bounded prefixes does not match a full conversion.
- AnyDoc returns 24/27 historical problem documents; Docling returns 26/27; BulkParse returns 12/27. The [per-document table](historical-failures.md) keeps every case visible.
- Some successful outputs have known feature omissions. Neither success counts nor speed establish higher quality. The smaller assistant quality review remains unfinished.
- Docling rejects one ambiguously encoded CSV. The nine historically failing CSVs do not pass strict Unicode reference decoding, so the source audit leaves them unscored rather than treating a fallback encoding as unquestionable ground truth. Its results and seven focused audit tests are retained.
- **Peak-memory comparison is unavailable from this interrupted run.** The workers were stopped before their OS high-water statistics were emitted. Earlier measurements from different runs are not substituted.

## Method and stop decision

Apple M3 Max, 36 GiB RAM, 14 logical CPUs, macOS. Same pinned AnyDoc source and installed Docling environment as recorded in `conversion`; versions, commands, thread settings, source/binary hashes, input hashes and power snapshots are retained. Input reading, parsing and Markdown export are timed; initialization and result transport are excluded. CPU is user+system time from the worker plus waited-for children, not energy use or CPU in unreaped children. The BulkParse default API retains its structured result until after its timed interval; API result lifetimes differ across tools.

One warm-up and four measured rounds completed before the user instructed us to stop repeated timing until BulkParse is competitive. The fifth Docling round was interrupted. This report uses **only the first complete measured round**, consistently for every input and tool; other raw rounds remain archived. This is **not a completed five-pass benchmark**.

Going forward, development runs default to **one pass with no extra warm-up**. Use targeted checks and reuse the frozen competitor outputs while fixing BulkParse. Repeated timing is reserved for explicit final validation after coverage and quality are competitive and initial speed/CPU/memory results meet the target.

The new suite passed 108 checks across default, recovery, Markdown and Markdown-recovery modes. All 240 full-feature Rust test executions passed. Strict Clippy still has four pre-existing warnings in unchanged parser code, documented separately. No parser production logic was changed to obtain these results; adapter changes only add CPU accounting. No paid judging jobs were submitted and no quality score is invented.

## Reproduce development checks

```sh
python3 scripts/restore_realworld_corpus.py --manifest benchmarks/realworld-failures/manifest.json
cargo build --release --locked -p bulkparse --example office_bench
python3 scripts/check_realworld_failures.py --out /tmp/bulkparse-failure-check
python3 scripts/benchmark_realworld_comparison.py --engines bulkparse --manifest benchmarks/realworld-failures/manifest.json --out /tmp/bulkparse-failure-pass
# One pass for all three tools, when comparator reruns are needed:
python3 scripts/benchmark_realworld_comparison.py --out /tmp/bulkparse-three-tool-pass
```

Use fresh output directories. Repeated timing now requires both `--final-validation` and an explicit `--passes` count; an excluded warm-up additionally requires `--warmup`. The original first-pass binaries and source snapshot are preserved in this report. The corpus has informed development and is not an unseen holdout.
'''
    (OUT/'README.md').write_text(body)
    print(json.dumps(summary,indent=2))


if __name__ == '__main__':
    main()
