#!/usr/bin/env python3
"""Publish measured memory results, without inferring quality from returned output."""
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'reports/memory-2026-11-12'
def read(path):return json.loads((OUT/path).read_text())
def mib(n):return n/(1024**2)
def main():
 s=read('benchmark-final/summary.json');v=s['versions'];checks=read('regression-checks.json')
 assert all(not s[k] for k in ['conversion_regressions','default_conversion_regressions','default_output_changes','bounded_output_changes','default_diagnostic_changes','bounded_diagnostic_changes','previously_successful_outputs_changed'])
 assert checks['limits_unchanged'] and all(not r['issues'] for r in checks['source_checks'])
 assert all(r['unchanged'] for rs in checks['development'].values() for r in rs)
 before={(r['mode'],r['id']):r for r in read('per-document-before/summary.json')}
 after={(r['mode'],r['id']):r for r in read('per-document-after/summary.json')}
 assert before.keys()==after.keys()
 assert all(all(b[k]==after[key][k] for k in ['ok','status','output_sha256']) for key,b in before.items())
 table='| Profile | Returned / 112 | Median seconds / 112 attempts | Peak worker RSS MiB |\n| --- | ---: | ---: | ---: |\n'
 for key,label in [('before','Before: defaults'),('default','After: defaults'),('before_bounded','Before: bounded recovery'),('bounded','After: bounded recovery')]:
  row=v[key];table+=f"| {label} | {row['returned_all_passes']}/112 | {row['median_corpus_seconds']:.3f} | {mib(row['lifetime_peak_rss_bytes']):.1f} |\n"
 deltas={}
 for old,new in [('before','default'),('before_bounded','bounded')]:
  deltas[new]={'peak_rss_reduction_percent':100*(1-v[new]['lifetime_peak_rss_bytes']/v[old]['lifetime_peak_rss_bytes']),'corpus_time_reduction_percent':100*(1-v[new]['median_corpus_seconds']/v[old]['median_corpus_seconds'])}
 slowdowns=[]
 for old,new in [('before','default'),('before_bounded','bounded')]:
  previous={r['id']:r for r in v[old]['documents']}
  for row in v[new]['documents']:
   prior=previous[row['id']]['median_ms']
   if row['median_ms']-prior>2 and row['median_ms']>prior*1.2:
    slowdowns.append({'profile':new,'id':row['id'],'before_median_ms':prior,'after_median_ms':row['median_ms']})
 (OUT/'per-document-slowdowns.json').write_text(json.dumps(slowdowns,indent=2)+'\n')
 slowtable='| Document / profile | Before median ms | After median ms |\n| --- | ---: | ---: |\n'
 for row in slowdowns:slowtable+=f"| `{row['id']}` / {row['profile']} | {row['before_median_ms']:.2f} | {row['after_median_ms']:.2f} |\n"
 peaks='| Document | Default before → after MiB | Recovery before → after MiB |\n| --- | ---: | ---: |\n'
 for id in ['ods-5cefbfc9f77dafdd','ods-39a8edec753b433a','xlsx-3d7484ae52ab377f','ods-c197f40e846f9d2b','csv-fbeb7b5e9b4f3ed9']:
  b=before['default',id];values=[f"{mib(before[m,id]['peak_rss_bytes']):.1f} → {mib(after[m,id]['peak_rss_bytes']):.1f}" for m in ['default','bounded']]
  peaks+=f"| {b['title']} (`{id}`) | {values[0]} | {values[1]} |\n"
 repeat_before={(r['mode'],r['id']):r for r in read('repeated-before/summary.json')}
 repeat_after={(r['mode'],r['id']):r for r in read('repeated-after/summary.json')}
 repeated='| 20 conversions in one process | Before MiB | After MiB |\n| --- | ---: | ---: |\n'
 for key,r in repeat_before.items():
  repeated+=f"| `{key[1]}`, {key[0]} | {mib(r['peak_rss_bytes']):.1f} | {mib(repeat_after[key]['peak_rss_bytes']):.1f} |\n"
 stress=read('repeated-200-after/summary.json')[0]
 report=f'''# Parser memory profile — November 12, 2026

The largest avoidable memory peaks came from oversized ODS and XLSX XML parts. **OCR was disabled.** Streaming preflight now rejects XML that cannot fit the node budget before expanding it into a retained string or allocating its document tree. ZIP text buffers also become owned strings without cloning the full text.

## Same-machine before/after

{table}
Default peak RSS fell **{deltas['default']['peak_rss_reduction_percent']:.1f}%**, and bounded-recovery peak RSS fell **{deltas['bounded']['peak_rss_reduction_percent']:.1f}%**. Median corpus time fell **{deltas['default']['corpus_time_reduction_percent']:.1f}%** and **{deltas['bounded']['corpus_time_reduction_percent']:.1f}%**, respectively. These are before/after comparisons within each profile, not a comparison of equal work between profiles or against another converter.

One excluded warm-up and five rotating sequential full-corpus passes per profile, one persistent process each. Timings include file reading, parsing and Markdown rendering; setup and protocol serialization are excluded. RSS is the operating system's lifetime high-water mark for the worker, including warm-up and serialization. Hardware: Apple M3 Max, 36 GiB RAM, 14 logical CPUs, macOS. No concurrent build or test job ran during the timed passes. Inputs, source and binary hashes were checked. RSS varies with allocator and operating-system behavior; these measurements do not establish Linux production memory requirements.

## CPU tradeoff on individual documents

Accepted large XML parts are scanned and then decompressed again. Aggregate time improved, but some documents became slower. The rows below exceed both a 2 ms and 20% increase in median document time; this is a descriptive filter, not a statistical significance test.

{slowtable}
## Which documents caused the peaks?

Fresh process per document/profile; RSS includes worker startup, conversion and serialization. Source titles and IDs come from the frozen 112-document manifest.

{peaks}
The CSV path was not changed in this memory optimization. Differences in its measured RSS are not attributed to the XML changes. The complete rankings, including all 112 documents in both profiles, are in [before](per-document-before/summary.json) and [after](per-document-after/summary.json).

## Repeated conversions

{repeated}
An additional **200-conversion** stress check on the worst rejected ODS peaked at **{mib(stress['peak_rss_bytes']):.1f} MiB** with defaults. This is an observed high-water mark, not a hard memory guarantee. We did not run that 200-conversion stress check on the original baseline; the table above compares matching 20-conversion runs.

## Implementation

- XLSX/XLSM and ODF XML parts above 256 KiB receive a streaming check against the remaining node budget. Relationship XML uses the same path. No document names, source IDs or corpus-specific thresholds appear in the parser.
- The check retains a read buffer and the current XML event, not the decompressed XML prefix. It counts guaranteed distinct DOM nodes, leaving coalesced text/CDATA/entity counts to the authoritative DOM parser. Cancellation, expansion limits and DTD rejection remain enforced.
- Accepted large parts are decompressed again for full-fidelity extraction. This trades CPU on those parts for lower memory on oversized rejections. Smaller parts use the direct path.
- UTF-8 ZIP buffers transfer ownership into strings. The BOM is removed in place; malformed UTF-8 is rejected.
- All existing configured limits remain unchanged. The normal structured-output and Markdown APIs still materialize their results.

## Regression evidence

All 112 documents have identical Markdown hashes and statuses before/after in both profiles. Diagnostics are unchanged in the five-pass corpus run. All 81 earlier Office fixtures are unchanged in both modes. Independent source checks again verified 190,145 retained XLSX/XLSM/ODS cells and 5,555 complete CSV records. The four recovered XLS inputs do not have an independent per-cell audit; their completed-sheet behavior is covered by synthetic BIFF regression tests.

All 206 full-feature Rust test executions, 24 minimal-feature Rust test executions and 5 Node binding tests passed. The Node binding was rebuilt against this core. The test logs and [regression-checks.json](regression-checks.json) record validation. New tests cover early rejection before reading an entire large XML part, coalesced text/entity/CDATA budget boundaries, BOM preservation, cumulative budgets, cancellation, invalid UTF-8 and expansion limits.

## What remains

- **15 documents still exceed normal limits.** Opt-in recovery returns explicitly partial output for those inputs. Defaults return 97/112; recovery returns 112/112, of which 101 have partial status and only 11 complete status. Returned output is not a quality score.
- Full streaming of accepted worksheets, shared strings and rendered output remains future work. Input bytes, DOM structures, the document model and Markdown may coexist, and a single large XML event still requires storage. Per-request limits are not a hard RSS cap; production concurrency multiplies memory needs.
- Recovery preserves useful prefixes, not all source layout: spreadsheet previews omit merges, styling, later content and other features. Unread tails and ZIP checksums are not certified when recovery stops early.
- Legacy Office automatic numbering, some notes/equations, charts/images and complex layout still have fidelity gaps. The broader eight-converter quality study remains unfinished. This now-used corpus is regression evidence, not an unseen quality estimate.

## Reproduce

From the repository root, with the frozen corpus and preserved baseline executable available:

```sh
cargo build --offline --release -p bulkparse --example office_bench
python3 scripts/profile_parser_memory.py --binary target/release/examples/office_bench --out reports/memory-recheck-documents
python3 scripts/benchmark_parser_memory.py --out reports/memory-recheck/benchmark-final
```

The report-root source validator expects a `before/source-snapshot` alongside `benchmark-final`. For this recorded run:

```sh
target/competitors/docling-venv/bin/python scripts/validate_large_documents.py --out reports/memory-2026-11-12
python3 scripts/report_parser_memory.py
```

The validator uses BeautifulSoup and markdown-it-py; it does not invoke Docling. A repeated-document run uses `profile_parser_memory.py --repeats 20 --ids DOCUMENT_ID ...`. `/usr/bin/time -l` is macOS-specific and may require permission to read process metrics. Use a fresh output directory; runners refuse to overwrite evidence.

[Configurations and executable identity](benchmark-final/identity.json) · [Full summary](benchmark-final/summary.json) · [After source snapshot](benchmark-final/source-snapshot/) · [Before source snapshot](before/source-snapshot/) · [Enable partial recovery](../../docs/large-documents.md)

`benchmark-development` and similarly named profiling folders retain the intermediate implementation. Only `benchmark-final`, `per-document-after` and `repeated-after` are the final implementation results.
'''
 (OUT/'README.md').write_text(report)
 (OUT/'comparison.json').write_text(json.dumps(deltas,indent=2)+'\n')
 p=ROOT/'README.md';text=p.read_text();start=text.index('<!-- realworld-benchmark:start -->');end=text.index('<!-- realworld-benchmark:end -->',start)+len('<!-- realworld-benchmark:end -->')
 section=f'''<!-- realworld-benchmark:start -->
### Real-world large-document handling

**112 public documents across 13 formats exposed limits the smaller fixtures missed.** Encoding and legacy-format fixes increased returned outputs from 85 to 97. Optional bounded recovery returns partial data from the remaining 15 inputs. Streaming XML checks and fewer duplicate buffers now reduce memory use without raising limits.

{table}
**The 15 resource-limit recoveries are explicitly partial—not full conversions.** Defaults return 97/112 outputs; recovery returns 112/112. Only 11 results have complete status in either mode. Recovery retains complete CSV records, completed XLS sheets or coordinate/value previews for XLSX/ODS; formatting and later content may be omitted.

One excluded warm-up and five rotating sequential passes on the same Apple M3 Max; OCR disabled. Compare before/after within each profile: recovery does more work. Peak worker RSS fell {deltas['default']['peak_rss_reduction_percent']:.0f}% with defaults and {deltas['bounded']['peak_rss_reduction_percent']:.0f}% with recovery. All returned Markdown, statuses and diagnostics are unchanged; 81 separate Office fixtures also match. These are measured process peaks, not memory guarantees.

This corpus has informed fixes and is regression evidence. The broader eight-converter comparison and assistant quality review remain unfinished; no quality leaderboard is claimed.

[Memory profiles and reproducible results](reports/memory-2026-11-12/README.md) · [Recovery contract and source checks](reports/large-documents-2026-11-12/README.md) · [Corpus and study status](benchmarks/realworld/README.md)
<!-- realworld-benchmark:end -->'''
 p.write_text(text[:start]+section+text[end:])
 print(json.dumps(deltas,indent=2))
if __name__=='__main__':main()
