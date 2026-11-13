#!/usr/bin/env python3
"""Sequential same-machine Office benchmark: one warm-up, five rotating passes.
Measures input reads + conversion + Markdown export; excludes process startup,
output serialization/writes, scoring, and unsupported-format dispatch.
"""
import argparse, hashlib,json,os,platform,re,statistics,subprocess,time
from pathlib import Path
from benchmark_same_machine import Worker
ROOT=Path(__file__).resolve().parents[1]
ENGINES=['bulkparse','anydoc','docling']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,v):p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,default=ROOT/'reports/multiformat-2026-11-11/benchmark');args=ap.parse_args();out=args.out
    out.mkdir(parents=True,exist_ok=False)
    corpus=json.loads((ROOT/'reports/multiformat-2026-11-11/corpus-manifest.json').read_text())+json.loads((ROOT/'fixtures/office-authored/manifest.json').read_text())
    for i,r in enumerate(corpus):
        assert sha(ROOT/r['path'])==r['sha256'],r['path'];r['id']=f'{i:03d}'
    write(out/'manifest.json',corpus)
    paths={e:[r['path'] for r in corpus if r['track']!='robustness' and (e!='docling' or r['format'] in ('xlsx','xlsm','pptx','pptm','ppsx','ppsm'))] for e in ENGINES}
    argv={'bulkparse':[str(ROOT/'target/release/examples/office_bench')], 'anydoc':[str(ROOT/'target/multiformat-anydoc/release/anydoc-bench-worker')], 'docling':[str(ROOT/'target/competitors/docling-venv/bin/python'),str(ROOT/'scripts/worker_docling_office.py')]}
    env=os.environ.copy();env.update({'RAYON_NUM_THREADS':'1','OMP_NUM_THREADS':'1','OPENBLAS_NUM_THREADS':'1','HF_HUB_OFFLINE':'1','TRANSFORMERS_OFFLINE':'1'})
    workers={};runs={e:[] for e in ENGINES};resources={}
    try:
        for e in ENGINES:
            cmd=['/usr/bin/time','-l' if platform.system()=='Darwin' else '-v',*argv[e]]
            workers[e]=Worker(cmd,out/f'{e}.stderr',env)
        assert workers['docling'].receive(timeout=120)['ready']
        for e in ENGINES:
            response=workers[e].call({'documents':paths[e]});write(out/f'{e}-warmup.json',response)
            print(f'{e}: warm-up {len(response["documents"])} documents',flush=True)
        for pass_index in range(5):
            order=ENGINES[pass_index%3:]+ENGINES[:pass_index%3]
            for e in order:
                response=workers[e].call({'documents':paths[e]});assert [r['path'] for r in response['documents']]==paths[e]
                write(out/f'{e}-pass-{pass_index+1}.json',response);runs[e].append(response)
                print(f'{e}: pass {pass_index+1}, {sum(r["elapsed_ns"] for r in response["documents"])/1e6:.3f} ms',flush=True)
        # Robustness is intentionally excluded from speed/quality aggregates.
        for e in ('bulkparse','anydoc'):
            response=workers[e].call({'documents':[r['path'] for r in corpus if r['track']=='robustness']});write(out/f'{e}-robustness.json',response)
    finally:
        for e,w in workers.items():resources[e]={'exit':w.close()}
    write(out/'worker-exits.json',resources)
    summary={'method':__doc__,'platform':platform.platform(),'machine':platform.machine(),'engines':{},'per_format':{},'authored':{},'resources':resources}
    for e in ENGINES:
        log=(out/f'{e}.stderr').read_text();m=re.search(r'(\d+)\s+maximum resident set size',log)
        if m:resources[e]['lifetime_peak_rss_bytes']=int(m[1])
        # macOS time reports lifetime CPU including startup, all six quality runs
        # and the separately labelled robustness pass. It is not warm CPU time.
        m=re.search(r'([\d.]+)\s+real\s+([\d.]+)\s+user\s+([\d.]+)\s+sys',log)
        if m:resources[e].update(lifetime_user_seconds=float(m[2]),lifetime_system_seconds=float(m[3]))
        bypath=[{r['path']:r for r in run['documents']} for run in runs[e]]
        deterministic=all([(r['ok'],r['markdown']) for r in run['documents']]==[(r['ok'],r['markdown']) for r in runs[e][0]['documents']] for run in runs[e])
        summary['engines'][e]={'attempted':len(paths[e]),'returned':sum(r['ok'] for r in runs[e][0]['documents']), 'median_corpus_ms':statistics.median(sum(r['elapsed_ns'] for r in run['documents'])/1e6 for run in runs[e]),'deterministic':deterministic}
        for fmt in sorted({r['format'] for r in corpus if r['track']!='robustness'}):
            chosen=[r for r in corpus if r['format']==fmt and r['track']!='robustness' and r['path'] in paths[e]]
            if not chosen:continue
            times=[statistics.median(run[r['path']]['elapsed_ns']/1e6 for run in bypath) for r in chosen]
            summary['per_format'].setdefault(fmt,{})[e]={'attempted':len(chosen),'returned':sum(bypath[0][r['path']]['ok'] for r in chosen),'partial':sum(bypath[0][r['path']].get('status') in ('partial','partial_success') for r in chosen),'median_document_ms':statistics.median(times),'median_format_corpus_ms':statistics.median(sum(run[r['path']]['elapsed_ns']/1e6 for r in chosen) for run in bypath)}
        checks=[]
        for r in corpus:
            if r['track']!='authored' or r['path'] not in paths[e]:continue
            result=bypath[0][r['path']];md=result['markdown'];md=re.sub(r'\\([\\`*_{}\[\]<>#!|~+\-=.])',r'\1',md)
            missing=[s for s in r['required_text'] if s not in md];forbidden=[s for s in r['forbidden_text'] if s in md]
            checks.append({'path':r['path'],'passed':result['ok'] and not missing and not forbidden,'missing':missing,'forbidden':forbidden})
        summary['authored'][e]=checks
        pred=out/'predictions'/e;pred.mkdir(parents=True)
        for r in corpus:
            if r['path'] in bypath[0]:(pred/(r['id']+'.md')).write_text(bypath[0][r['path']]['markdown'])
    source=[p for p in (ROOT/'crates/bulkparse/src').rglob('*.rs')]+[ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'crates/bulkparse/Cargo.toml',ROOT/'scripts/benchmark_office_formats.py',ROOT/'scripts/worker_docling_office.py',ROOT/'scripts/generate_office_benchmark.py',ROOT/'crates/bulkparse/examples/office_bench.rs',ROOT/'benchmarks/multiformat/anydoc.rs']
    write(out/'source-manifest.json',{str(p.relative_to(ROOT)):sha(p) for p in source})
    write(out/'environment.json',{'platform':platform.platform(),'commands':argv,'binary_hashes':{e:sha(Path(v[0])) for e,v in argv.items()},'rustc':subprocess.check_output(['rustc','-V'],text=True).strip(),'machine':subprocess.run(['sysctl','-n','machdep.cpu.brand_string'],capture_output=True,text=True).stdout.strip() or 'unavailable' if platform.system()=='Darwin' else platform.processor(),'versions':{'bulkparse':'0.1.0','anydoc':'0.2.4','docling':subprocess.check_output([argv['docling'][0],'-c','import importlib.metadata;print(importlib.metadata.version("docling"))'],text=True).strip()}})
    write(out/'summary.json',summary);print(json.dumps(summary['engines'],indent=2),flush=True)
if __name__=='__main__':main()
