#!/usr/bin/env python3
"""Validate and report the general BIFF5/7 compatibility change."""
import difflib,hashlib,json,re,shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'reports/biff-legacy-2026-11-13';CHANGED='xls-98c4582ee4ac67ec'
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 manifest=read(ROOT/'reports/mixed-200-2026-11-13/corpus-manifest.json');assert len(manifest)==200
 assert all(sha(ROOT/d['path'])==d['sha256'] for d in manifest)
 runs={n:read(OUT/n/'summary.json') for n in ['diagnostic','confirmation','streaming','xls-json']}
 for name,s in runs.items():
  assert [d['id'] for d in s['comparisons']['changed']]==[CHANGED]
  assert all(not x for x in s['comparisons']['within_profile_changes'].values())
  assert s['profiles']['after']['returned']==(18 if name=='xls-json' else 200)
 empty=hashlib.sha256(b'').hexdigest()
 for path in (OUT/'confirmation').glob('after-*.json'):
  rows=read(path);assert len(rows)==200 and all(r['ok'] and r['markdown_sha256']!=empty for r in rows)
 structured=read(OUT/'diagnostic/after-0.json');streamed=read(OUT/'streaming/after-0.json')
 assert [(r['id'],r['markdown_sha256']) for r in structured]==[(r['id'],r['markdown_sha256']) for r in streamed]
 a=read(OUT/'allocations-summary.json');assert a['profiles']['before']['max_parser_render_peak_bytes']==a['profiles']['after']['max_parser_render_peak_bytes']
 values=read(OUT/'workbook-values.json');assert values['nonempty_source_cells']==466 and not values['mismatches'] and not values['extra_output_cells']
 tests=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',(OUT/'tests.log').read_text())));assert tests==315
 old=read(OUT/'before/source-manifest.json');new=read(OUT/'after/source-manifest.json');changed=[p for p in sorted(old.keys()|new.keys()) if old.get(p)!=new.get(p)]
 assert all(sha(ROOT/p)==h for p,h in new.items())
 for folder,files in [('before',old),('after',new)]:assert all(sha(OUT/folder/'source-snapshot'/p)==h for p,h in files.items())
 patch=''
 for p in changed:
  left=(OUT/'before/source-snapshot'/p).read_text() if p in old else ''
  right=(OUT/'after/source-snapshot'/p).read_text() if p in new else ''
  patch+=''.join(difflib.unified_diff(left.splitlines(True),right.splitlines(True),fromfile='a/'+p,tofile='b/'+p))
 (OUT/'implementation.diff').write_text(patch)
 verification=dict(documents=200,nonempty_outputs=200,original_199_output_signatures_unchanged=True,streamed_and_structured_markdown_match=200,existing_xls_json_unchanged=17,recovered_xls_json_valid=True,recovered_workbook_nonempty_cells_verified=466,tests_passed=tests,changed_source_files=changed,source_snapshots_verified=True,current_source_matches_verified_build=True,input_hashes_unchanged=True,scope='Output signatures cover Markdown, statuses, diagnostics and asset metadata. Full JSON comparison is limited to XLS. Quality judging on the full mixed corpus remains pending.')
 (OUT/'verification.json').write_text(json.dumps(verification,indent=2)+'\n')
 conf=runs['confirmation']['profiles'];st=runs['streaming']['profiles'];diag=runs['diagnostic']['profiles']
 lines=['| Path / measurement | Before | After |','| --- | ---: | ---: |',f'| Nonempty outputs | 199/200 | **200/200** |',f'| In-memory → Markdown, median corpus time (5 passes) | {conf["before"]["seconds"]:.3f} s | {conf["after"]["seconds"]:.3f} s |',f'| CPU, median corpus time (5 passes) | {conf["before"]["cpu_seconds"]:.3f} s | {conf["after"]["cpu_seconds"]:.3f} s |',f'| Persistent-worker peak RSS, warm-up + 5 passes | {conf["before"]["peak_rss_bytes"]/2**20:,.1f} MiB | {conf["after"]["peak_rss_bytes"]/2**20:,.1f} MiB |',f'| Streamed Markdown, one corpus pass | {st["before"]["seconds"]:.3f} s | {st["after"]["seconds"]:.3f} s |',f'| Streaming worker peak RSS, one pass | {st["before"]["peak_rss_bytes"]/2**20:,.1f} MiB | {st["after"]["peak_rss_bytes"]/2**20:,.1f} MiB |',f'| Instrumented maximum live parser/render heap | {a["profiles"]["before"]["max_parser_render_peak_bytes"]:,} bytes | {a["profiles"]["after"]["max_parser_render_peak_bytes"]:,} bytes |']
 report=f'''# BIFF5/7 XLS compatibility and regression check

**200/200 nonempty outputs (100.0%) on the frozen 200-document, 19-format corpus.** The previous 199 successful outputs are unchanged. No measured speed or peak-memory regression was found in the matched runs below. This is a coverage and regression result, not a claim of perfect extraction or a new eight-engine quality ranking.

## General implementation

The XLS reader now accepts workbook BOF versions `0x0500` (BIFF5/7) and `0x0600` (BIFF8). Legacy sheet names, cell labels, rich-string text, custom number formats and cached formula strings use their correct byte-length fields and workbook code page. Legacy CONTINUE records contain raw text, including multibyte characters split between records. Numeric, date, merge, coordinate and streaming behavior reuses the existing implementation.

The workbook-global BOF selects the text layout: legacy producers can put a newer version in individual worksheet BOFs. An authored regression fixture covers this combination. There are no filename, hash, publisher, cell-marker or corpus-specific branches in parser code. No limits were raised, new runtime dependencies added or spreadsheet streaming paths removed.

Common Windows/Asian/Mac code pages and IBM866/UTF-8 are supported. An absent code page uses Windows-1252 with a notice. Unsupported declared code pages, malformed encoded text, encryption and resource-limit violations remain explicit errors. BIFF versions before 5 remain unsupported. BIFF5/7 inline comments are explicitly diagnosed as omitted; advanced visual formatting is not reconstructed. See the [format contract](../../docs/office-formats.md).

Format interpretation was checked against the vendored AnyDoc reader and installed xlrd; the implementation extends BulkParse's own bounded record/string reader. [Implementation diff](implementation.diff) · [Before sources](before/source-manifest.json) · [After sources](after/source-manifest.json).

## Matched results

Apple M3 Max, 36 GiB RAM, macOS; same input order and thread settings, sequential conversion, OCR off. First, one diagnostic pass compared the preserved before build, fixed build and unchanged AnyDoc. Once 200/200 coverage and output equivalence were confirmed, the two BulkParse builds received one excluded warm-up and five alternating measured passes. The headline time is the median **complete corpus time**, not a per-document median. Streaming was checked separately with one pass per build.

{chr(10).join(lines)}

The roughly {(1-conf['after']['seconds']/conf['before']['seconds'])*100:.1f}% timing difference is small and should not be marketed as a robust speed improvement. The useful conclusion is restored coverage with no detected slowdown. RSS varies between process lifetimes; compare matched scopes, not these repeated-run peaks with the original one-pass overview. All 199 previously successful documents retain identical Markdown, status, diagnostics and image-asset metadata, and every fixed-build confirmation pass is deterministic.

Peak RSS comes from macOS `/usr/bin/time -l`, includes worker initialization and output transport, and is not a simultaneous process-tree sum. Timing excludes worker startup and response transport, and includes reads, conversion and Markdown rendering. Output files are generated by the ordinary in-memory API; the separate writer profile includes Markdown file writes. Allocator instrumentation is run separately and never used for timing. Its maximum live parser/render heap and transport heap are identical before/after; total allocations include the newly recovered workbook and small per-process differences. [Allocator measurements](allocations-summary.json).

The unchanged AnyDoc control returned **{diag['anydoc']['returned']}/200** outputs in {diag['anydoc']['seconds']:.3f} seconds for one pass, including quick rejection of unsupported inputs. It does less successful work; this total is not an equal-workload comparison. The other six converters were not rerun for this parser-only change. Their original measurements, and BulkParse's original 99.5% coverage, remain preserved in the [eight-engine report](../mixed-200-2026-11-13/README.md).

## Recovered document and quality checks

The Department for Transport workbook is identified by xlrd as BIFF7/code page 1252. Its `Book` stream has global version `0x0500` and worksheet BOFs advertising `0x0600`.

An independent xlrd 2.0.2 check matched all **466 nonempty source cells** at their original sheet/row/column coordinates across **five visible worksheets**: **194 text cells, 242 numeric cells and 30 dates**, with no missing or extra output cells. Text comparison normalizes outer whitespace and CRLF only; numbers are compared exactly after removing currency/grouping notation, and dates against ISO dates. This verifies cell values, not visual layout. [Value-check evidence](workbook-values.json) · [Recovered full JSON](recovered-workbook.json).

The recovered document still has formatting-limit notices and an informational notice about retained hidden rows/columns. The CLI writes usable output and returns its normal partial-result exit code. Successful conversion is not a claim that every visual feature is reproduced.

## Validation

- **{tests} workspace/all-feature tests passed**, including general legacy-code-page, continuation, mixed-BOF, formula-cache, format/date/merge, encryption, malformed-input and budget tests.
- All **200** input hashes match the frozen corpus; all **199** previous successful output signatures are unchanged.
- Streamed and in-memory Markdown agree byte-for-byte for all **200** documents.
- Full structured JSON is unchanged for the **17** previously supported XLS files; all **18** XLS files now produce valid full JSON.
- Final source snapshots and preserved executable hashes are recorded. Early preflight artifacts are kept under `initial-*`; they are not the reported final results.

[Verification](verification.json) · [Initial final-build diagnostic](diagnostic/summary.json) · [Five-pass confirmation](confirmation/summary.json) · [Streaming](streaming/summary.json) · [XLS JSON check](xls-json/summary.json) · [Test log](tests.log).

## Reproduce

```sh
cargo test --locked --workspace --all-features
cargo build --release --locked -p bulkparse --example office_bench
python3 scripts/benchmark_office_speed.py \\
  --before reports/biff-legacy-2026-11-13/before-office-bench \\
  --after target/release/examples/office_bench \\
  --manifest reports/mixed-200-2026-11-13/corpus-manifest.json \\
  --passes 5 --warmup 1 --out reports/biff-recheck
# Add --stream and use --passes 1 --warmup 0 for the writer profile.
# Add --formats xls --verify-json for the full XLS JSON regression check.
```

Preserved binaries are macOS ARM64; other platforms must rebuild both source snapshots. The machine identity, commands, hashes and AC-power state for each run are recorded in its identity.json. The corpus downloader is `scripts/fetch_mixed200.py`. No paid judging jobs were submitted, and mixed-corpus quality judging remains pending.
'''
 (OUT/'README.md').write_text(report)
 note='**Current BulkParse follow-up: 100.0% nonempty outputs (200/200).** General BIFF5/7 XLS support resolves the one remaining failure, with unchanged prior outputs and no measured speed or peak-memory regression. [Matched before/after results](reports/biff-legacy-2026-11-13/README.md). The overview and format tables below preserve the original eight-engine run; they have not been mixed with these later measurements.'
 (OUT/'readme-note.md').write_text(note+'\n')
 dest=OUT/'reproduction-tools';dest.mkdir(exist_ok=True)
 for name in ['benchmark_office_speed.py','check_legacy_xls_values.py','profile_biff_allocations.py','report_biff_legacy.py']:shutil.copy2(ROOT/'scripts'/name,dest/name)
 print('Validated 200/200, 199 unchanged outputs, 466 independently checked cells, and unchanged maximum live heap.')
if __name__=='__main__':main()
