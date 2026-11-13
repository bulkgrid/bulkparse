#!/usr/bin/env python3
"""One full-corpus structured-model pass with complete JSON serialization validation."""
import argparse,datetime,hashlib,json,os,re
from pathlib import Path
from benchmark_realworld import ROOT,REPORT,Worker,digest,write
from benchmark_opendataloader import snapshot_sources

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--reference',type=Path,required=True);ap.add_argument('--out',type=Path,required=True)
    args=ap.parse_args();out=args.out.resolve();out.mkdir(parents=True,exist_ok=False)
    corpus=json.loads((REPORT/'corpus-manifest.json').read_text());assert len(corpus)==112
    reference={r['id']:r for r in json.loads((args.reference/'after.json').read_text())['documents']}
    binary=ROOT/'target/release/examples/office_bench';cmd=[str(binary),'--large-documents','--verify-json']
    sources=snapshot_sources(out);write(out/'source-manifest.json',sources)
    binary_hash=digest(binary)
    write(out/'identity.json',dict(method=__doc__,command=cmd,binary_sha256=binary_hash,
        reference=str(args.reference),reference_sha256=digest(args.reference/'after.json'),
        started_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),warmup=0,passes=1,
        interpretation='Full structured model retained in memory. Complete compact JSON serialized to a buffered temporary file and validated to EOF with a streaming JSON decoder. Parsing/Markdown timing excludes JSON verification; process RSS includes all steps and benchmark serialization. No bounded recovery.'))
    worker=Worker(cmd,out/'worker.log',dict(os.environ,RAYON_NUM_THREADS='1'))
    rows=[];differences=[]
    try:
        for i,doc in enumerate(corpus):
            assert digest(ROOT/doc['path'])==doc['sha256']
            row=worker.call(ROOT/doc['path']);md=row.pop('markdown');row['output_sha256']=hashlib.sha256(md.encode()).hexdigest()
            row.update(id=doc['id'],format=doc['format'])
            changed=[k for k in ['ok','status','diagnostics','render_diagnostics','output_sha256'] if row.get(k)!=reference[doc['id']].get(k)]
            if changed:differences.append(dict(id=doc['id'],fields=changed))
            rows.append(row)
            with (out/'documents.jsonl').open('a') as f:f.write(json.dumps(row)+'\n')
            if (i+1)%10==0 or not row['ok']:print(i+1,doc['id'],row['ok'],row.get('json_check'),flush=True)
    finally:code=worker.close()
    rss=re.findall(r'(\d+)\s+maximum resident set size',(out/'worker.log').read_text())
    summary=dict(documents=len(rows),returned=sum(r['ok'] for r in rows),json_validated=sum(r.get('json_check',{}).get('valid',False) for r in rows),
        compact_json_bytes=sum(r.get('json_check',{}).get('bytes',0) for r in rows),
        parser_and_markdown_seconds=sum(r['elapsed_ns'] for r in rows)/1e9,
        parser_and_markdown_cpu_seconds=sum(r['cpu_seconds'] for r in rows),
        json_serialization_and_validation_seconds=sum(r.get('json_check',{}).get('serialization_and_validation_ns',0) for r in rows)/1e9,
        worker_peak_rss_bytes=int(rss[-1]) if rss else None,exit_code=code,differences=differences)
    write(out/'summary.json',summary);print(summary,flush=True)
    assert digest(binary)==binary_hash and all(digest(ROOT/p)==h for p,h in sources.items())
    assert summary['json_validated']==112 and not differences
if __name__=='__main__':main()
