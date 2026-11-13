#!/usr/bin/env python3
"""One-pass default-output comparison against the previous explicit large profile."""
import argparse,datetime,json,os,re,shutil,subprocess
from pathlib import Path
from benchmark_realworld import ROOT,REPORT,Worker,digest,write
from benchmark_opendataloader import snapshot_sources

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--before',type=Path,required=True)
    ap.add_argument('--after',type=Path,default=ROOT/'target/release/examples/office_bench')
    ap.add_argument('--out',type=Path,required=True)
    ap.add_argument('--reuse-before',type=Path,help='Reuse frozen before runs after an implementation fix; do not repeat their timings')
    args=ap.parse_args();out=args.out.resolve();out.mkdir(parents=True,exist_ok=False)
    corpus=json.loads((REPORT/'corpus-manifest.json').read_text())
    assert len(corpus)==len({d['sha256'] for d in corpus})==112
    assert all(digest(ROOT/d['path'])==d['sha256'] for d in corpus)
    write(out/'manifest.json',corpus)
    before,after=str(args.before.resolve()),str(args.after.resolve())
    commands={
        'before_markdown':[before,'--large-documents','--stream-markdown'],
        'after_markdown':[after,'--stream-markdown'],
        'before_structured':[before,'--large-documents','--verify-json'],
        'after_structured':[after,'--verify-json'],
    }
    hashes={k:digest(Path(v[0])) for k,v in commands.items()}
    sources=snapshot_sources(out)
    for p in [Path(__file__).resolve(),ROOT/'crates/bulkparse/examples/office_bench.rs',ROOT/'benchmarks/realworld/cpu.rs']:
        key=str(p.relative_to(ROOT));sources[key]=digest(p)
        dest=out/'source-snapshot'/key;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dest)
    write(out/'source-manifest.json',sources)
    write(out/'identity.json',dict(method=__doc__,commands=commands,binary_sha256=hashes,
        started_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        hardware=subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string','hw.memsize','hw.logicalcpu'],text=True).splitlines(),
        reused_before=str(args.reuse_before) if args.reuse_before else None,
        power=subprocess.check_output(['pmset','-g','batt'],text=True),warmup=0,passes=1,
        limits='Before explicitly selects 16,777,216 nodes, 4,194,304 cells, 256 MiB expansion. After uses defaults, identical work caps, plus 100,000 additional repeated cells.',
        output_scope='Complete streamed Markdown or full structured model; no recovery. Native JSON serialization/validation/SHA-256 check is timed separately. RSS includes protocol transport and verification.'))
    env=dict(os.environ,RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1')
    profiles={};results={}
    for label,cmd in commands.items():
        if label.startswith('before_') and args.reuse_before:
            reference=args.reuse_before
            identity=json.loads((reference/'identity.json').read_text())
            assert identity['binary_sha256'][label]==hashes[label]
            run=json.loads((reference/(label+'.json')).read_text())
            results[label]={r['id']:r for r in run['documents']}
            assert set(results[label])=={d['id'] for d in corpus}
            profiles[label]=json.loads((reference/'summary.json').read_text())['profiles'][label]
            write(out/(label+'.json'),run)
            continue
        folder=out/'prediction'/label;folder.mkdir(parents=True)
        worker=Worker(cmd,out/(label+'.log'),env);rows=[]
        print('Start',label,flush=True)
        try:
            for index,doc in enumerate(corpus):
                row=worker.call(ROOT/doc['path']);md=row.pop('markdown')
                p=folder/(doc['id']+'.md');p.write_text(md)
                row.update(id=doc['id'],format=doc['format'],output_sha256=digest(p),output_bytes=p.stat().st_size)
                rows.append(row)
                with (out/(label+'.jsonl')).open('a') as f:f.write(json.dumps(row)+'\n')
                if (index+1)%20==0 or not row['ok']:print(label,index+1,doc['id'],row['ok'],row.get('error',''),flush=True)
        finally:code=worker.close()
        rss=re.findall(r'(\d+)\s+maximum resident set size',(out/(label+'.log')).read_text())
        profiles[label]=dict(returned=sum(r['ok'] for r in rows),nonempty=sum(bool(r['output_bytes']) for r in rows),
            complete=sum(r.get('status')=='complete' for r in rows),partial=sum(r.get('status')=='partial' for r in rows),
            truncated=sum(any(d['code']=='resource_limit_partial' for d in r.get('diagnostics',[])) for r in rows),
            seconds=sum(r['elapsed_ns'] for r in rows)/1e9,cpu_seconds=sum(r['cpu_seconds'] for r in rows),
            worker_peak_rss_bytes=int(rss[-1]) if rss else None,exit_code=code,
            json_validated=sum(r.get('json_check',{}).get('valid',False) for r in rows),
            json_validation_seconds=sum(r.get('json_check',{}).get('serialization_and_validation_ns',0) for r in rows)/1e9)
        results[label]={r['id']:r for r in rows};write(out/(label+'.json'),dict(documents=rows));print(label,profiles[label],flush=True)
    changes={};fields=['ok','status','diagnostics','render_diagnostics','error','output_sha256']
    for label in ['after_markdown','before_structured','after_structured']:
        changes[label]={}
        for i,r in results[label].items():
            different=[k for k in fields if r.get(k)!=results['before_markdown'][i].get(k)]
            if label=='after_structured' and r.get('json_check',{}).get('sha256')!=results['before_structured'][i].get('json_check',{}).get('sha256'):
                different.append('full_json_sha256')
            if different:changes[label][i]=different
    assert all(digest(ROOT/p)==h for p,h in sources.items()),'Sources changed during measurement'
    assert all(digest(Path(commands[k][0]))==h for k,h in hashes.items())
    write(out/'summary.json',dict(profiles=profiles,differences=changes))
    assert all(r['returned']==112 and r['truncated']==0 for r in profiles.values()),profiles
    assert all(not r for r in changes.values()),changes
    assert profiles['after_structured']['json_validated']==112
    print('112/112 default Markdown and structured JSON; all content and full JSON hashes unchanged.',flush=True)
if __name__=='__main__':main()
