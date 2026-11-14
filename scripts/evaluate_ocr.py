#!/usr/bin/env python3
"""Evaluate actual optional OCR against frozen original expectations, no output repair."""
from bulkparse_cli import parse_command
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import time
import unicodedata

ROOT=Path(__file__).resolve().parents[1]


def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def normalized(text): return ' '.join(unicodedata.normalize('NFC',text).split())


def distance(left,right):
    row=list(range(len(right)+1))
    for i,a in enumerate(left,1):
        new=[i]
        for j,b in enumerate(right,1): new.append(min(new[-1]+1,row[j]+1,row[j-1]+(a!=b)))
        row=new
    return row[-1]


def text(document):
    # This OCR corpus contains paragraph text only; preserve newlines and order.
    return '\n'.join(''.join(i['text'] for i in b.get('inlines',[]) if i['type']=='text')
                     for b in document['document']['blocks'])


def quality(actual,reference):
    a,b=normalized(actual),normalized(reference)
    lines=[normalized(line) for line in reference.splitlines() if line.strip()]
    positions=[a.find(line) for line in lines]
    pairs=len(lines)*(len(lines)-1)//2
    return {'exact_after_whitespace':a==b,
            'word_error_rate':distance(b.split(),a.split())/len(b.split()) if b else (0.0 if not a else None),
            'character_error_rate':distance(b,a)/len(b) if b else (0.0 if not a else None),
            'exact_line_pair_order':sum(0<=positions[i]<positions[j] for i in range(len(lines)) for j in range(i+1,len(lines)))/pairs if pairs else None}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--config',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    args.out.mkdir(parents=True,exist_ok=False)
    manifest=ROOT/'benchmarks/ocr/manifest.json'
    cases=json.loads(manifest.read_text())
    config=json.loads(args.config.read_text())
    models=json.loads(Path(config['models']).read_text())
    for model in models['models'].values(): assert sha(model['path'])==model['sha256']
    metadata={'created_utc':datetime.now(timezone.utc).isoformat(),'manifest_sha256':sha(manifest),
              'binary_sha256':sha(args.binary),'config':config,'models':models,
              'source_sha256':{str(p.relative_to(ROOT)):sha(p) for p in [
                  ROOT/'Cargo.lock',ROOT/'crates/bulkparse-ocr-native/Cargo.toml',Path(__file__).resolve(),
                  ROOT/'scripts/provision_native_ocr.mjs',
                  *sorted((ROOT/'crates/bulkparse-ocr-native').rglob('*.rs')),
                  ROOT/'crates/bulkparse/src/formats/pdf.rs',ROOT/'crates/bulkparse/src/ocr.rs',
                  ROOT/'crates/bulkparse/src/ocr/layout.rs',ROOT/'crates/bulkparse/src/model.rs',
                  ROOT/'scripts/check_ocr_results.py',ROOT/'crates/bulkparse-cli/src/main.rs']},
              'scope':'Eight original PDF cases: six newly authored and two existing regression cases. One run per profile; CPU OCR only. Metrics are NFC/whitespace-normalized, case/punctuation-sensitive; no spelling fixes or document-specific replacements. Not a real-world scan accuracy benchmark.'}
    (args.out/'run.json').write_text(json.dumps(metadata,indent=2)+'\n')
    records=[]
    for case in cases:
        source=ROOT/case['path']; ref=ROOT/case['reference']
        assert sha(source)==case['sha256'] and sha(ref)==case['reference_sha256']
        parsed={}
        for profile in ['native','ocr']:
            command=[str(args.binary), parse_command(args.binary),str(source),'--to','json']
            if profile=='ocr': command+=['--ocr-config',str(args.config)]
            start=time.monotonic()
            result=subprocess.run(command,capture_output=True,text=True,timeout=120)
            elapsed=time.monotonic()-start
            assert result.returncode==2,(case['id'],profile,result.stderr)
            output=json.loads(result.stdout); parsed[profile]=output
            if profile == 'ocr':
                for block in output['document']['blocks']:
                    if case['id']=='native-and-scan' and block['source'].get('page')==1:
                        assert 'ocr' not in block['source']
                        continue
                    evidence=block['source'].get('ocr')
                    if evidence:
                        width,height=evidence['image_size']
                        assert width>0 and height>0 and 0<=evidence['confidence']<=1
                        assert len(evidence['quad'])==4
                        assert all(0<=x<=width and 0<=y<=height for x,y in evidence['quad'])
                    else:
                        region=block['source']['pdf_region']
                        assert region['bbox'][2]>region['bbox'][0] and region['bbox'][3]>region['bbox'][1]
            value=text(output)
            record={'id':case['id'],'profile':profile,'wall_seconds':elapsed,'status':output['status'],
                    'output_sha256':hashlib.sha256(value.encode()).hexdigest(),
                    'diagnostics':[{'code':d['code'],'page':d['source'].get('page')} for d in output['diagnostics']],
                    'quality':quality(value,ref.read_text())}
            records.append(record)
            # All inputs and outputs here are authored project fixtures.
            (args.out/f"{case['id']}-{profile}.json").write_text(result.stdout)
            (args.out/f"{case['id']}-{profile}.txt").write_text(value+'\n')
        if case['id']=='native-and-scan':
            native=lambda d:[b for b in d['document']['blocks'] if b['source'].get('page')==1]
            assert native(parsed['native'])==native(parsed['ocr'])
            assert [d['source']['page'] for d in parsed['ocr']['diagnostics'] if d['code']=='pdf_ocr_applied']==[2]
        print(case['id'],records[-1]['quality'],flush=True)
    (args.out/'summary.json').write_text(json.dumps(records,indent=2)+'\n')


if __name__=='__main__': main()
