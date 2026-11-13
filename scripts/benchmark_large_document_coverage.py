#!/usr/bin/env python3
"""One-pass large-document diagnostic; same explicit limits before and after."""
import argparse, datetime, json, os, platform, re, shutil, subprocess
from pathlib import Path
from benchmark_realworld import ROOT, REPORT, Worker, digest, write
from benchmark_opendataloader import snapshot_sources

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--before', type=Path, required=True)
    ap.add_argument('--after', type=Path, default=ROOT/'target/release/examples/office_bench')
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    out = args.out.resolve(); out.mkdir(parents=True, exist_ok=False)
    corpus = json.loads((REPORT/'corpus-manifest.json').read_text())
    assert len(corpus) == len({d['sha256'] for d in corpus}) == 112
    assert all(digest(ROOT/d['path']) == d['sha256'] for d in corpus)
    write(out/'manifest.json', corpus)
    sources = snapshot_sources(out)
    for p in [Path(__file__), ROOT/'crates/bulkparse/examples/office_bench.rs', ROOT/'benchmarks/realworld/cpu.rs']:
        key = str(p.resolve().relative_to(ROOT)); sources[key] = digest(p)
        dest = out/'source-snapshot'/key; dest.parent.mkdir(parents=True, exist_ok=True); shutil.copy2(p, dest)
    write(out/'source-manifest.json', sources)
    commands = {label:[str(binary.resolve()), '--stream-markdown', '--large-documents'] for label,binary in [('before',args.before), ('after',args.after)]}
    hashes = {k:digest(Path(v[0])) for k,v in commands.items()}
    write(out/'identity.json', dict(method=__doc__, commands=commands, binary_sha256=hashes,
        started_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), platform=platform.platform(),
        hardware=subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string','hw.memsize','hw.logicalcpu'],text=True).splitlines(),
        warmup=0, passes=1, partial_on_limit=False,
        limits=dict(max_input_bytes=33554432,max_text_bytes=33554432,max_nodes=16777216,max_table_cells=4194304,max_expanded_bytes=268435456),
        output_limit='After: 128 MiB including markup. Before binary has no independent Markdown byte cap.',
        scope='Native streamed Markdown with larger explicit work budgets, not default or structured API coverage. Worker RSS includes protocol readback/serialization.'))
    env=dict(os.environ,RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1')
    summary={}; results={}
    for label,cmd in commands.items():
        folder=out/'prediction'/label;folder.mkdir(parents=True)
        worker=Worker(cmd,out/(label+'.log'),env);rows=[]
        try:
            for index,doc in enumerate(corpus):
                row=worker.call(ROOT/doc['path'])
                md=row.pop('markdown');path=folder/(doc['id']+'.md');path.write_text(md)
                row.update(id=doc['id'],format=doc['format'],output_sha256=digest(path),output_bytes=path.stat().st_size)
                rows.append(row)
                with (out/(label+'.jsonl')).open('a') as f:f.write(json.dumps(row)+'\n')
                if not row['ok'] or doc['format'] in ('xls','xlsx','xlsm','ods'):
                    print(label,index+1,doc['id'],row['ok'],row.get('error',''),round(row['elapsed_ns']/1e9,3),flush=True)
        finally:
            exit_code=worker.close()
        write(out/(label+'.json'),dict(documents=rows))
        matches=re.findall(r'(\d+)\s+maximum resident set size',(out/(label+'.log')).read_text())
        summary[label]=dict(returned=sum(r['ok'] for r in rows),nonempty=sum(r['output_bytes']>0 for r in rows),
            complete=sum(r.get('status')=='complete' for r in rows),partial=sum(r.get('status')=='partial' for r in rows),
            truncated=sum(any(d['code']=='resource_limit_partial' for d in r.get('diagnostics',[])) for r in rows),
            seconds=sum(r['elapsed_ns'] for r in rows)/1e9,cpu_seconds=sum(r['cpu_seconds'] for r in rows),
            worker_peak_rss_bytes=int(matches[-1]) if matches else None,exit_code=exit_code)
        results[label]={r['id']:r for r in rows}
        print(label,summary[label],flush=True)
    keys=['ok','status','error','output_sha256','diagnostics','render_diagnostics']
    differences={i:[k for k in keys if results['before'][i].get(k)!=r.get(k)] for i,r in results['after'].items()}
    differences={i:ks for i,ks in differences.items() if ks}
    assert all(digest(ROOT/p)==h for p,h in sources.items()), 'Sources changed during measurement'
    assert all(digest(Path(commands[k][0]))==h for k,h in hashes.items())
    write(out/'summary.json',dict(profiles=summary,differences=differences))
    assert not differences, differences
    print('Output/status/diagnostics identical for all 112 documents.',flush=True)
if __name__=='__main__':main()
