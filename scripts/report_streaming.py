#!/usr/bin/env python3
"""Publish the streaming-output benchmark and its explicit scope/tradeoffs."""
import json,re
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'reports/streaming-2026-11-12'
def read(name):return json.loads((OUT/name).read_text())
def mib(n):return n/2**20
def main():
 s=read('benchmark-final/summary.json');v=s['versions'];checks=read('regression-checks.json');pdf=read('pdf-regression-checks.json')
 assert not any(s['stream_comparisons'].values()) and not pdf['changed_documents']
 assert all(not s[k] for k in ['default_output_changes','bounded_output_changes','default_diagnostic_changes','bounded_diagnostic_changes','conversion_regressions'])
 assert all(not r['issues'] for r in checks['source_checks']) and checks['limits_unchanged']
 assert all(r['unchanged'] for rows in checks['development'].values() for r in rows)
 before={(r['mode'],r['id']):r for r in read('per-document-before/summary.json')};after={(r['mode'],r['id']):r for r in read('per-document-after/summary.json')}
 for (mode,id),r in after.items():
  b=before['bounded' if mode in ['bounded','stream_bounded'] else 'default',id]
  assert all(r[k]==b[k] for k in ['ok','status','output_sha256'])
 differences=[]
 keys=['ok','status','output_sha256','diagnostics','render_diagnostics','error_code']
 for i in range(1,6):
  for a,b in [('before','default'),('before','stream'),('before_bounded','bounded'),('before_bounded','stream_bounded')]:
   previous=read(f'benchmark-final/{a}-pass-{i}.json')['documents'];current=read(f'benchmark-final/{b}-pass-{i}.json')['documents']
   for x,y in zip(previous,current):
    assert x['id']==y['id']
    changed=[k for k in keys if x.get(k)!=y.get(k)]
    if changed:differences.append({'pass':i,'profile':b,'id':x['id'],'fields':changed})
 assert not differences,differences
 (OUT/'all-pass-regressions.json').write_text(json.dumps({'measured_passes':5,'profile_comparisons':4,'documents_per_pass':112,'changed':differences},indent=2)+'\n')
 labels={'before':'Previous in-memory API','default':'Updated in-memory API','stream':'Streaming Markdown file','before_bounded':'Previous in-memory API + recovery','bounded':'Updated in-memory API + recovery','stream_bounded':'Streaming Markdown file + recovery'}
 table='| Profile | Returned / 112 | Median seconds / 112 attempts | Peak worker RSS MiB |\n| --- | ---: | ---: | ---: |\n'
 for key in ['before','default','stream','before_bounded','bounded','stream_bounded']:
  r=v[key];table+=f"| {labels[key]} | {r['returned_all_passes']}/112 | {r['median_corpus_seconds']:.3f} | {mib(r['lifetime_peak_rss_bytes']):.1f} |\n"
 delta={}
 for a,b in [('before','stream'),('before_bounded','stream_bounded')]:
  delta[b]={'peak_rss_reduction_percent':100*(1-v[b]['lifetime_peak_rss_bytes']/v[a]['lifetime_peak_rss_bytes']),'corpus_time_change_percent':100*(v[b]['median_corpus_seconds']/v[a]['median_corpus_seconds']-1)}
 peaks='| Source document | Mode | Previous MiB | Streaming MiB |\n| --- | --- | ---: | ---: |\n'
 for mode,id in [('default','csv-694b7b75298efd2b'),('bounded','csv-fbeb7b5e9b4f3ed9'),('default','xls-589ace4188fb7ccc'),('default','xls-b0aaa1a67a6a133c'),('default','ods-20f58e9c6ef54e2c'),('bounded','xlsm-1c382d45c18005cd')]:
  a=before[mode,id];b=after['stream' if mode=='default' else 'stream_bounded',id]
  peaks+=f"| {a['title']} (`{id}`) | {mode} | {mib(a['peak_rss_bytes']):.1f} | {mib(b['peak_rss_bytes']):.1f} |\n"
 tests={}
 for name in ['tests-final.log','minimal-tests-final.log']:
  results=re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed',(OUT/name).read_text());assert results and all(r[0]=='ok' and r[2]=='0' for r in results);tests[name]=sum(int(r[1]) for r in results)
 nodes=(OUT/'node-tests-final.log').read_text();assert 'fail 0' in nodes
 body=f'''# Streaming Markdown benchmark — November 12, 2026

BulkParse now offers a Markdown-only writer alongside its structured conversion API. CSV/TSV records and completed XLS/XLSB/XLSX/XLSM/ODS worksheets are rendered and released as output is staged. Table rendering avoids a second whole-table Markdown string. The existing conversion API still returns its full model and Markdown.

## Five-pass comparison on the same machine

{table}
Compared with the previous in-memory API, streaming reduced peak worker RSS by **{delta['stream']['peak_rss_reduction_percent']:.1f}%** with defaults and **{delta['stream_bounded']['peak_rss_reduction_percent']:.1f}%** with recovery. Corpus time changed by **{delta['stream']['corpus_time_change_percent']:+.1f}%** and **{delta['stream_bounded']['corpus_time_change_percent']:+.1f}%**, respectively (positive means slower).

These paths have different output contracts. Streaming timings include temporary spool creation, rendering, staging and writing a Markdown file; the in-memory API produces a model and Markdown string without file output. Verification readback and JSON protocol serialization are excluded from elapsed time but included in process-lifetime RSS. This is a memory/latency tradeoff, not a claim that streaming is the fastest conversion mode.

The frozen corpus contains 112 public documents across 13 formats. OCR is disabled. Each profile gets one persistent sequential worker, one excluded warm-up and five measured full-corpus passes, with rotating profile order. Median is over the five corpus totals. Peak RSS is the operating system's lifetime high-water mark, including warm-up. Hardware is the same Apple M3 Max with 36 GiB RAM and 14 logical CPUs, running macOS. Builds and other tests were not run during measured passes. Inputs, executable hashes and source snapshots were verified. These are macOS measurements, not Linux production guarantees.

## Which documents benefit?

Fresh process per document/profile; the table includes examples with large gains and examples where the remaining parser allocations dominate.

{peaks}
The CSV spending file converts successfully with complete status. The listed successful spreadsheet conversions still carry their existing partial/fidelity diagnostics. The planning CSV uses explicit resource-limit recovery. XLSX/ODS recovery still materializes its coordinate/value model, so it does not receive the CSV record-streaming benefit. No completeness improvement is claimed.

[All before profiles](per-document-before/summary.json) · [All after profiles](per-document-after/summary.json)

## Output and regression checks

- All 112 documents preserve success/failure, status and Markdown hashes across the previous API, updated API and streaming output, in both normal and recovery modes. Parser/render diagnostics and error codes also match in the five-pass comparison.
- All 81 previous Office fixtures match in four current profiles: structured/default, structured/recovery, streaming/default and streaming/recovery.
- All 200 PDFs in opendataloader-bench have identical Markdown, statuses and diagnostics between the preserved binary, updated structured output and streaming output. This is a renderer regression comparison; no new PDF quality score was calculated.
- Independent checks again verified 190,145 retained XLSX/XLSM/ODS source cells and 5,555 complete CSV records. The four recovered XLS documents do not have an independent per-cell audit; completed-sheet recovery is covered by BIFF tests and unchanged corpus output.
- {tests['tests-final.log']} full-feature Rust test executions, {tests['minimal-tests-final.log']} minimal-feature executions and 6 Node tests passed. CLI checks cover file output, stdin/stdout, no overwrite, no published malformed suffix, and exact partial CSV output with exit code 2. The OCR-enabled CLI build was checked; OCR itself was not benchmarked here.
- New tests exercise multi-sheet order, merges and styles, repeated ODS rows and headers, output spilling, row-budget boundaries, recovery after a written prefix, cancellation, writer errors and destination preservation. The previous diagnostic for externally supplied zero-span models is preserved.

[Corpus comparison](benchmark-final/summary.json) · [All five pass checks](all-pass-regressions.json) · [Source/Office checks](regression-checks.json) · [PDF checks](pdf-regression-checks.json) · [CLI checks](cli-checks.json)

## Use the implementation

```js
const {{parseFileToMarkdown}} = require('@bulkgrid/bulkparse');
const summary = await parseFileToMarkdown('workbook.xlsx', 'workbook.md');
```

The CLI uses streaming for Markdown conversion and batch output unless an explicit OCR provider is selected. Rust exposes `Parser::parse_to_markdown`, `parse_reader_to_markdown`, `parse_to_markdown_with_cancel` and `parse_file_to_markdown`. Node's new file API reads and writes in its native worker, without a JavaScript input copy or returned model/Markdown buffer. Python/WASM retain their existing structured conversion APIs.

Output staging uses a 64 KiB in-memory spool before spilling to a temporary file, plus a 64 KiB write buffer. A failed first attempt is discarded before partial recovery restarts. Parse/strict failures publish no Markdown. File APIs atomically publish completed output and refuse to replace existing destinations. A write failure or cancellation during copying to a caller-owned writer can leave a prefix in that writer. See the [API and failure contract](../../docs/streaming-markdown.md).

## Remaining limits

**This is output streaming, not fully streamed workbook parsing.** Inputs are still capped and buffered. Encoded CSV can require a decoded text buffer. XML, shared strings, merge/layout information and one worksheet model can still determine peak memory. Further savings on large single-sheet XLSX/ODS files require incremental worksheet XML extraction, while preserving styles, merges and diagnostics. The report includes those limited-gain cases rather than presenting only the CSV improvement.

Original resource limits are unchanged. Default conversion still returns 97/112; optional recovery returns 112/112, including 15 resource-limited prefixes. Only 11 documents have complete status; recovery has 101 partial results. Legacy formatting, notes/equations and complex layouts still have existing fidelity gaps. This corpus has informed development and is now regression evidence. The broader eight-converter quality review remains unfinished.

## Reproduce

Keep the corpus paths from the recorded manifest and preserved baseline binary available. Use new output directories:

```sh
cargo build --offline --release -p bulkparse --example office_bench
python3 scripts/profile_parser_memory.py --binary target/release/examples/office_bench --out reports/streaming-recheck-documents --modes default bounded stream stream_bounded
python3 scripts/benchmark_streaming.py --out reports/streaming-recheck/benchmark-final
```

For this recorded report root:

```sh
target/competitors/docling-venv/bin/python scripts/validate_large_documents.py --out reports/streaming-2026-11-12
python3 scripts/check_streaming_pdf.py
python3 scripts/report_streaming.py
```

The source validator uses BeautifulSoup and markdown-it-py, not Docling conversion. `/usr/bin/time -l` is macOS-specific and may require permission to inspect process memory. The benchmark runners refuse to overwrite existing runs. The source validator expects `before/source-snapshot` beside `benchmark-final`; preserve that when re-running elsewhere.

[Configuration and binary hashes](benchmark-final/identity.json) · [Source snapshot](benchmark-final/source-snapshot/) · [Before source snapshot](before/source-snapshot/)

`benchmark-development` and `per-document-development` retain an intermediate build. Final measurements are in `benchmark-final` and `per-document-after`.
'''
 (OUT/'README.md').write_text(body);(OUT/'comparison.json').write_text(json.dumps(delta,indent=2)+'\n')
 p=ROOT/'README.md';text=p.read_text();start=text.index('<!-- realworld-benchmark:start -->');end=text.index('<!-- realworld-benchmark:end -->',start)+len('<!-- realworld-benchmark:end -->')
 concise='| Output path | Returned / 112 | Median seconds / 112 attempts | Peak worker RSS MiB |\n| --- | ---: | ---: | ---: |\n'
 for key in ['before','stream','before_bounded','stream_bounded']:
  r=v[key];concise+=f"| {labels[key]} | {r['returned_all_passes']}/112 | {r['median_corpus_seconds']:.3f} | {mib(r['lifetime_peak_rss_bytes']):.1f} |\n"
 section=f'''<!-- realworld-benchmark:start -->
### Real-world large-document handling

**112 public documents across 13 formats exposed limits the smaller fixtures missed.** Encoding and legacy-format fixes increased returned outputs from 85 to 97. Bounded recovery returns partial data from the remaining 15 inputs. Markdown-only streaming now releases CSV records and completed spreadsheet worksheets as output is staged, without raising limits.

{concise}
Streaming uses less memory in this run, with additional staging and file-write work. The previous API returns its model and Markdown in memory; these are different output contracts. Timings include streaming file writes. One excluded warm-up and five rotating sequential passes on the same Apple M3 Max; OCR disabled. Peak RSS is a measured process high-water mark, not a memory guarantee.

**The 15 resource-limit recoveries are explicitly partial—not full conversions.** Only 11 results have complete status. All 112 documents retain their previous Markdown, statuses and diagnostics; 81 Office fixtures and 200 PDFs also show no output regressions. This used corpus is regression evidence. The broader eight-converter quality review remains unfinished; no quality leaderboard is claimed.

[Streaming results and tradeoffs](reports/streaming-2026-11-12/README.md) · [Use the streaming API](docs/streaming-markdown.md) · [Earlier XML memory improvements](reports/memory-2026-11-12/README.md) · [Corpus and study status](benchmarks/realworld/README.md)
<!-- realworld-benchmark:end -->'''
 p.write_text(text[:start]+section+text[end:]);print(json.dumps(delta,indent=2))
if __name__=='__main__':main()
