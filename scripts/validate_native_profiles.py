#!/usr/bin/env python3
"""Broader native OCR checks: authored multilingual/degraded scans and two unscored external cases."""
import argparse, hashlib, json, sys, time
from pathlib import Path
from evaluate_ocr import quality, text
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'bindings/python/python'))

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--config',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
    from bulkparse import OcrParser
    a.out.mkdir(parents=True,exist_ok=False)
    config=json.loads(a.config.read_text());rows=[]
    cases=json.loads((ROOT/'benchmarks/ocr-profiles/manifest.json').read_text())
    with OcrParser(config) as parser:
        for case in cases:
            file=ROOT/case['path'];assert hashlib.sha256(file.read_bytes()).hexdigest()==case['sha256']
            start=time.perf_counter();record={'id':case['id'],'track':case['track'],'language':case.get('language')}
            try:
                result=parser.parse_file(file,budget={'max_pages':2000,'max_output_bytes':33554432})
                record.update(seconds=time.perf_counter()-start,status=result['status'],nonempty=bool(result['markdown'].strip()),diagnostics=[d['code'] for d in result['diagnostics']])
                if case.get('reference'):
                    reference=ROOT/case['reference'];assert hashlib.sha256(reference.read_bytes()).hexdigest()==case['reference_sha256']
                    record['quality']=quality(text(result),reference.read_text())
                (a.out/(case['id']+'.json')).write_text(json.dumps(result,ensure_ascii=False))
            except Exception as error:record['error']=str(error)
            rows.append(record);print(json.dumps(record),flush=True)
            (a.out/'summary.json').write_text(json.dumps(rows,indent=2))
    (a.out/'identity.json').write_text(json.dumps({'config':config,'manifest_sha256':hashlib.sha256((ROOT/'benchmarks/ocr-profiles/manifest.json').read_bytes()).hexdigest(),'models':json.loads(Path(config['models']).read_text()),'scope':'24 authored/regression cases with references; two external smoke cases without scored reference. Existing development corpus, not unseen generalization evidence. One inference pass, model reuse; timing is not a controlled performance comparison.'},indent=2))
    if any('error' in r or 'pdf_ocr_error' in r.get('diagnostics',[]) or 'pdf_ocr_budget' in r.get('diagnostics',[]) for r in rows):raise SystemExit(1)
if __name__=='__main__':main()
