#!/usr/bin/env python3
"""Linux before/after benchmark of the real Rust parser and supervised PDF OCR."""
import argparse, hashlib, json, os, platform, queue, resource, signal, statistics, subprocess, threading, time
from datetime import datetime, timezone
from pathlib import Path
import psutil
from benchmark_ocr_alternatives import aggregate, score
from evaluate_ocr import text

ROOT = Path(__file__).resolve().parents[1]

def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()

class Worker:
    def __init__(self, binary, config, folder):
        self.stderr = (folder/'stderr.log').open('w')
        self.proc = subprocess.Popen([binary,str(config)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                                     stderr=self.stderr,text=True,bufsize=1,start_new_session=True)
        self.ps = psutil.Process(self.proc.pid);self.messages=queue.Queue();self.done=threading.Event()
        self.peak=0;self.seen=set();self.samples=0
        def read():
            for line in self.proc.stdout:self.messages.put(json.loads(line))
            self.messages.put(None)
        self.reader=threading.Thread(target=read,daemon=True);self.reader.start()
        def sample():
            while not self.done.is_set():
                try:
                    members=self.members();rss=sum(p.memory_info().rss for p in members)
                    self.peak=max(self.peak,rss);self.samples+=1
                    self.seen.update(p.pid for p in members if p.pid!=self.proc.pid)
                except (psutil.NoSuchProcess,FileNotFoundError):pass
                self.done.wait(.005)
        self.sampler=threading.Thread(target=sample,daemon=True);self.sampler.start()
        assert self.messages.get(timeout=30)=={'ready':True}

    def members(self):
        # Read only the benchmark root's direct child list, not the system process table.
        ids=Path(f'/proc/{self.proc.pid}/task/{self.proc.pid}/children').read_text().split()
        return [self.ps]+[psutil.Process(int(pid)) for pid in ids]

    def cpu(self):
        t=self.ps.cpu_times()
        total=t.user+t.system+t.children_user+t.children_system
        for p in self.members()[1:]:
            t=p.cpu_times();total+=t.user+t.system
        return total

    def call(self, case):
        before=self.cpu();wall=time.perf_counter()
        self.proc.stdin.write(json.dumps({'id':case['id'],'path':str(ROOT/case['path'])})+'\n');self.proc.stdin.flush()
        result=self.messages.get(timeout=120)
        assert result is not None and result['id']==case['id'],result
        result['rpc_wall_seconds']=time.perf_counter()-wall
        result['cpu_seconds']=self.cpu()-before
        result['sampled_peak_rss_bytes']=self.peak
        result['backend_processes_observed']=len(self.seen)
        return result

    def close(self):
        self.proc.stdin.close()
        try:self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:os.killpg(self.proc.pid,signal.SIGKILL);self.proc.wait()
        self.done.set();self.sampler.join();self.reader.join(timeout=1);self.stderr.close()

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--out',type=Path,required=True)
    ap.add_argument('--passes',type=int,default=5);ap.add_argument('--smoke',action='store_true')
    ap.add_argument('--profiles',nargs='+',default=['legacy','reuse','reuse-low-memory'])
    ap.add_argument('--manifest',type=Path,default=ROOT/'benchmarks/ocr-profiles/manifest.json')
    a=ap.parse_args();a.out.mkdir(parents=True,exist_ok=False)
    cases=[c for c in json.loads(a.manifest.read_text()) if c['track']!='external_smoke']
    if a.smoke:cases=[c for c in cases if c['id'] in ['ocr-two-pages','swedish-a-clean','chinese-clean','native-and-scan']]
    for c in cases:
        assert sha(ROOT/c['path'])==c['sha256'];assert sha(ROOT/c['reference'])==c['reference_sha256']
    sources=['crates/bulkparse-ocr/src/lib.rs','crates/bulkparse-ocr/src/worker.rs',
             'crates/bulkparse-ocr/examples/ocr_bench.rs','backends/rapidocr/rapidocr_backend.py',
             'backends/rapidocr/profiles.py','scripts/benchmark_ocr_reuse.py']
    (a.out/'run.json').write_text(json.dumps({'created_utc':datetime.now(timezone.utc).isoformat(),
        'platform':platform.platform(),'python':platform.python_version(),'passes':a.passes,'smoke':a.smoke,
        'cases':cases,'profiles':a.profiles,'source_sha256':{p:sha(ROOT/p) for p in sources},
        'binaries':{p:sha(p) for p in ['/bench/ocr_bench','/bench/before_ocr_bench']},
        'method':'One provider per profile; excluded full-corpus warm-up then sequential measured full-corpus passes per profile. Profiles run in separate blocks. Real PDF reading, parsing, selective rendering, inference, process management and layout included; JSON serialization excluded from primary wall time, included in RPC wall time. CPU is Linux procstat user/system plus reaped-child CPU and live-child CPU, read at quiescent request boundaries. Root+direct child RSS sampled every 5ms; spikes can be missed and shared pages counted twice. No result cache. Only one provider is alive at a time; providers are closed between profile blocks. Startup/recycling after warm-up included.'},indent=2)+'\n')
    summaries=[]
    for name in a.profiles:
        folder=a.out/name;folder.mkdir()
        config=ROOT/'target/ocr-reuse/configs'/f'{name}.json'
        (folder/'config.json').write_bytes(config.read_bytes())
        binary='/bench/before_ocr_bench' if name=='legacy' else '/bench/ocr_bench'
        worker=Worker(binary,config,folder);runs=[]
        try:
            with (folder/'records.jsonl').open('w') as f:
                for run in range(a.passes+1):
                    rows=[]
                    for c in cases:
                        r=worker.call(c);value=text(r['result']) if 'result' in r else ''
                        r.update(run=run,quality=score(value,(ROOT/c['reference']).read_text()),
                                 text_sha256=hashlib.sha256(value.encode()).hexdigest())
                        f.write(json.dumps(r,ensure_ascii=False)+'\n');f.flush();rows.append(r)
                    runs.append(rows)
                    print(name,'warmup' if run==0 else f'pass {run}',round(sum(r['wall_seconds'] for r in rows),3),
                          'seconds',round(worker.peak/1048576),'MiB',flush=True)
        finally:worker.close()
        errors=[{'run':r['run'],'id':r['id'],'error':r.get('error'),'diagnostics':r.get('result',{}).get('diagnostics',[])}
                for rows in runs for r in rows if 'error' in r or any(d['code']=='pdf_ocr_error' for d in r['result']['diagnostics'])]
        summary={'profile':name,'passes':a.passes,'peak_rss_bytes':worker.peak,'memory_samples':worker.samples,
                 'backend_processes_observed':len(worker.seen),'errors':errors,
                 'quality':aggregate([r['quality'] for r in runs[1]]),
                 'by_track':{track:aggregate([r['quality'] for c,r in zip(cases,runs[1]) if c['track']==track]) for track in sorted({c['track'] for c in cases})},
                 'changed_text_across_runs':[c['id'] for i,c in enumerate(cases) if len({rows[i]['text_sha256'] for rows in runs})>1]}
        for key in ['wall_seconds','cpu_seconds','rpc_wall_seconds']:
            summary[key]=[sum(r[key] for r in rows) for rows in runs[1:]]
            summary['median_'+key]=statistics.median(summary[key])
        (folder/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');summaries.append(summary)
        (a.out/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')

if __name__=='__main__':main()
