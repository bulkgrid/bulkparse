#!/usr/bin/env python3
"""Recompute first-pass tables from raw per-document records, without quality scores."""
import collections, hashlib, json, statistics, math
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];BASE=ROOT/'reports/mixed-200-2026-11-13';RUN=BASE/'run'
ENGINES=['bulkparse','anydoc','docling','markitdown','pandoc','mammoth','libreoffice','unstructured']
NAMES=dict(zip(ENGINES,['BulkParse','AnyDoc','Docling','MarkItDown','Pandoc','Mammoth','LibreOffice','Unstructured']))
def read(p):return json.loads(p.read_text())
def write(p,x):p.write_text(json.dumps(x,indent=2,ensure_ascii=False)+'\n')
def good(r):return r['ok'] and r['output_bytes']>0
def med(rs):return statistics.median(r['elapsed_ns']/1e6 for r in rs) if rs else None
def fmt(v,n=2):return f'{v:,.{n}f}' if v is not None else '—'
def main():
 followup_path=ROOT/'reports/biff-legacy-2026-11-13/readme-note.md'
 followup=followup_path.read_text().strip() if followup_path.exists() else ''
 assert read(RUN/'verification.json')['complete']
 manifest=read(BASE/'corpus-manifest.json');identity=read(RUN/'identity.json');resources=read(RUN/'resources.json');options=read(RUN/'effective-options.json');counts=collections.Counter(d['format'] for d in manifest);cohort={d['id']:d['cohort'] for d in manifest}; inputs={d['id']:d for d in manifest}; regression=read(BASE/'regression-check.json')
 assert len(manifest)==200 and identity['measured_passes']==1 and identity['warmup']==0
 raw={e:read(RUN/f'{e}-round-1.json')['documents'] for e in ENGINES}
 for e,rows in raw.items():
  assert len(rows)==200 and [r['id'] for r in rows]==[d['id'] for d in manifest]
  for r in rows:
   p=RUN/'prediction'/e/'round-1'/(r['id']+'.md')
   assert hashlib.sha256(p.read_bytes()).hexdigest()==r['output_sha256']
 stats={};failures={};format_stats={}
 for e,rows in raw.items():
  attempted=[r for r in rows if r.get('status')!='unsupported'];ok=[r for r in rows if good(r)];errors=[r for r in attempted if not r['ok']]
  stats[e]=dict(attempted=len(attempted),returned=sum(r['ok'] for r in rows),nonempty=len(ok),unsupported=200-len(attempted),failed=len(errors),empty=sum(r['ok'] and not r['output_bytes'] for r in rows),formats=len({r['format'] for r in ok}),median_success_ms=med(ok),p95_success_ms=sorted(r['elapsed_ns']/1e6 for r in ok)[math.ceil(.95*len(ok))-1] if ok else None,attempt_seconds=sum(r['elapsed_ns'] for r in attempted)/1e9,known_cpu_seconds=sum(r.get('cpu_seconds') or 0 for r in attempted),cpu_incomplete=any(r.get('cpu_seconds') is None for r in attempted),peak_worker_mib=(resources[e]['lifetime_peak_rss_bytes']/2**20 if resources[e]['lifetime_peak_rss_bytes'] else None),memory_incomplete=any(r.get('status')=='timeout_or_worker_failure' for r in rows),cohorts={c:dict(nonempty=sum(good(r) for r in rows if cohort[r['id']]==c),total=sum(cohort[r['id']]==c for r in rows)) for c in ['retained-112','added-88']},statuses=dict(collections.Counter(r.get('status','returned' if r['ok'] else 'error') for r in rows)))
  failures[e]=[dict(id=r['id'],format=r['format'],title=inputs[r['id']]['title'],source_url=inputs[r['id']]['url'],cohort=cohort[r['id']],elapsed_ms=r['elapsed_ns']/1e6,status=r.get('status'),error=r.get('error'),empty=r['ok'] and not r['output_bytes']) for r in attempted if not good(r)]
  format_stats[e]={f:dict(total=counts[f],attempted=sum(r.get('status')!='unsupported' for r in rows if r['format']==f),nonempty=sum(good(r) for r in rows if r['format']==f),median_success_ms=med([r for r in ok if r['format']==f])) for f in sorted(counts)}
 byid={e:{r['id']:r for r in rows} for e,rows in raw.items()};common=set.intersection(*[{r['id'] for r in rows if good(r)} for rows in raw.values()]);pairs={}
 for e in ENGINES[1:]:
  ids=sorted({r['id'] for r in raw['bulkparse'] if good(r)} & {r['id'] for r in raw[e] if good(r)})
  pairs[e]=dict(ids=ids,documents=len(ids),bulkparse_seconds=sum(byid['bulkparse'][i]['elapsed_ns'] for i in ids)/1e9,competitor_seconds=sum(byid[e][i]['elapsed_ns'] for i in ids)/1e9)
 same={e:dict(documents=len(common),total_seconds=sum(byid[e][i]['elapsed_ns'] for i in common)/1e9,median_ms=med([byid[e][i] for i in common])) for e in ENGINES}
 write(BASE/'results.json',dict(overall=stats,formats=format_stats,common_all_ids=sorted(common),common_all=same,pairwise_common=pairs,quality='not assessed; no quality scores'))
 write(BASE/'failures.json',failures)
 table=['| Engine | Nonempty outputs | Formats with output | Median ms¹ | Attempt time s¹ | CPU s² | Peak RSS MiB³ |','| --- | ---: | ---: | ---: | ---: | ---: | ---: |']
 for e in ENGINES:
  s=stats[e];cpu=('≥ ' if s['cpu_incomplete'] else '')+fmt(s['known_cpu_seconds']);rss=('≥ ' if s['memory_incomplete'] else '')+fmt(s['peak_worker_mib'],1)
  table.append(f'| {NAMES[e]} | {s["nonempty"]}/{len(manifest)} | {s["formats"]}/{len(counts)} | {fmt(s["median_success_ms"])} | {fmt(s["attempt_seconds"])} | {cpu} | {rss} |')
 table='\n'.join(table)
 notes='''¹ Unsupported means outside the configured adapter’s format profile, not the engine’s complete capability list. Failed attempts include unsupported variants within an attempted extension. Median is across nonempty outputs from one conversion per document. Attempt time includes failed attempts and excludes unsupported inputs. Each engine has a different successful subset: these totals and medians are **not an equal-workload ranking**. No full-corpus warm-up or repeated timing passes were run; a small adapter preflight preceded fresh workers. Worker startup is excluded, first-use work remains included, and required CLI launches are included. Engine order and OS cache effects are not averaged out in one pass.

² CPU includes the worker and completed child processes. “≥” means a killed worker prevented complete CPU accounting.

³ macOS `/usr/bin/time -l` reports high-water RSS, including initialization, output transport and waited-for child-process peaks. It is **not the simultaneous sum of the process tree**. “≥” means killed workers may lack memory samples. Different workloads and model imports affect these values; no whole-system memory winner is claimed.'''
 per=['| Format (documents) | '+' | '.join(NAMES[e] for e in ENGINES)+' |','| --- | '+' | '.join(['---:']*8)+' |']
 for f,n in sorted(counts.items()):
  cells=[]
  for e in ENGINES:
   d=format_stats[e][f];cells.append('—' if not d['attempted'] else f'{d["nonempty"]}/{n} · {fmt(d["median_success_ms"])}')
  per.append(f'| {f} ({n}) | '+' | '.join(cells)+' |')
 per='\n'.join(per)
 cohorts=['| Engine | Retained 112 | Added 88 |','| --- | ---: | ---: |']
 for e in ENGINES:cohorts.append(f'| {NAMES[e]} | {stats[e]["cohorts"]["retained-112"]["nonempty"]}/112 | {stats[e]["cohorts"]["added-88"]["nonempty"]}/88 |')
 pair_table=['| Comparator | Common nonempty documents | BulkParse s | Comparator s |','| --- | ---: | ---: | ---: |']
 for e,x in pairs.items():pair_table.append(f'| {NAMES[e]} | {x["documents"]} | {fmt(x["bulkparse_seconds"],3)} | {fmt(x["competitor_seconds"],3)} |')
 tails=['| Engine | Nonempty timing denominator | p95 ms (nearest rank) |','| --- | ---: | ---: |']
 for e in ENGINES:tails.append(f'| {NAMES[e]} | {stats[e]["nonempty"]} | {fmt(stats[e]["p95_success_ms"])} |')
 shared=['| Engine | Time on shared documents, s | Median ms |','| --- | ---: | ---: |']
 for e in ENGINES:shared.append(f'| {NAMES[e]} | {fmt(same[e]["total_seconds"],3)} | {fmt(same[e]["median_ms"])} |')
 versions={'bulkparse':'0.1.0 (preserved build; SHA-256 in run/identity.json)','anydoc':'0.2.4 (vendored source snapshot)'}
 for e in ENGINES[2:]:versions[e]=options[e]['versions']
 write(BASE/'versions.json',versions)
 report=f'''# Mixed-document comparison: first diagnostic pass

200 documents · 19 formats · eight engines · November 13, 2026 · Apple M3 Max, 36 GiB RAM.

{followup.replace('](reports/', '](../')}

This is **one full diagnostic pass**, not a five-pass performance release or a quality leaderboard. OCR is off. BulkParse source was not modified during this task.

## Overall results

{table}

Nonempty output counts use all 200 documents as the denominator. Formats with output counts extensions with at least one nonempty result.

{notes}

Nonempty output is a coverage measure, not proof that all content or formatting survived. Empty returned outputs, if any, are separate from failures in [results.json](results.json). Engine status conventions differ; BulkParse partial/warning states cannot be directly compared with tools that return a bare string.

## Old and new documents

{chr(10).join(cohorts)}

The retained group includes all 27 historical failure-regression inputs. These 112 documents informed prior implementation work. The 88 additions were selected before conversion; the combined dataset is a **development benchmark**, not an unseen holdout.

## Results by format

Each cell is **nonempty outputs / corpus documents · median successful conversion milliseconds**. A dash means unsupported by the configured adapter. Formats represented by one or a few files are smoke coverage, not representative performance estimates. Successful subsets within a format can differ.

{per}

## Identical-input comparison

All eight engines returned nonempty output on **{len(common)} common documents**, with formats {', '.join(sorted({byid['bulkparse'][i]['format'] for i in common}))}. This narrow overlap is not representative of the whole workload, and equal inputs do not establish equal extraction quality.

{chr(10).join(shared)}

For broader pairwise comparisons, each row below uses the **same files for both engines**. Different rows use different intersections; excluded failures still count against coverage in the overall table. Equal inputs do not establish equal output quality.

{chr(10).join(pair_table)}

The exact document IDs are retained in [results.json](results.json). Failed and unsupported documents are never counted as fast conversions.

## Tail latency

One observation per successful document, nearest-rank p95. Different successful subsets prevent an equal-workload ranking. Failure latencies remain in the raw records and overall attempted totals.

{chr(10).join(tails)}

## Regression and diagnostics

The retained-output check compared Markdown bytes, status, parser/render diagnostics and image-asset metadata with the previous preserved build: **{regression['retained_output_regression']['unchanged']}/112 unchanged**. All 27 historical failure-regression inputs remain in the corpus. [Verification details](regression-check.json). This does not independently validate extraction quality.

BulkParse's reported status counts are {stats['bulkparse']['statuses']}. Documents with reported missing-content diagnostics: **{len(regression['diagnostic_document_ids']['missing_content'])}**; formatting-limit diagnostics: **{len(regression['diagnostic_document_ids']['formatting_limitation'])}**; informational notices: **{len(regression['diagnostic_document_ids']['informational'])}**. These categories can overlap, include uncertain warnings, and are not independently confirmed loss counts.

## Problems found

BulkParse returned **112/112 retained documents and 87/88 additions**. Its one new failure is the Department for Transport discretionary-spend workbook, an older **BIFF5/7 XLS** (`Book` stream, BOF version `0x0500`); the current implementation accepts BIFF8 only. Other converters returned output for it. It remains in the corpus and counts as a failed attempt. [Source-level diagnosis](legacy-xls-diagnosis.json). No production parser changes were made during this task.

[failures.json](failures.json) lists every failed or empty attempt, including deadlines. Raw statuses and diagnostics are in the per-engine round files. Quality is **not assessed** in this pass: no completeness, structure, formatting or cleanliness scores are fabricated. The previous unfinished quality review is not reused as a current score.

## Corpus and selection

[Manifest](corpus-manifest.json) · [Freeze and counts](corpus-freeze.json) · [Candidate pool](candidate-pool.json) · [Exclusions](corpus-exclusions.json).

The 200 files total {sum(d['bytes'] for d in manifest)/2**20:.1f} MiB. SHA-256 ranking within formats, one document per publication landing page, publisher cap 24 and a 15 MiB individual download cap were applied before conversion. Exact hashes and Office package content were checked for duplicates; added files were checked against recorded prior-input hashes. All original 112 files were retained unchanged. Actual counts replace the planned target counts: rare-format shortages were redistributed before running converters. DOCM/PPT/TSV/TXT each have only one file; XLSB/PPS/POT/PPTM/PPSX/PPSM have none.

Sources are predominantly UK government publications, plus Gutenberg books, open-source README documents, Gapminder data and four related Python Data Science Handbook chapters. Government monthly-series documents and related notebook chapters are not statistically independent. The corpus is not a random sample of customer uploads. Source URLs, publisher and license notes are in the manifest; input files are not redistributed with the report. Download changes or withdrawal can prevent exact reproduction; the fetch script rejects changed bytes.

## Configuration and reproducibility

[Versions](versions.json) · [Memory-accounting validation](memory-accounting.json) · [Model file hashes](model-manifest.json) · [Effective options](run/effective-options.json) · [Machine, commands and executable hashes](run/identity.json) · [Verified source snapshot](run/source-manifest.json) · [Verification](run/verification.json).

The same frozen manifest order is used for every engine; converters and documents execute sequentially. This pass ran AnyDoc, Docling, MarkItDown, Pandoc, Mammoth, LibreOffice, Unstructured, then BulkParse. OS file caches were not reset, so order/cache effects are another reason not to treat this single pass as a stable speed ranking. A 90-second per-document wall deadline includes response transport; a timeout kills the worker group and starts a new worker for the next input. Native/Python persistent-worker initialization is excluded from conversion timings. Reads and Markdown generation are included. Large output transport is excluded from internal successful conversion timing but contributes to worker memory. Main BulkParse uses its structured parser followed by Markdown rendering with default asset retention, without the separate streaming or full-JSON verification profiles. AnyDoc uses its standard Markdown API. Assets are not written to disk in this comparison.

Docling uses CPU PDF layout/table models (accurate tables, one configured thread), with OCR, enrichment and remote services disabled; its PDF pipeline initializes before the ready signal. Other formats use installed defaults. Unstructured PDF uses text-only `fast`; MarkItDown has no LLM client or plugins. Pandoc outputs GFM, Mammoth HTML is rendered via markdownify, and LibreOffice uses isolated profiles with macro execution restricted and HTML export converted to GFM. Environment thread settings and dependency versions are recorded. Defaults can still use helper processes; this is not a strict single-core enforcement experiment.

Preflight results are separate from the full pass. Missing Unstructured PDF/Markdown dependencies were repaired before the full pass; its version is pinned at 0.27.37. No paid judging jobs were submitted.

```sh
python3 scripts/fetch_mixed200.py
python3 scripts/benchmark_mixed200.py --manifest reports/mixed-200-2026-11-13/corpus-manifest.json --out reports/mixed200-rerun --bulkparse-binary reports/mixed-200-2026-11-13/bulkparse-worker
```

The preserved executable targets this Mac; other platforms must build the snapshotted Rust source and supply that executable. Install Python requirements from the three run/*-requirements.txt files and provision the Docling models recorded in the effective settings. The harness is macOS-specific for OS high-water memory counters. Repeated passes require explicit `--final-validation --passes 5 --warmup`; they were not run here.
'''
 (BASE/'README.md').write_text(report)
 # Publish the current BulkParse diagnostic pass as a complete row and column,
 # keeping the original eight-engine evidence above unchanged.
 current=ROOT/'reports/biff-legacy-2026-11-13'
 checked=read(current/'verification.json')
 diagnostic=current/'diagnostic'
 current_manifest=read(diagnostic/'manifest.json')
 assert [(d['id'],d['sha256']) for d in current_manifest]==[(d['id'],d['sha256']) for d in manifest]
 current_rows=read(diagnostic/'after-0.json')
 assert [r['id'] for r in current_rows]==[d['id'] for d in manifest]
 empty_hash=hashlib.sha256(b'').hexdigest()
 assert all(r['ok'] and r['markdown_sha256'] not in (None,empty_hash) for r in current_rows)
 assert checked['nonempty_outputs']==len(current_rows)==200
 current_identity=read(diagnostic/'identity.json')
 assert current_identity['passes']==1 and current_identity['warmup']==0
 profile=read(diagnostic/'summary.json')['profiles']['after']
 current_line=f'| BulkParse | {len(current_rows)}/{len(manifest)} | {len({r["format"] for r in current_rows})}/{len(counts)} | {fmt(med(current_rows))} | {fmt(sum(r["elapsed_ns"] for r in current_rows)/1e9)} | {fmt(sum(r["cpu_seconds"] for r in current_rows))} | {fmt(profile["peak_rss_bytes"]/2**20,1)} |'
 table='\n'.join(current_line if line.startswith('| BulkParse |') else line for line in table.splitlines())
 current_formats={f:[r for r in current_rows if r['format']==f] for f in counts}
 per_lines=per.splitlines()
 for i,(f,n) in enumerate(sorted(counts.items()),start=2):
  cells=per_lines[i].split('|')
  cells[2]=f' {len(current_formats[f])}/{n} · {fmt(med(current_formats[f]))} '
  per_lines[i]='|'.join(cells)
 per='\n'.join(per_lines)
 section=f'''### Overall engine comparison

**Tested on 200 real-world documents across 19 formats and eight engines.** One diagnostic pass on an Apple M3 Max with 36 GiB RAM, November 13, 2026. OCR was disabled.

BulkParse’s row and per-format results use its [current-build diagnostic pass](reports/biff-legacy-2026-11-13/diagnostic/summary.json), run later the same day on the identical corpus. The other engines use the [eight-engine comparison run](reports/mixed-200-2026-11-13/README.md). Each displayed measurement comes from one pass; these were separate runs, not a simultaneous retest of all engines.

{table}

Nonempty output counts use all 200 documents as the denominator. Formats with output counts extensions with at least one nonempty result.

{notes}

**Quality review is pending.** Nonempty output measures coverage, not extraction accuracy. The planned assessment compares completeness, structure, formatting and cleanliness against source references on the same 40-document sample across engines. Overall scores will use equal format weights, with failed and unsupported inputs counted as zero; supported-only quality will be reported separately. This will be labelled assistant-reviewed, not independent blind judging. [Quality review protocol](benchmarks/realworld/README.md).

The corpus includes 112 development documents and 88 selected before conversion, so it is not an entirely unseen test set. [Full methodology, configurations and raw results](reports/mixed-200-2026-11-13/README.md).

### Results by format

The same 200 documents and engine runs shown above underpin this breakdown. Cells show **nonempty outputs / documents · median successful conversion ms**; `—` means unsupported. Tiny samples and different successful subsets limit speed comparisons.

{per}

[Identical-input comparisons and exact document IDs](reports/mixed-200-2026-11-13/README.md#identical-input-comparison) · [Detailed conversion outcomes](reports/mixed-200-2026-11-13/failures.json)

'''
 p=ROOT/'README.md';s=p.read_text();start=s.index('### Overall engine comparison');end=s.index('### PDF inspection and extraction',start);p.write_text(s[:start]+section+s[end:])
 print(table);print('Updated report and README')
if __name__=='__main__':main()
