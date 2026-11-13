#!/usr/bin/env python3
"""Matched Office performance and output-equivalence check; no parser output repairs."""
import argparse, hashlib, json, os, platform, re, statistics, subprocess
from pathlib import Path
from benchmark_realworld import ROOT, Worker, digest, write
from benchmark_broad import office_corpus


def signature(row):
    return {k: v for k, v in row.items() if k not in {'elapsed_ns', 'cpu_seconds', 'json_check'}} | ({'json_check': {k: v for k, v in row['json_check'].items() if k != 'serialization_and_validation_ns'}} if 'json_check' in row else {})


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--before',type=Path,required=True)
    ap.add_argument('--after',type=Path)
    ap.add_argument('--out',type=Path,required=True)
    ap.add_argument('--corpus',choices=['fixtures','realworld','pdf'],default='fixtures')
    ap.add_argument('--manifest',type=Path,help='Use a frozen custom corpus manifest instead of --corpus')
    ap.add_argument('--formats',nargs='+',help='Restrict the corpus to these extensions, e.g. epub')
    ap.add_argument('--passes',type=int,default=5)
    ap.add_argument('--warmup',type=int,default=1)
    ap.add_argument('--anydoc',action='store_true')
    ap.add_argument('--stream',action='store_true')
    ap.add_argument('--verify-json',action='store_true')
    ap.add_argument('--reverse',action='store_true',help='Reverse the initial profile order for an independent confirmation')
    a=ap.parse_args(); out=a.out.resolve(); out.mkdir(parents=True,exist_ok=False)
    assert a.passes>0 and a.warmup>=0
    if a.manifest:corpus=json.loads(a.manifest.read_text())
    elif a.corpus=='fixtures': corpus=office_corpus()
    elif a.corpus=='realworld':corpus=json.loads((ROOT/'reports/realworld-2026-11-12/corpus-manifest.json').read_text())
    else:
        frozen=json.loads((ROOT/'reports/same-machine-ocr-table-alignment-2026-11-11/corpus-manifest.json').read_text())
        corpus=[dict(id=i,format='pdf',path=f'target/opendataloader-bench/pdfs/{i}.pdf',sha256=frozen['sha256'][f'pdfs/{i}.pdf']) for i in frozen['document_ids']]
    if a.formats:
        corpus=[r for r in corpus if r['format'] in a.formats]
        if not corpus: ap.error('No documents match the requested formats')
    assert all(digest(ROOT/r['path'])==r['sha256'] for r in corpus)
    write(out/'manifest.json',corpus)
    flags=(['--stream-markdown'] if a.stream else [])+(['--verify-json'] if a.verify_json else [])
    commands={'before':[str(a.before.resolve()),*flags]}
    if a.after:commands['after']=[str(a.after.resolve()),*flags]
    if a.anydoc:commands['anydoc']=[str(ROOT/'target/multiformat-anydoc/release/anydoc-bench-worker')]
    if a.reverse: commands=dict(reversed(list(commands.items())))
    env=dict(os.environ,RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1')
    hashes={k:digest(Path(cmd[0])) for k,cmd in commands.items()}
    write(out/'identity.json',dict(commands=commands,binary_sha256=hashes,platform=platform.platform(),hardware=subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string','hw.memsize','hw.logicalcpu'],text=True).splitlines(),warmup=a.warmup,passes=a.passes,started_utc=__import__('datetime').datetime.now(__import__('datetime').timezone.utc).isoformat(),power=subprocess.check_output(['pmset','-g','batt'],text=True),harness_sha256=digest(Path(__file__)),scope='Sequential reads, conversion and Markdown rendering; worker startup and protocol excluded from time. RSS includes full worker lifetime and transport. Optional JSON verification excluded from conversion time but included in RSS. Rotating profile order; unchanged inputs and binaries verified.'))
    workers={k:Worker(cmd,out/(k+'.log'),env) for k,cmd in commands.items()}; runs={k:[] for k in commands}
    try:
        for i in range(a.warmup+a.passes):
            order=list(commands); order=order[i%len(order):]+order[:i%len(order)]
            for k in order:
                rows=[]
                for r in corpus:
                    row=workers[k].call(ROOT/r['path']);md=row.pop('markdown');row.update(id=r['id'],format=r['format'],markdown_sha256=hashlib.sha256(md.encode()).hexdigest())
                    rows.append(row)
                write(out/f'{k}-{i}.json',rows);runs[k].append(rows)
                print(k,i,round(sum(r['elapsed_ns'] for r in rows)/1e9,6),sum(r['ok'] for r in rows),flush=True)
    finally:
        codes={k:w.close() for k,w in workers.items()}
        write(out/'exit-codes.json',codes)
    assert all(c==0 for c in codes.values()),codes
    summary={}
    for k,passes in runs.items():
        measured=passes[a.warmup:]
        peaks=re.findall(r'(\d+)\s+maximum resident set size',(out/(k+'.log')).read_text())
        assert peaks
        summary[k]=dict(returned=sum(r['ok'] for r in measured[0]),attempted=len(corpus),seconds=statistics.median(sum(r['elapsed_ns'] for r in rows)/1e9 for rows in measured),cpu_seconds=statistics.median(sum(r.get('cpu_seconds',0) for r in rows) for rows in measured),peak_rss_bytes=max(map(int,peaks)),per_format={})
        for f in sorted({r['format'] for r in corpus}):
            selected=[[r for r in rows if r['format']==f] for rows in measured]
            times=[sum(r['elapsed_ns'] for r in rows)/1e6 for rows in selected]
            summary[k]['per_format'][f]=dict(attempted=len(selected[0]),returned=sum(r['ok'] for r in selected[0]),median_ms=statistics.median(times),samples_ms=times)
    comparisons={}
    if 'after' in runs:
        before=runs['before'][a.warmup]
        comparisons['changed']=[{'id':r['id'],'fields':[key for key in set(signature(r))|set(signature(s)) if signature(r).get(key)!=signature(s).get(key)]} for r,s in zip(before,runs['after'][a.warmup]) if signature(r)!=signature(s)]
    comparisons['within_profile_changes']={k:[i for i,rows in enumerate(passes[a.warmup:]) if [signature(r) for r in rows]!=[signature(r) for r in passes[a.warmup]]] for k,passes in runs.items()}
    assert all(digest(Path(commands[k][0]))==h for k,h in hashes.items())
    assert all(digest(ROOT/r['path'])==r['sha256'] for r in corpus)
    write(out/'summary.json',dict(profiles=summary,comparisons=comparisons))
    for f in sorted({r['format'] for r in corpus}):print(f,{k:round(v['per_format'][f]['median_ms'],3) for k,v in summary.items()})
    print('comparisons',comparisons)

if __name__=='__main__':main()
