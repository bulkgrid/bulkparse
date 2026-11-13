#!/usr/bin/env python3
"""Bounded, persistent process-pool benchmark; engine startup is outside timing.

JSONL protocol adapters initialize once per worker. One in-flight job per worker;
closed-loop dispatch, no unbounded work queue and no cached conversion results.
Requires psutil (available in the isolated comparison Python environment).
"""
import argparse
from datetime import datetime,timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import queue
import random
import shutil
import signal
import statistics
import subprocess
import threading
import time
import psutil
from evaluate_holdout import fingerprint_engine,quality
from benchmark_parsers import sha
from evaluate_fixtures import source_digest

ROOT=Path(__file__).resolve().parents[1]


def percentile(values,p):
    """Nearest-rank percentile; no interpolation hiding tail observations."""
    return sorted(values)[max(0,math.ceil(len(values)*p)-1)] if values else None


class Abort(Exception):pass


class Worker:
    def __init__(self,argv,index,events,logdir):
        self.index=index;self.started=time.perf_counter();self.pending=None
        self.stderr=(logdir/f'worker-{index}.stderr').open('wb')
        self.process=subprocess.Popen(argv,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
            stderr=self.stderr,text=True,encoding='utf-8',bufsize=1,start_new_session=True)
        self.ps=psutil.Process(self.process.pid)
        def reader():
            try:
                while True:
                    line=self.process.stdout.readline(16*1024**2+1)
                    if not line:break
                    if len(line)>16*1024**2:raise ValueError('worker response limit')
                    events.put((index,json.loads(line),time.perf_counter()))
            except Exception as error:events.put((index,{'protocol_error':str(error)},time.perf_counter()))
            finally:events.put((index,{'eof':True},time.perf_counter()))
        self.reader=threading.Thread(target=reader,daemon=True);self.reader.start()

    def send(self,request):
        self.process.stdin.write(json.dumps(request)+'\n');self.process.stdin.flush()

    def close(self):
        # Adapter owns its process group. Always reap it, including failed startup.
        if self.process.poll() is None:
            try:os.killpg(self.process.pid,signal.SIGTERM)
            except ProcessLookupError:pass
            try:self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                try:os.killpg(self.process.pid,signal.SIGKILL)
                except ProcessLookupError:pass
                self.process.wait()
        self.reader.join(timeout=1)
        self.process.stdin.close();self.process.stdout.close();self.stderr.close()


def run_pool(argv,cases,concurrency,logdir,*,duration=10,max_jobs=50000,
             job_timeout=60,startup_timeout=180,max_rss_bytes=8*1024**3,
             min_available_bytes=2*1024**3,sample_interval=.02):
    logdir.mkdir(parents=True,exist_ok=False)
    events=queue.Queue();workers=[];memory=[];samples=[];outputs={};variants=set()
    started=time.perf_counter();phase='startup';next_sample=0.;ready_times=[]
    initial_available=psutil.virtual_memory().available
    result={'status':'initializing','concurrency':concurrency,'initial_available_bytes':initial_available,
            'memory_sample_interval_ms':sample_interval*1000,'pids':[],
            'startup_seconds':None,'warmup_seconds':None,'measurement_seconds':None}
    def observe(force=False):
        nonlocal next_sample
        now=time.perf_counter()
        if not force and now<next_sample:return
        next_sample=now+sample_interval
        rss=0;cpu=0
        for worker in workers:
            try:
                rss+=worker.ps.memory_info().rss
                t=worker.ps.cpu_times();cpu+=t.user+t.system
            except psutil.NoSuchProcess:
                if worker.process.poll() is not None:raise Abort('worker_exited')
        available=psutil.virtual_memory().available
        memory.append({'elapsed_seconds':now-started,'phase':phase,'pool_rss_bytes':rss,
                       'pool_cpu_seconds':cpu,'host_available_bytes':available})
        if rss>max_rss_bytes:raise Abort('pool_rss_budget')
        if available<min_available_bytes:raise Abort('host_available_memory_floor')
    def receive(deadline):
        while True:
            observe()
            if time.perf_counter()>deadline:raise Abort('response_timeout')
            try:item=events.get(timeout=sample_interval)
            except queue.Empty:continue
            if item[1].get('eof') or item[1].get('protocol_error'):raise Abort('worker_protocol_or_exit')
            return item
    try:
        observe(True)
        # Sequential startup permits budget enforcement before loading another model.
        for index in range(concurrency):
            worker=Worker(argv,index,events,logdir);workers.append(worker)
            result['pids'].append(worker.process.pid)
            i,message,received=receive(worker.started+startup_timeout)
            if i!=index or message.get('ready') is not True:raise Abort('invalid_ready')
            ready_times.append(received-worker.started)
        result['startup_seconds']=time.perf_counter()-started
        result['worker_ready_seconds']=ready_times
        phase='warmup';warmup_start=time.perf_counter()
        for w in workers:w.send({'id':-1,'input':str(Path(cases[0]['source_path']).resolve())})
        warm=set()
        while len(warm)<concurrency:
            i,message,_=receive(warmup_start+job_timeout)
            if message.get('id')!=-1 or not message.get('ok') or i in warm:raise Abort('warmup_failed')
            warm.add(i)
        result['warmup_seconds']=time.perf_counter()-warmup_start
        observe(True);result['warm_pool_rss_bytes']=memory[-1]['pool_rss_bytes']
        cpu_start=memory[-1]['pool_cpu_seconds']
        phase='measured';measure_start=time.perf_counter();sent=0;pending={};stopping=False
        limit=max(len(cases),max_jobs//len(cases)*len(cases))
        def dispatch(worker):
            nonlocal sent
            case=cases[sent%len(cases)];job=sent;sent+=1
            dispatched=time.perf_counter()
            pending[worker.index]=(job,case,dispatched)
            worker.send({'id':job,'input':str(Path(case['source_path']).resolve())})
        for w in workers:dispatch(w)
        while pending:
            deadline=min(item[2]+job_timeout for item in pending.values())
            i,message,received=receive(deadline)
            if i not in pending:raise Abort('unexpected_worker_reply')
            job,case,dispatched=pending.pop(i)
            if message.get('id')!=job or not isinstance(message.get('ok'),bool):raise Abort('invalid_response_id')
            md=message.get('markdown','')
            if not isinstance(md,str):raise Abort('invalid_markdown')
            signature=(message['ok'],md)
            if case['id'] not in outputs:outputs[case['id']]=message
            elif signature!=(outputs[case['id']]['ok'],outputs[case['id']].get('markdown','')):variants.add(case['id'])
            samples.append({'id':case['id'],'job':job,'worker':i,'ok':message['ok'],
                'latency_ms':(received-dispatched)*1000,'service_ms':message.get('service_ms'),
                'output_bytes':len(md.encode()),'error':message.get('error')})
            if sent%len(cases)==0 and (received-measure_start>=duration or sent>=limit):stopping=True
            if not stopping:dispatch(workers[i])
        elapsed=time.perf_counter()-measure_start;observe(True)
        cpu_end=memory[-1]['pool_cpu_seconds']
        success=[s for s in samples if s['ok']]
        per_case={case['id']:sum(s['id']==case['id'] for s in samples) for case in cases}
        assert len(set(per_case.values()))==1,'unbalanced completed workload'
        result.update(status='completed',measurement_seconds=elapsed,
            stop_reason='duration' if elapsed>=duration else 'max_jobs',
            documents_per_second=len(success)/elapsed,attempts_per_second=len(samples)/elapsed,
            successful_documents=len(success),failed_documents=len(samples)-len(success),
            attempts_per_case=per_case,p50_latency_ms=percentile([s['latency_ms'] for s in success],.5),
            p95_latency_ms=percentile([s['latency_ms'] for s in success],.95),
            all_attempt_p95_latency_ms=percentile([s['latency_ms'] for s in samples],.95),
            pool_cpu_seconds=cpu_end-cpu_start,average_busy_cores=(cpu_end-cpu_start)/elapsed,
            nondeterministic_cases=sorted(variants))
    except (Abort,BrokenPipeError,psutil.Error) as error:
        result.update(status='aborted',abort_reason=str(error),completed_attempts=len(samples),
                      aborted_phase=phase,nondeterministic_cases=sorted(variants))
    finally:
        for worker in workers:worker.close()
        result['workers_reaped']=all(w.process.poll() is not None for w in workers)
    result['sampled_peak_pool_rss_bytes']=max((m['pool_rss_bytes'] for m in memory),default=0)
    result['sampled_peak_measured_pool_rss_bytes']=max((m['pool_rss_bytes'] for m in memory if m['phase']=='measured'),default=0)
    (logdir/'memory.jsonl').write_text(''.join(json.dumps(x)+'\n' for x in memory))
    (logdir/'samples.jsonl').write_text(''.join(json.dumps(x)+'\n' for x in samples))
    (logdir/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
    return result,outputs


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--engines',type=Path,default=ROOT/'benchmarks/workers/engines.json')
    p.add_argument('--manifest',type=Path,default=ROOT/'benchmarks/holdout/manifest.jsonl')
    p.add_argument('--freeze',type=Path,default=ROOT/'benchmarks/pdf-tables/freeze-v2.json')
    p.add_argument('--levels',type=int,nargs='+',default=[1,2,4,8])
    p.add_argument('--engine',action='append')
    p.add_argument('--duration',type=float,default=10)
    p.add_argument('--max-jobs',type=int,default=50000)
    p.add_argument('--max-pool-rss-mib',type=int,default=8192)
    p.add_argument('--min-available-mib',type=int,default=2048)
    p.add_argument('--out',type=Path,required=True)
    args=p.parse_args()
    if any(n<1 or n>8 for n in args.levels) or args.duration<=0 or args.max_pool_rss_mib<=0 or args.min_available_mib<0:p.error('invalid limits')
    freeze=json.loads(args.freeze.read_text());assert source_digest()==freeze['rust_source_sha256'],'parser changed'
    cases=[json.loads(s) for s in args.manifest.read_text().splitlines()]
    for case in cases:
        assert sha(case['source_path'])==case['source_sha256']
        assert sha(case['reference_path'])==case['reference_sha256']
    # Fixed shuffled cycle for every engine/level; warmup remains simple prose.
    prose=next(c for c in cases if c['id']=='synthetic/prose')
    cases.remove(prose);random.Random(20261110).shuffle(cases);cases.insert(0,prose)
    engines=[e for e in json.loads(args.engines.read_text()) if not args.engine or e['name'] in args.engine]
    if not engines or (args.engine and set(args.engine)-{e['name'] for e in engines}):p.error('unknown engine')
    engines=[fingerprint_engine(e) for e in engines]
    args.out.mkdir(parents=True,exist_ok=False)
    raw=ROOT/'target/worker-benchmarks'/args.out.name;raw.mkdir(parents=True,exist_ok=False)
    metadata={'created_utc':datetime.now(timezone.utc).isoformat(),'platform':platform.platform(),
        'python':platform.python_version(),'psutil':psutil.__version__,'logical_cpus':psutil.cpu_count(),
        'host_memory_bytes':psutil.virtual_memory().total,'manifest_sha256':sha(args.manifest),
        'freeze':freeze,'engines':engines,'case_order':[c['id'] for c in cases],
        'levels':args.levels,'duration_seconds':args.duration,'max_jobs':args.max_jobs,
        'max_pool_rss_mib':args.max_pool_rss_mib,'min_available_mib':args.min_available_mib,
        'harness_sha256':sha(__file__),'metric_sha256':sha(ROOT/'scripts/evaluate_holdout.py'),
        'scope':'Persistent isolated processes, one engine per worker and one in-flight job each. Warmup and initialization excluded. Closed-loop dispatch-to-response latency includes input read, conversion, Markdown and JSONL IPC. Success throughput excludes errors but elapsed time includes them. RSS is sampled aggregate worker RSS, shared pages may be counted multiple times; guards are polling limits, not OS hard caps. No OCR.'}
    (args.out/'run.json').write_text(json.dumps(metadata,indent=2)+'\n')
    snapshots=args.out/'harness';snapshots.mkdir()
    for path in [Path(__file__),ROOT/'scripts/worker_docling.py',ROOT/'benchmarks/workers/native.rs']:
        shutil.copyfile(path,snapshots/path.name)
    results=[]
    for engine in engines:
        for level in args.levels:
            name=f'{engine["name"]}-{level}';directory=raw/name
            print(f'Starting {name}',flush=True)
            result,outputs=run_pool(engine['argv'],cases,level,directory,duration=args.duration,
                max_jobs=args.max_jobs,max_rss_bytes=args.max_pool_rss_mib*1024**2,
                min_available_bytes=args.min_available_mib*1024**2)
            result['engine']=engine['name'];scored=[]
            for case in cases:
                output=outputs.get(case['id'])
                if output is None:
                    scored.append({'id':case['id'],'evaluated':False});continue
                md=output.get('markdown','') if output['ok'] else ''
                (directory/(str(cases.index(case))+'.md')).write_text(md)
                scored.append({'id':case['id'],'evaluated':True,'ok':output['ok'],'error':output.get('error'),
                    'reference_kind':case['reference_kind'],'output_sha256':hashlib.sha256(md.encode()).hexdigest(),
                    'quality':quality(md,Path(case['reference_path']).read_text(),case)})
            result['quality']=scored;results.append(result)
            dest=args.out/name;dest.mkdir()
            for f in ['memory.jsonl','samples.jsonl']:shutil.copyfile(directory/f,dest/f)
            (args.out/'summary.json').write_text(json.dumps(results,indent=2)+'\n')
            print(f'{name}: {result["status"]}; {result.get("documents_per_second",0):.1f} successful docs/s; RSS {result["sampled_peak_pool_rss_bytes"]/1024**2:.0f} MiB; {result.get("abort_reason","")}',flush=True)
    assert source_digest()==freeze['rust_source_sha256']
    for engine in engines:
        assert sha(engine['argv'][0])==engine['executable_sha256']
        for path,digest in engine['pin_sha256'].items():assert sha(path)==digest


if __name__=='__main__':main()
