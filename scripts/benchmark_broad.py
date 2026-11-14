#!/usr/bin/env python3
"""Expanded same-machine comparison, warm-up plus five rotating sequential passes."""
import argparse,hashlib,json,os,platform,re,shutil,statistics,subprocess,sys,time
from pathlib import Path
from benchmark_same_machine import Worker,validate_run
from benchmark_opendataloader import ROOT,REVISION,EVALUATOR_FILES,validate_corpus,evaluate,snapshot_sources,sha256,write_json

PY=ROOT/'target/competitors/broad-venv/bin/python'
DPY=ROOT/'target/competitors/docling-venv/bin/python'
PDF=['bulkparse','anydoc','pdf-inspector','liteparse','opendataloader','pymupdf4llm','pymupdf4llm-layout','markitdown','docling']
OFFICE=['bulkparse','anydoc','docling','markitdown','pandoc','mammoth']
SUPPORT={
 'markitdown':{'docx','xlsx','xls','pptx','epub'},
 'pandoc':{'docx','odt','epub','rtf'},
 'mammoth':{'docx'},
 'docling':{'docx','doc','docm','rtf','pptx','pptm','ppsx','ppsm','ppt','xlsx','xlsm','xls','odt','ods','odp','epub'},
}

def commands(mode):
    common={e:[str(PY),str(ROOT/'scripts/worker_broad.py'),e] for e in PDF+OFFICE}
    common['pdf-inspector']=[str(ROOT/'target/broad/pdf-inspector/release/pdf-inspector-bench-worker')]
    if mode=='pdf':
        common.update(bulkparse=[str(ROOT/'target/broad/bulkparse/release/bulkparse-opendataloader-bench')],
            anydoc=[str(ROOT/'target/same-machine-benchmark/anydoc-build/release/anydoc-bench-worker')],
            docling=[str(DPY),str(ROOT/'scripts/worker_docling_corpus.py'),'--artifacts',str(ROOT/'target/competitors/models')])
    else:
        common.update(bulkparse=[str(ROOT/'target/release/examples/office_bench')],
            anydoc=[str(ROOT/'target/multiformat-anydoc/release/anydoc-bench-worker')],
            docling=[str(DPY),str(ROOT/'scripts/worker_docling_broad_office.py')])
    return common

def office_corpus():
    rows=json.loads((ROOT/'reports/multiformat-legacy-layout-2026-11-12/benchmark-final/manifest.json').read_text())
    rows=[r for r in rows if r['track']!='robustness']
    known={r['path'] for r in rows}
    for p in sorted((ROOT/'legacy/anydoc-main/tests/fixtures/docx').glob('*.docx')):
        name=str(p.relative_to(ROOT))
        if name not in known: rows.append(dict(path=name,sha256=sha256(p),format='docx',track='added-docx'))
    for i,r in enumerate(rows): r['id']=f'{i:03d}';assert sha256(ROOT/r['path'])==r['sha256']
    return rows

def freeze(out,cmds):
    sources=snapshot_sources(out)
    paths=list((ROOT/'benchmarks/broad').rglob('*'))
    paths+=list((ROOT/'legacy/anydoc-main/src').rglob('*.rs'))+list((ROOT/'legacy/pdf-inspector-main/src').rglob('*.rs'))
    paths+=list((ROOT/'legacy/pdf-inspector-main/external').rglob('*'))
    for base in ['legacy/anydoc-main','legacy/pdf-inspector-main','benchmarks/multiformat/anydoc','benchmarks/same-machine/anydoc']:
        paths+=list((ROOT/base).glob('Cargo.*'))+list((ROOT/base).glob('LICENSE*'))
    paths += [ROOT/'scripts'/n for n in ['benchmark_broad.py','worker_broad.py','worker_docling_broad_office.py','worker_docling_corpus.py','benchmark_same_machine.py']]
    paths += [ROOT/'benchmarks/same-machine/anydoc.rs',ROOT/'benchmarks/multiformat/anydoc.rs',ROOT/'crates/bulkparse/examples/office_bench.rs']
    for p in paths:
        if not p.is_file():continue
        name=str(p.relative_to(ROOT));sources[name]=sha256(p);dest=out/'source-snapshot'/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dest)
    write_json(out/'source-manifest.json',sources)
    for name,python in [('comparators',PY),('docling',DPY)]:
        freeze=subprocess.check_output([str(python),'-m','pip','freeze'],text=True)
        (out/(name+'-requirements.txt')).write_text(freeze)
    return sources

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--mode',choices=['pdf','office'],required=True);ap.add_argument('--out',type=Path,required=True);ap.add_argument('--smoke',action='store_true');a=ap.parse_args()
    out=a.out.resolve();out.mkdir(parents=True,exist_ok=False);engines=PDF if a.mode=='pdf' else OFFICE
    cmds=commands(a.mode);upstream=ROOT/'target/opendataloader-bench'
    if a.mode=='pdf':
        assert subprocess.check_output(['git','-C',str(upstream),'rev-parse','HEAD'],text=True).strip()==REVISION
        corpus=[dict(id=p.stem,path=str(p.relative_to(ROOT)),sha256=sha256(p),format='pdf') for p in validate_corpus(upstream)]
        if a.smoke:corpus=corpus[:2]
        sources_to_hash=list((upstream/'ground-truth/markdown').glob('*.md'))+[upstream/'src'/n for n in EVALUATOR_FILES]
        write_json(out/'evaluator-reference-hashes.json',{str(p.relative_to(ROOT)):sha256(p) for p in sources_to_hash})
    else:
        corpus=office_corpus()
        if a.smoke:corpus=[next(r for r in corpus if r['format']==f) for f in ['docx','xlsx','epub','doc']]
    write_json(out/'manifest.json',corpus)
    paths={e:[str(ROOT/r['path']) for r in corpus if a.mode=='pdf' or e not in SUPPORT or r['format'] in SUPPORT[e]] for e in engines}
    env=dict(os.environ,RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1',HF_HUB_OFFLINE='1',TRANSFORMERS_OFFLINE='1',TOKENIZERS_PARALLELISM='false')
    sources=freeze(out,cmds)
    machine=subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string','hw.memsize','hw.logicalcpu'],text=True).splitlines()
    identity=dict(platform=platform.platform(),hardware=machine,commands={e:cmds[e] for e in engines},env={k:env[k] for k in ['RAYON_NUM_THREADS','OMP_NUM_THREADS','OPENBLAS_NUM_THREADS','MKL_NUM_THREADS','HF_HUB_OFFLINE','TRANSFORMERS_OFFLINE']},binary_sha256={e:sha256(Path(cmds[e][0])) for e in engines},warmup=1,passes=5 if not a.smoke else 1,mode=a.mode,smoke=a.smoke,started_utc=__import__('datetime').datetime.now(__import__('datetime').timezone.utc).isoformat())
    write_json(out/'identity.json',identity)
    workers={};runs={e:[] for e in engines};resources={};ready={}
    try:
        for e in engines:
            workers[e]=Worker(['/usr/bin/time','-l',*cmds[e]],out/(e+'.stderr.log'),env)
            if e not in ('bulkparse','anydoc','pdf-inspector'):
                ready[e]=workers[e].receive(timeout=240);assert ready[e]['ready'];write_json(out/'effective-options.json',ready)
        for index in range(2 if a.smoke else 6):
            order=engines[index%len(engines):]+engines[:index%len(engines)]
            for e in order:
                print(f'Start {a.mode} round {index} {e} ({len(paths[e])} inputs)',flush=True)
                folder=out/'prediction'/e/f'round-{index}'/'markdown'
                q={'documents':paths[e]}
                if a.mode=='pdf':q['output_dir']=str(folder)
                reply=workers[e].call(q)
                assert len(reply['documents'])==len(paths[e])
                if a.mode=='pdf':
                    hashes=validate_run(reply,[Path(p) for p in paths[e]],folder);elapsed=reply['elapsed_ns']
                else:
                    assert [r['path'] for r in reply['documents']]==paths[e]
                    folder.mkdir(parents=True);hashes={}
                    for row in reply['documents']:
                        if not row['ok']:assert row['markdown']==''
                        entry=next(r for r in corpus if str(ROOT/r['path'])==row['path']);p=folder/(entry['id']+'.md');p.write_text(row['markdown']);hashes[p.name]=sha256(p)
                    elapsed=sum(r['elapsed_ns'] for r in reply['documents'])
                reply.update(index=index,engine_order=order,prediction_sha256=hashes,corpus_elapsed_ns=elapsed)
                runs[e].append(reply);write_json(out/f'{e}-round-{index}.json',reply)
                print(f'Done {e}: {elapsed/1e9:.6f}s, {sum(r["ok"] for r in reply["documents"])}/{len(paths[e])} returned',flush=True)
    finally:
        for e,w in workers.items():
            resources[e]={'exit':w.close()}
            log=(out/(e+'.stderr.log')).read_text(errors='replace');m=re.search(r'(\d+)\s+maximum resident set size',log)
            if m:resources[e]['lifetime_peak_rss_bytes']=int(m[1])
        write_json(out/'resources.json',resources)
    assert all(x['exit']==0 for x in resources.values())
    assert all(sha256(ROOT/n)==v for n,v in sources.items()),'Source changed during run'
    assert all(sha256(ROOT/r['path'])==r['sha256'] for r in corpus)
    summary={}
    for e,rs in runs.items():
        deterministic=all(r['prediction_sha256']==rs[1]['prediction_sha256'] for r in rs[1:])
        summary[e]=dict(attempted=len(paths[e]),returned=sum(r['ok'] for r in rs[-1]['documents']),partial=sum(r.get('status') in ['partial','partial_success'] for r in rs[-1]['documents']),deterministic=deterministic,median_seconds=statistics.median(r['corpus_elapsed_ns']/1e9 for r in rs[1:]),measured_seconds=[r['corpus_elapsed_ns']/1e9 for r in rs[1:]],resources=resources[e],failed_documents=[r.get('path',r.get('document_id')) for r in rs[-1]['documents'] if not r['ok']])
        if a.mode=='pdf' and not a.smoke:
            sys.path.insert(0,str(upstream/'src'));indexes=[5] if deterministic else list(range(1,6));scores=[]
            for i in indexes:
                folder=out/'prediction'/e/f'round-{i}'
                scores.append(evaluate(upstream,folder/'markdown',folder,e,{'engine_name':e,'document_count':200,'total_elapsed':summary[e]['median_seconds']}))
            summary[e]['metrics']={k:statistics.mean(s['metrics']['score'][k] for s in scores) for k in scores[0]['metrics']['score']};summary[e]['quality_rounds']=indexes
        if a.mode=='office':
            summary[e]['per_format']={}
            for fmt in sorted({r['format'] for r in corpus}):
                selected={str(ROOT/r['path']) for r in corpus if r['format']==fmt}&set(paths[e])
                if not selected:continue
                summary[e]['per_format'][fmt]=dict(attempted=len(selected),returned=sum(r['ok'] for r in rs[-1]['documents'] if r['path'] in selected),median_seconds=statistics.median(sum(r['elapsed_ns'] for r in run['documents'] if r['path'] in selected)/1e9 for run in rs[1:]))
        write_json(out/'summary.json',summary)
    print(json.dumps(summary,indent=2),flush=True)

if __name__=='__main__':main()
