#!/usr/bin/env python3
"""Verify unchanged Markdown on the frozen 200-PDF corpus after format additions."""
import argparse,hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,default=ROOT/'reports/multiformat-2026-11-11/pdf-regression.json');args=ap.parse_args()
    base=ROOT/'reports/same-machine-ocr-table-alignment-2026-11-11';manifest=json.loads((base/'corpus-manifest.json').read_text());worker=ROOT/'target/release/examples/office_bench'
    paths=[ROOT/'target/opendataloader-bench/pdfs'/f'{x}.pdf' for x in manifest['document_ids']]
    for p in paths:assert sha(p)==manifest['sha256']['pdfs/'+p.name]
    response=subprocess.run([str(worker)],input=json.dumps({'documents':[str(p) for p in paths]})+'\n',capture_output=True,text=True,timeout=60,check=True)
    rows=json.loads(response.stdout)['documents'];assert len(rows)==200;results=[]
    for x in rows:
        old=base/'prediction/bulkparse/round-0/markdown'/Path(x['path']).with_suffix('.md').name
        results.append({'path':str(Path(x['path']).relative_to(ROOT)),'returned':x['ok'],'markdown_identical':x['markdown']==old.read_text(),'sha256':hashlib.sha256(x['markdown'].encode()).hexdigest()})
    report={'count':len(results),'passed':all(r['returned'] and r['markdown_identical'] for r in results),'baseline':str(base.relative_to(ROOT)),'binary_sha256':sha(worker),'harness_sha256':sha(Path(__file__)),'documents':results}
    args.out.write_text(json.dumps(report,indent=2)+'\n');assert report['passed'];print('200 PDF Markdown predictions unchanged')
if __name__=='__main__':main()
