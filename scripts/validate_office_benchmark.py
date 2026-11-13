#!/usr/bin/env python3
"""Check corpus/source identity, failure accounting and repeated measurements."""
import argparse,hashlib,json,statistics
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    ap=argparse.ArgumentParser();ap.add_argument('run',type=Path);a=ap.parse_args();p=a.run
    manifest=json.loads((p/'manifest.json').read_text());summary=json.loads((p/'summary.json').read_text())
    assert len({r['path'] for r in manifest})==len(manifest)
    for r in manifest:assert sha(ROOT/r['path'])==r['sha256'],r['path']
    for name,digest in json.loads((p/'source-manifest.json').read_text()).items():assert sha(ROOT/name)==digest,name
    for engine,aggregate in summary['engines'].items():
        expected=[r['path'] for r in manifest if r['track']!='robustness' and (engine!='docling' or r['format'] in ('xlsx','xlsm','pptx','pptm','ppsx','ppsm'))]
        runs=[json.loads((p/f'{engine}-pass-{i}.json').read_text()) for i in range(1,6)]
        for run in runs:
            assert [r['path'] for r in run['documents']]==expected
            assert all(r['elapsed_ns']>0 and (r['ok'] or (r.get('error') and r['markdown']=='')) for r in run['documents'])
            assert sum(r['elapsed_ns'] for r in run['documents'])<=run['wall_ns']
            assert [(r['ok'],r['markdown']) for r in run['documents']]==[(r['ok'],r['markdown']) for r in runs[0]['documents']]
        assert aggregate['attempted']==len(expected)
        assert aggregate['returned']==sum(r['ok'] for r in runs[0]['documents'])
        median=statistics.median(sum(r['elapsed_ns'] for r in run['documents'])/1e6 for run in runs)
        assert abs(median-aggregate['median_corpus_ms'])<1e-9
        assert summary['resources'][engine]['exit']==0
    result={'passed':True,'corpus_inputs':len(manifest),'measured_passes_per_engine':5,'source_hashes_verified':True,'all_outcomes_and_predictions_stable':True}
    (p/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
if __name__=='__main__':main()
