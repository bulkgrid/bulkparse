#!/usr/bin/env python3
"""One-pass image-decoder and streamed-output regression check against preserved binaries."""
import argparse, hashlib, json, os, re
from pathlib import Path
from benchmark_realworld import ROOT, Worker, digest, write

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--before',type=Path,required=True)
    ap.add_argument('--after',type=Path,required=True)
    ap.add_argument('--out',type=Path,required=True)
    ap.add_argument('--pdf-only',action='store_true',help='Skip mixed-format streaming when covered by another comparison')
    args=ap.parse_args(); args.out.mkdir(parents=True,exist_ok=False)
    frozen=json.loads((ROOT/'reports/same-machine-ocr-table-alignment-2026-11-11/corpus-manifest.json').read_text())
    pdf=[dict(id=id,format='pdf',path=f'target/opendataloader-bench/pdfs/{id}.pdf',sha256=frozen['sha256'][f'pdfs/{id}.pdf']) for id in frozen['document_ids']]
    office=json.loads((ROOT/'reports/realworld-2026-11-12/corpus-manifest.json').read_text())
    for d in pdf+office: assert digest(ROOT/d['path'])==d['sha256']
    commands={
        'pdf_before':([str(args.before.resolve())],pdf),
        'pdf_after':([str(args.after.resolve())],pdf),
        'pdf_after_no_images':([str(args.after.resolve()),'--no-images'],pdf),
        'stream_before':([str(args.before.resolve()),'--stream-markdown'],office),
        'stream_after':([str(args.after.resolve()),'--stream-markdown'],office),
    }
    if args.pdf_only:commands={k:v for k,v in commands.items() if k.startswith('pdf_')}
    hashes={key:digest(Path(cmd[0])) for key,(cmd,_) in commands.items()}
    env=dict(os.environ,RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1')
    write(args.out/'identity.json',dict(method=__doc__,warmup=0,passes=1,ocr=False,binary_sha256=hashes,commands={k:v[0] for k,v in commands.items()},harness_sha256=digest(Path(__file__)),pdf_manifest=pdf,office_manifest=office,scope='Parser read + conversion + Markdown rendering; startup/JSON transport excluded from time, included in process-lifetime RSS. Streaming includes output spooling, excludes readback from conversion time. Not a competitor comparison or stabilized median.'))
    all_rows={};summary={}
    for name,(cmd,corpus) in commands.items():
        log=args.out/f'{name}.log'; worker=Worker(cmd,log,env);rows=[]
        try:
            for d in corpus:
                r=worker.call(ROOT/d['path']);md=r.pop('markdown');r.update(id=d['id'],format=d['format'],markdown_sha256=hashlib.sha256(md.encode()).hexdigest())
                if name=='pdf_after':
                    folder=args.out/'pdf-markdown';folder.mkdir(exist_ok=True)
                    (folder/f"{d['id']}.md").write_text(md)
                rows.append(r)
        finally:worker.close()
        peak=re.findall(r'(\d+)\s+maximum resident set size',log.read_text())
        assert peak, 'Missing OS RSS measurement'
        write(args.out/f'{name}.json',rows);all_rows[name]=rows
        summary[name]=dict(returned=sum(r['ok'] for r in rows),documents=len(rows),seconds=sum(r['elapsed_ns'] for r in rows)/1e9,cpu_seconds=sum(r.get('cpu_seconds',0) for r in rows),peak_rss_bytes=max(map(int,peak)),assets=sum(r.get('image_count',0) for r in rows),image_bytes=sum(r.get('image_bytes',0) for r in rows))
        print(name,summary[name],flush=True)
    comparisons={}
    for left,right in [('pdf_before','pdf_after'),('pdf_after','pdf_after_no_images'),('stream_before','stream_after')]:
        if left not in all_rows or right not in all_rows:continue
        pairs=list(zip(all_rows[left],all_rows[right]));assert all(a['id']==b['id'] for a,b in pairs)
        comparisons[f'{left} -> {right}']=dict(identical_markdown=sum(a['markdown_sha256']==b['markdown_sha256'] for a,b in pairs),changed=[a['id'] for a,b in pairs if a['markdown_sha256']!=b['markdown_sha256']],new_failures=[b['id'] for a,b in pairs if a['ok'] and not b['ok']])
    assert all(digest(Path(cmd[0]))==hashes[k] for k,(cmd,_) in commands.items())
    write(args.out/'summary.json',dict(profiles=summary,comparisons=comparisons,binaries_unchanged=True))
if __name__=='__main__':main()
