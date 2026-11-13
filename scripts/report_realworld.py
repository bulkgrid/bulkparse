#!/usr/bin/env python3
"""Publish measured conversion results; quality stays pending until validated."""
import collections,json,statistics
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];REPORT=ROOT/'reports/realworld-2026-11-12';RUN=REPORT/'conversion'
NAMES={'bulkparse':'BulkParse 0.1.0','anydoc':'AnyDoc 0.2.4','docling':'Docling 2.150.0','markitdown':'MarkItDown 0.1.9','pandoc':'Pandoc 3.9','mammoth':'Mammoth 1.13.0','libreoffice':'LibreOffice 26.8 alpha + Pandoc','unstructured':'Unstructured 0.27.37'}
def write(p,x):p.write_text(json.dumps(x,indent=2,ensure_ascii=False)+'\n')
def main():
 s=json.loads((RUN/'summary.json').read_text());verified=json.loads((RUN/'verification.json').read_text());assert verified['complete']
 corpus=json.loads((REPORT/'corpus-manifest.json').read_text());counts=collections.Counter(r['format'] for r in corpus)
 table=['| Converter | Formats attempted | Returned / attempted | Median ms / document |','| --- | ---: | ---: | ---: |']
 for e,r in s.items():table.append(f'| {NAMES[e]} | {len(r["formats"])}/13 | {r["returned_all_passes"]}/{r["attempted"]} | {r["median_ms"]:.2f} |')
 common=[];bulk={d['id']:d for d in s['bulkparse']['documents']}
 for e,r in s.items():
  if e=='bulkparse':continue
  shared=[d for d in r['documents'] if d['returned']==5 and bulk[d['id']]['returned']==5]
  if not shared:continue
  bm=statistics.median(bulk[d['id']]['median_ms'] for d in shared);em=statistics.median(d['median_ms'] for d in shared)
  common.append({'comparator':e,'documents':len(shared),'bulkparse_median_ms':bm,'comparator_median_ms':em,'bulkparse_time_change_percent':100*(bm/em-1)})
 write(REPORT/'common-successful-subsets.json',common)
 failures={};partials={}
 for e,r in s.items():
  rows=json.loads((RUN/f'{e}-round-1.json').read_text())['documents'];failures[e]=[{'id':x['id'],'format':x['format'],'status':x.get('status'),'error':x.get('error')} for x in rows if not x['ok']]
  partials[e]=sum(x.get('status') in ['partial','partial_success'] for x in rows)
 write(REPORT/'conversion-failures.json',failures)
 lines=['# Independent real-world converter study','', '**Conversion measurements complete. Blind AI quality judging is pending cost approval; no quality scores are published.**','',
 '**112 public documents · 13 formats · eight converters · Apple M3 Max, 36 GiB RAM · November 12, 2026.** One excluded warm-up and five sequential measured passes, with rotating engine order. Parser sources and input hashes remained unchanged.','',
 'The corpus was selected before conversion from public publications and ebooks, with a fixed seed and checks against recorded development inputs. It is predominantly English and public-sector material, with sparse presentation formats and no DOCM. It is not a random customer sample or a reproduction of AnyDoc’s private benchmark.','',
 '## Conversion timing and coverage','',*table,'',
 '**Do not rank these medians as equivalent workloads:** tools cover different inputs, and failed attempts remain in timing. “Returned” means output was returned in all five measured passes; it does not mean the extraction was complete or correct. Each median is taken across documents after taking each document’s median of five timings.','',
 'Initialization is excluded. Reads and Markdown export are included. Pandoc/LibreOffice command launches are included; output serialization and benchmark file writes are excluded. A 90-second deadline applies to every attempted conversion. Default resource limits remain enabled.','',
 '## Identical successful subsets','',
 'These compare identical documents that both engines returned in every measured pass. This isolates successful-conversion speed, but excludes failure cases and must be read together with the coverage table.','',
 '| Compared with BulkParse | Common successful inputs | BulkParse median ms | Comparator median ms |','| --- | ---: | ---: | ---: |']
 for r in common:lines.append(f'| {NAMES[r["comparator"]]} | {r["documents"]} | {r["bulkparse_median_ms"]:.2f} | {r["comparator_median_ms"]:.2f} |')
 lines+=['','## Reliability and limitations','',f'BulkParse returned output on **{s["bulkparse"]["returned_all_passes"]}/112** inputs; **{partials["bulkparse"]}** round-one outputs were marked partial. These are status reports, not AI quality scores.','']
 for e in s:
  errors=collections.Counter((x['error'] or 'Unspecified error').splitlines()[0][:180] for x in failures[e]);lines.append(f'- **{NAMES[e]}:** {len(failures[e])} failed attempts in measured pass one; {partials[e]} returned partial-status outputs. '+('; '.join(f'{n} × {err}' for err,n in errors.items()) if errors else 'No conversion errors in that pass.'))
 lines+=['','Failure messages and every per-pass outcome are retained in [conversion-failures.json](conversion-failures.json) and the [raw conversion directory](conversion/). Unsupported formats are excluded, while supported-format failures remain empty outputs for judging. All five passes are reported; no favorable pass was selected.','',
 '## Per-format timing','', '| Format (inputs) | '+' | '.join(NAMES[e] for e in s)+' |','| --- | '+' | '.join('---:' for _ in s)+' |']
 for f,n in sorted(counts.items()):lines.append('| '+f.upper()+f' ({n}) | '+' | '.join(f'{s[e]["formats"][f]["median_ms"]:.2f} ({s[e]["formats"][f]["returned_all_passes"]}/{n})' if f in s[e]['formats'] else '—' for e in s)+' |')
 lines+=['','Cells show median milliseconds per attempted document and returned/attempted counts. A dash means unsupported in the configured adapter.','',
 '## Resources and stability','', '| Converter | Recorded peak RSS MiB | Worker starts | Stable output hashes across measured passes |','| --- | ---: | ---: | --- |']
 for e,r in s.items():
  mem=r['resources']['lifetime_peak_rss_bytes'];lines.append(f'| {NAMES[e]} | {mem/2**20:.1f}'+' | '+str(r['resources']['worker_starts'])+' | '+('Yes' if r['deterministic'] else 'No; all outputs retained')+' |' if mem else f'| {NAMES[e]} | unavailable | {r["resources"]["worker_starts"]} | {r["deterministic"]} |')
 lines+=['','Memory is a worker-lifetime peak, not summed process-tree memory. Rows with more than one worker start have incomplete memory sampling because killed workers may not report their peaks; those values are lower bounds and must not support a memory ranking. Source snapshots, binary hashes, dependency versions, effective options and machine details are in [conversion/](conversion/).','',
 '## Blind quality evaluation: pending','',
 'Planned dimensions are completeness, structure, formatting and cleanliness, scored 0–100 by Astra against independent reference pages/records. Tool names are hidden, each pair is judged in both orders, and valid HTML tables receive the same eligibility for credit as Markdown tables. No score is inferred from speed, output length or parser status.','',
 'References cover the first six print pages/slides, first six reflowed EPUB pages, or first 60 CSV records. Spreadsheet print settings can produce covers/fragments; later sheets/pages are outside scope. Raw candidate outputs remain intact, with recorded 40,000-character presentation limits for judging. Unjudgeable cases and position disagreements will be reported. These are reference renderings and AI estimates, not independent human ground truth.','',
 'Quality rows will be published only after both orders return valid verdicts. Overall quality will macro-average formats; supported-format coverage and per-format scores will be shown alongside it. Different supported format mixes prevent a universal overall ranking.','',
 '## Reproduction and evidence','',
 '- [Frozen protocol and instructions](../../benchmarks/realworld/README.md)','- [Source manifest](corpus-manifest.json) and [selection freeze](corpus-freeze.json)','- [Reference manifest](reference-manifest.json) and [seeded visual review sample](reference-audit-contact-sheet.png)','- [Conversion summary](conversion/summary.json), [effective options](conversion/effective-options.json), [identity and binary hashes](conversion/identity.json), [source hashes](conversion/source-manifest.json)','',
 'An initial warm-up was aborted before measured passes to replace inefficient benchmark response buffering for large outputs. It is preserved in `conversion-warmup-aborted/` and contributes no results. No parser implementation or corpus selection changed. Input redistribution is not promised; retrieve originals from the manifest URLs and verify hashes.','']
 (REPORT/'README.md').write_text('\n'.join(lines))
 section=['<!-- realworld-benchmark:start -->','### Independent real-world Office and EPUB study','',
 '**112 public documents across 13 formats · eight converters.** Selected before conversion, checked against recorded development inputs, then measured on the same machine with one excluded warm-up and five sequential passes.','',*table,'',
 '**Coverage matters as much as speed.** These rows cover different supported inputs, and failed attempts remain in timing; they are not an equivalent-workload speed ranking. “Returned” requires output in all five measured passes and does not certify quality.','',
 f'BulkParse returned output on **{s["bulkparse"]["returned_all_passes"]}/112** documents. The failures expose remaining encoding and resource-limit gaps that the smaller development fixtures did not capture. The corpus is mostly public-sector material and classic literature; presentation formats are sparse and DOCM is absent.','',
 '**Blind quality scores are pending judging-cost approval.** The planned report scores completeness, structure, formatting and cleanliness against source references, with tool names hidden and A/B positions swapped. We will publish measured scores, not estimates inferred from conversion success.','',
 '[Full results, identical-subset comparisons and failures](reports/realworld-2026-11-12/README.md) · [Corpus, methodology and reproduction](benchmarks/realworld/README.md)','<!-- realworld-benchmark:end -->','']
 readme=ROOT/'README.md';text=readme.read_text();start='<!-- realworld-benchmark:start -->';end='<!-- realworld-benchmark:end -->'
 if start in text:text=text[:text.index(start)]+'\n'.join(section)+text[text.index(end)+len(end):]
 else:text=text.replace('### Office and EPUB benchmark','\n'.join(section)+'\n### Office and EPUB development benchmark')
 readme.write_text(text)
if __name__=='__main__':main()
