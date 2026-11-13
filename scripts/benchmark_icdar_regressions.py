#!/usr/bin/env python3
"""Rerun the now-observed ICDAR table regression set with unchanged scoring."""
from bulkparse_cli import parse_command
import argparse,datetime,hashlib,html,json,statistics,subprocess,sys
from collections import Counter,defaultdict
from pathlib import Path
from evaluate_independent_pdf import strings,tokens
ROOT=Path(__file__).resolve().parents[1];SPEC=ROOT/'benchmarks/icdar2013-tables'
sys.path.insert(0,str(ROOT/'target/opendataloader-bench/src'))
from evaluator_table import TEDSEvaluator

from benchmark_icdar_tables import sha,read,write,prediction_tables,score_page,aggregate

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    for k in ['before-cli','after-cli','before-worker','after-worker','before-source-report','after-source-report','out']:ap.add_argument('--'+k,type=Path,required=True)
    ap.add_argument('--before-study-report',type=Path,default=ROOT/'reports/icdar-generalization-2026-11-11')
    a=ap.parse_args();out=a.out.resolve()
    historical=read(a.before_study_report/'quality.json')
    old_identity=read(a.before_study_report/'identity.json')
    old_rows={r['id']:r for r in historical['documents']}
    if out.exists():raise ValueError('Use a fresh output directory')
    freeze=read(SPEC/'freeze.json')
    for name,digest in freeze['files'].items():assert sha(SPEC/name)==digest
    manifest=read(SPEC/'manifest.json');rows=[r for r in manifest['rows'] if not r['excluded']];refs=read(SPEC/'references.json')
    binaries={'before':a.before_cli.resolve(),'current':a.after_cli.resolve()};workers={'before':a.before_worker.resolve(),'current':a.after_worker.resolve()}
    assert sha(binaries['before'])==old_identity['cli_sha256']['current']
    assert sha(workers['before'])==old_identity['worker_sha256']['current']
    sources={}
    for v,p in [('before',a.before_source_report),('current',a.after_source_report)]:
        sources[v]=read(p/'source-manifest.json')
        for name,digest in sources[v].items():
            assert sha(p/'source-snapshot'/name)==digest
            if v=='current':assert sha(ROOT/name)==digest
    scratch=ROOT/'target/table-boundaries'/out.name
    write(out/'identity.json',{'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sample_is_now_observed':True,'historical_report':str(a.before_study_report),'scoring_harness_sha256':sha(ROOT/'scripts/benchmark_icdar_tables.py'),'protocol_freeze_sha256':sha(SPEC/'freeze.json'),'harness_sha256':sha(Path(__file__)),'evaluator_sha256':sha(ROOT/'target/opendataloader-bench/src/evaluator_table.py'),'cli_sha256':{v:sha(p) for v,p in binaries.items()},'worker_sha256':{v:sha(p) for v,p in workers.items()},'source_manifests':sources,'scratch':str(scratch.relative_to(ROOT)),'command':sys.argv})
    results=[];pages_by_version=defaultdict(list)
    for row in rows:
        assert sha(ROOT/row['pdf'])==row['pdf_sha256'] and sha(ROOT/row['xml'])==row['xml_sha256']
        result={'id':row['id'],'versions':{}};gold=refs[row['id']]
        for v,binary in binaries.items():
            dest=scratch/'json'/v/(row['id']+'.json');dest.parent.mkdir(parents=True,exist_ok=True)
            try:
                run=subprocess.run([str(binary), parse_command(binary),str(ROOT/row['pdf']),'--to','json'],capture_output=True,timeout=120)
                dest.write_bytes(run.stdout);dest.with_suffix('.stderr').write_bytes(run.stderr)
                value=json.loads(run.stdout) if run.stdout else None
                error=None if value else 'conversion failed';code=run.returncode
            except subprocess.TimeoutExpired:value=None;error='conversion timeout';code=None
            predicted=prediction_tables(value);pages=[]
            for page in sorted({t['page'] for t in gold}):
                g=[t for t in gold if t['page']==page];p=[t for t in predicted if t['page']==page]
                score=score_page(g,p);score.update(page=page,id=row['id']);pages.append(score)
            pages_by_version[v].extend(pages)
            result['versions'][v]={'pages':pages,'unannotated_page_predictions':sum(t['page'] not in {g['page'] for g in gold} for t in predicted),'error':error,'exit_code':code,'status':value.get('status') if value else None,'prediction_sha256':sha(dest) if dest.exists() else None,'tables':predicted}
        assert result['versions']['before']['prediction_sha256']==old_rows[row['id']]['versions']['current']['prediction_sha256']
        assert result['versions']['before']['pages']==old_rows[row['id']]['versions']['current']['pages']
        results.append(result)
        write(out/'quality.json',{'complete':False,'documents':results})
        print(row['id'],'evaluated',flush=True)
    summary={v:aggregate(pages_by_version[v]) for v in binaries}
    assert summary['before']==historical['summary']['current']
    write(out/'quality.json',{'complete':True,'sample_is_now_observed':True,'fresh_before_reproduces_saved_quality_and_json':True,'summary':summary,'documents':results});print(json.dumps(summary,indent=2),flush=True)
    processes={};logs={};runs={v:[] for v in workers};paths=sorted(str((ROOT/r['pdf']).resolve()) for r in rows)
    try:
        for v,worker in workers.items():
            logs[v]=open(out/(v+'-worker.stderr'),'w');processes[v]=subprocess.Popen([str(worker)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=logs[v],text=True)
        for i in range(6):
            order=list(workers) if i%2==0 else list(reversed(workers))
            for v in order:
                dest=scratch/'markdown'/v;p=processes[v]
                p.stdin.write(json.dumps({'documents':paths,'output_dir':str(dest)})+'\n');p.stdin.flush()
                run=json.loads(p.stdout.readline());run.update(round=i,warmup=i==0,version_order=order,prediction_sha256={p.name:sha(p) for p in dest.glob('*.md')})
                assert len(run['documents'])==len(paths)
                if runs[v]:assert run['prediction_sha256']==runs[v][0]['prediction_sha256']
                runs[v].append(run)
        write(out/'timing.json',{'documents':len(rows),'runs':runs,'summary':{v:{'median_seconds':statistics.median(r['elapsed_ns'] for r in rs[1:])/1e9,'measured_seconds':[r['elapsed_ns']/1e9 for r in rs[1:]],'errors':sum(not d['ok'] for r in rs for d in r['documents'])} for v,rs in runs.items()}})
    finally:
        for v,p in processes.items():p.stdin.close();assert p.wait(timeout=15)==0;logs[v].close()
    assert sha(Path(__file__))==read(out/'identity.json')['harness_sha256']
if __name__=='__main__':main()
