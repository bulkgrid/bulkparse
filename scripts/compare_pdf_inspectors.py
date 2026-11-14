#!/usr/bin/env python3
"""Paired local PDF inspection/conversion benchmark; never runs OCR.

Fixed existing corpus, persistent single workers, alternating engine order,
per-case warmup, library wall time excluding input reads and JSON serialization.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import selectors
import statistics
import subprocess
import time
from evaluate_holdout import quality

ROOT=Path(__file__).resolve().parents[1]

def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def source_hash(directory):
    files=sorted(Path(directory).rglob('*.rs'))
    return hashlib.sha256(''.join(str(p.relative_to(directory))+':'+sha(p)+'\n' for p in files).encode()).hexdigest()

def corpus():
    cases=[json.loads(l) for l in (ROOT/'benchmarks/holdout/manifest.jsonl').read_text().splitlines()]
    for c in cases:
        if c['id'].startswith('synthetic/'):
            c['expected_ocr_pages']={'scan':[1],'mixed-scan':[2]}.get(c['category'],[])
    existing={c['source_sha256'] for c in cases}
    for c in json.loads((ROOT/'benchmarks/ocr/manifest.json').read_text()):
        if c['sha256'] in existing: continue
        cases.append({'id':'ocr/'+c['id'],'source_path':c['path'],'source_sha256':c['sha256'],
            'reference_path':c['reference'],'reference_sha256':c['reference_sha256'],
            'reference_kind':'authored_gold','category':'scan','expected_ocr_pages':[1,2] if c['id']=='ocr-two-pages' else [1]})
    for name,routes in [('inspection/header-over-scan.pdf',[1]),('native-and-empty.pdf',[])]:
        p=ROOT/'fixtures'/name
        cases.append({'id':name,'source_path':str(p.relative_to(ROOT)),'source_sha256':sha(p),
                      'reference_kind':'routing_only','category':'routing','expected_ocr_pages':routes})
    for c in cases:
        assert sha(ROOT/c['source_path'])==c['source_sha256'],c['id']
        if c.get('reference_path'): assert sha(ROOT/c['reference_path'])==c['reference_sha256'],c['id']
    return cases

class Worker:
    def __init__(self,binary,mode,stderr):
        self.mode=mode
        self.err=open(stderr,'w+')
        self.proc=subprocess.Popen([str(binary)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.err,
            cwd=ROOT,env={**os.environ,'RAYON_NUM_THREADS':'1'},bufsize=0,start_new_session=True)
        self.selector=selectors.DefaultSelector(); self.selector.register(self.proc.stdout,selectors.EVENT_READ)
        self.pending=b''
    def call(self,case):
        self.proc.stdin.write((json.dumps({'path':str(ROOT/case['source_path']),'mode':self.mode})+'\n').encode())
        import time
        deadline=time.monotonic()+45
        while b'\n' not in self.pending:
            left=deadline-time.monotonic()
            if left<=0 or not self.selector.select(left):
                raise TimeoutError('benchmark worker timeout')
            chunk=os.read(self.proc.stdout.fileno(),65536)
            if not chunk: raise RuntimeError('benchmark worker exited')
            self.pending+=chunk
            if len(self.pending)>64*1024*1024: raise RuntimeError('benchmark response exceeded 64 MiB')
        line,self.pending=self.pending.split(b'\n',1)
        return json.loads(line)
    def close(self):
        # Reap exactly our worker and obtain its usage; no global process listing
        # or privileged macOS sysctl calls are needed.
        self.proc.stdin.close()
        deadline=time.monotonic()+10
        while True:
            pid,status,usage=os.wait4(self.proc.pid,os.WNOHANG)
            if pid: break
            if time.monotonic()>deadline:
                import signal
                os.killpg(self.proc.pid,signal.SIGKILL)
                pid,status,usage=os.wait4(self.proc.pid,0)
                break
            time.sleep(0.01)
        self.proc.returncode=os.waitstatus_to_exitcode(status)
        self.selector.close();self.proc.stdout.close();self.err.close()
        return {'exit_code':self.proc.returncode,'max_rss_bytes':usage.ru_maxrss*(1 if platform.system()=='Darwin' else 1024),
                'user_cpu_seconds':usage.ru_utime,'system_cpu_seconds':usage.ru_stime}

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out',type=Path,required=True)
    ap.add_argument('--repeats',type=int,default=7)
    args=ap.parse_args();assert args.repeats>=3
    args.out.mkdir(parents=True,exist_ok=False)
    raw=ROOT/'target/inspector-comparison'/args.out.name;raw.mkdir(parents=True,exist_ok=False)
    cases=corpus();(args.out/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n')
    engines={n:ROOT/f'target/inspector-comparison/{n}/release/{"bulkparse" if n=="bulkparse" else "firecrawl"}-inspector-bench' for n in ['bulkparse','firecrawl']}
    metadata={'created_utc':datetime.now(timezone.utc).isoformat(),'platform':platform.platform(),
        'machine':platform.machine(),'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),
        'logical_cpus':os.cpu_count(),'repeats':args.repeats,'warmups_per_case_per_mode':1,
        'rayon_num_threads':1,'firecrawl_version':'1.25.2 supplied main snapshot; source digest, not verified Git revision',
        'bulkparse_source_sha256':source_hash(ROOT/'crates/bulkparse/src'),
        'firecrawl_source_sha256':source_hash(ROOT/'legacy/pdf-inspector-main/src'),
        'harness_sha256':sha(__file__),'worker_source_sha256':sha(ROOT/'benchmarks/inspector-comparison/worker.rs'),
        'executables':{n:{'sha256':sha(p),'bytes':p.stat().st_size} for n,p in engines.items()},
        'locks':{n:sha(ROOT/f'benchmarks/inspector-comparison/{n}/Cargo.lock') for n in engines},
        'scope':'No OCR. All-page inspection. BulkParse decodes text; Firecrawl DetectOnly does not. Conversion includes native extraction and Markdown rendering. Input read, JSON serialization and startup excluded from internal wall times. Single persistent worker per engine, single Rayon thread. Peak RSS from wait4 on each owned child covers each worker lifetime including warmups, serialization and input reads; not memory per PDF. Failures remain in attempt counts. Quality proxies are not semantic/layout accuracy.'}
    (args.out/'run.json').write_text(json.dumps(metadata,indent=2)+'\n')
    records=[];resources={};artifacts={}
    try:
        for mode in ['inspect','convert']:
            workers={n:Worker(p,mode,raw/f'{n}-{mode}.stderr') for n,p in engines.items()}
            try:
                for c in cases:
                    for w in workers.values(): w.call(c)
                for rep in range(args.repeats):
                    for idx,c in enumerate(cases):
                        names=list(workers)
                        if (idx+rep)%2: names.reverse()
                        for name in names:
                            reply=workers[name].call(c)
                            result=reply.get('result',{})
                            encoded=json.dumps(result,sort_keys=True,ensure_ascii=False).encode()
                            record={'engine':name,'mode':mode,'id':c['id'],'repetition':rep,
                                'ok':reply['ok'],'ns':reply['ns'],'output_sha256':hashlib.sha256(encoded).hexdigest(),
                                'page_count':result.get('page_count'),'ocr_pages':result.get('ocr_pages'), 'error':reply.get('error')}
                            if 'expected_ocr_pages' in c:
                                record['routing_exact']=reply['ok'] and result.get('ocr_pages')==c['expected_ocr_pages']
                            if rep==0:
                                path=raw/f'{name}-{mode}-{idx}.json';path.write_bytes(encoded);artifacts[(name,mode,c['id'])]=path
                            records.append(record)
                    print(mode,rep+1,flush=True)
            finally:
                for n,w in workers.items(): resources[n+'-'+mode]=w.close()
    finally:
        (args.out/'samples.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in records))
        (args.out/'resources.json').write_text(json.dumps(resources,indent=2)+'\n')
    per_case=[]
    for mode in ['inspect','convert']:
        for name in engines:
            for c in cases:
                rows=[r for r in records if r['engine']==name and r['mode']==mode and r['id']==c['id']]
                row={'engine':name,'mode':mode,'id':c['id'],'reference_kind':c['reference_kind'],
                    'successes':sum(r['ok'] for r in rows),'attempts':len(rows),
                    'median_ms':statistics.median(r['ns']/1e6 for r in rows),
                    'deterministic':len({r['output_sha256'] for r in rows})==1,'ocr_pages':rows[0]['ocr_pages']}
                if 'expected_ocr_pages' in c:
                    row.update(expected_ocr_pages=c['expected_ocr_pages'],routing_exact=all(r['routing_exact'] for r in rows))
                if mode=='convert' and c.get('reference_path'):
                    data=json.loads(artifacts[(name,mode,c['id'])].read_text())
                    actual=data.get('markdown') or ''
                    row['quality']=quality(actual,(ROOT/c['reference_path']).read_text(),c)
                per_case.append(row)
    summary=[]
    for mode in ['inspect','convert']:
        for name in engines:
            group=[r for r in per_case if r['engine']==name and r['mode']==mode]
            times=[sum(r['ns'] for r in records if r['engine']==name and r['mode']==mode and r['repetition']==rep)/1e6 for rep in range(args.repeats)]
            subset=[r for r in group if 'routing_exact' in r]
            summary.append({'engine':name,'mode':mode,'documents':len(group),'attempts':sum(r['attempts'] for r in group),'successes':sum(r['successes'] for r in group),
                'median_corpus_ms':statistics.median(times),'corpus_ms_range':[min(times),max(times)],
                'median_document_ms':statistics.median(r['median_ms'] for r in group),
                'max_rss_mib':resources[name+'-'+mode]['max_rss_bytes']/1024**2 if resources[name+'-'+mode]['max_rss_bytes'] else None,
                'routing_cases':len(subset),'routing_exact_cases':sum(r['routing_exact'] for r in subset)})
    (args.out/'per-case.json').write_text(json.dumps(per_case,indent=2)+'\n')
    (args.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    assert len(records)==len(cases)*2*2*args.repeats
    assert all(r['deterministic'] for r in per_case)
    assert all(r['exit_code']==0 for r in resources.values()),resources
    print(json.dumps(summary,indent=2))

if __name__=='__main__': main()
