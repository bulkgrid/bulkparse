#!/usr/bin/env python3
"""Independently check corpus coverage, all predictions and reported medians."""
import hashlib,json,statistics
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];REPORT=ROOT/'reports/realworld-2026-11-12';RUN=REPORT/'conversion'
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 corpus=read(REPORT/'corpus-manifest.json');freeze=read(REPORT/'corpus-freeze.json');assert sha(REPORT/'corpus-manifest.json')==freeze['manifest_sha256'];assert sha(ROOT/'benchmarks/realworld/protocol.json')==freeze['protocol_sha256']
 assert len(corpus)==112 and len({r['sha256'] for r in corpus})==112
 assert all(sha(ROOT/r['path'])==r['sha256'] for r in corpus)
 identity=read(RUN/'identity.json');summary=read(RUN/'summary.json');assert identity['passes']==5 and identity['warmup']==1
 assert len(summary)==8
 predictions=0
 for e,s in summary.items():
  expected=[r['id'] for r in corpus if r['format'] in identity['support'][e]];rounds=[]
  for i in range(6):
   rows=read(RUN/f'{e}-round-{i}.json')['documents'];assert [r['id'] for r in rows]==expected
   folder=RUN/'prediction'/e/f'round-{i}';assert {p.stem for p in folder.glob('*.md')}==set(expected)
   for r in rows:
    p=folder/(r['id']+'.md');assert sha(p)==r['output_sha256'];assert p.stat().st_size==r['output_bytes'];assert r['elapsed_ns']>0
    if not r['ok']:assert p.stat().st_size==0
    predictions+=1
   rounds.append(rows)
  medians=[statistics.median(rs[j]['elapsed_ns']/1e6 for rs in rounds[1:]) for j in range(len(expected))]
  assert abs(statistics.median(medians)-s['median_ms'])<1e-9
  assert s['returned_all_passes']==sum(all(rs[j]['ok'] for rs in rounds[1:]) for j in range(len(expected)))
  assert len(s['documents'])==len(expected)
  for j,d in enumerate(s['documents']):assert d['id']==expected[j] and abs(d['median_ms']-medians[j])<1e-9
 for name,h in read(RUN/'source-manifest.json').items():assert sha(ROOT/name)==h and sha(RUN/'source-snapshot'/name)==h
 for e,h in identity['binary_sha256'].items():assert sha(Path(identity['commands'][e][0]))==h
 refs=read(REPORT/'reference-manifest.json');assert {r['id'] for r in refs}=={r['id'] for r in corpus}
 for r in refs:
  for im in r['images']:assert sha(ROOT/im['path'])==im['sha256']
  if r.get('text_path'):assert sha(ROOT/r['text_path'])==r['text_sha256']
 result={'validated':True,'corpus_documents':len(corpus),'converters':len(summary),'passes_per_converter':6,'prediction_files_verified':predictions,'sources_and_binaries_unchanged':True,'reference_hashes_verified':True,'quality_scores_verified_by_this_check':False}
 (REPORT/'independent-verification.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
if __name__=='__main__':main()
