#!/usr/bin/env python3
"""Verify the frozen corpus, raw outcomes and retained-document output regression."""
import collections,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];BASE=ROOT/'reports/mixed-200-2026-11-13';RUN=BASE/'run'
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 corpus=read(BASE/'corpus-manifest.json');old=read(ROOT/'reports/realworld-2026-11-12/corpus-manifest.json');hard=read(ROOT/'benchmarks/realworld-failures/manifest.json')
 assert len(corpus)==len({r['id'] for r in corpus})==len({r['sha256'] for r in corpus})==200
 byid={r['id']:r for r in corpus}
 assert all(all(byid[r['id']][k]==r[k] for k in ['sha256','path','format','bytes']) for r in old)
 assert all(r['id'] in byid for r in hard) and len(hard)==27
 assert all(sha(ROOT/r['path'])==r['sha256'] for r in corpus)
 freeze=read(BASE/'corpus-freeze.json');identity=read(RUN/'identity.json')
 assert sha(BASE/'corpus-manifest.json')==freeze['manifest_sha256']
 assert freeze['frozen_utc']<identity['started_utc']
 assert read(RUN/'verification.json')['complete']
 rows=read(RUN/'bulkparse-round-1.json')['documents'];current={r['id']:r for r in rows};before=read(ROOT/'reports/epub-speed-2026-11-13/full-regression/after-0.json')
 assert len(before)==112
 mismatch=[]
 for r in before:
  now=current[r['id']];fields=[]
  if r['markdown_sha256']!=now['output_sha256']:fields.append('Markdown bytes')
  for key in ['ok','status','diagnostics','render_diagnostics','image_count','image_bytes','image_assets']:
   if r.get(key)!=now.get(key):fields.append(key)
  if fields:mismatch.append(dict(id=r['id'],fields=fields))
 diagnostics={category:sorted({r['id'] for r in rows for d in [*r.get('diagnostics',[]),*r.get('render_diagnostics',[])] if d.get('category')==category}) for category in ['missing_content','formatting_limitation','informational']}
 codes=collections.Counter(d['code'] for r in rows for d in [*r.get('diagnostics',[]),*r.get('render_diagnostics',[])])
 result=dict(unique_inputs=200,retained_unchanged_inputs=112,historical_failures_retained=27,frozen_before_run=True,all_input_hashes_verified=True,retained_output_regression=dict(compared=112,unchanged=112-len(mismatch),mismatches=mismatch,scope='Markdown bytes, parser/render diagnostics, status, image asset metadata; full structured JSON is not separately serialized in this run'),diagnostic_document_ids=diagnostics,diagnostic_code_counts=dict(codes),note='Diagnostics are parser-reported; they are not independently judged content loss or quality scores.')
 (BASE/'regression-check.json').write_text(json.dumps(result,indent=2)+'\n')
 assert not mismatch,f'Retained output regressions: {mismatch}'
 print('Verified 200 unique unchanged inputs, all 27 historical failures retained, and all 112 previous Markdown/diagnostic/asset results unchanged.')
if __name__=='__main__':main()
