#!/usr/bin/env python3
"""Evaluate the frozen independent cell-structure test; never tune the parser."""
from bulkparse_cli import parse_command
import argparse,datetime,hashlib,html,json,statistics,subprocess,sys
from collections import Counter,defaultdict
from pathlib import Path
from evaluate_independent_pdf import strings,tokens
ROOT=Path(__file__).resolve().parents[1];SPEC=ROOT/'benchmarks/icdar2013-tables'
sys.path.insert(0,str(ROOT/'target/opendataloader-bench/src'))
from evaluator_table import TEDSEvaluator

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_text())
def write(p,v):p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(v,indent=2,ensure_ascii=False)+'\n')

def canonical(cells):
    """Canonical rectangular grid with occupied span slots omitted from HTML."""
    occupied={};origins={}
    for c in cells:
        r,k=c['row'],c['col'];rs,cs=c['rowspan'],c['colspan']
        assert min(r,k)>=0 and min(rs,cs)>0
        for i in range(r,r+rs):
            for j in range(k,k+cs):
                assert (i,j) not in occupied,'overlapping cells'
                occupied[i,j]=(r,k)
        origins[r,k]=c
    if not occupied:return '<html><body><table></table></body></html>'
    nr=max(i for i,j in occupied)+1;nc=max(j for i,j in occupied)+1
    assert nr*nc<=1000000
    rows=[]
    for r in range(nr):
        cells_html=[]
        for k in range(nc):
            if (r,k) in occupied and occupied[r,k]!=(r,k):continue
            c=origins.get((r,k),{'rowspan':1,'colspan':1,'text':''})
            cells_html.append(f'<td rowspan="{c["rowspan"]}" colspan="{c["colspan"]}">{html.escape(c["text"])}</td>')
        rows.append('<tr>'+''.join(cells_html)+'</tr>')
    return '<html><body><table>'+''.join(rows)+'</table></body></html>'

def prediction_tables(value):
    result=[]
    for block in (value or {}).get('document',{}).get('blocks',[]):
        if block.get('type')!='table':continue
        cells=[];occupied=set()
        for r,row in enumerate(block['rows']):
            k=0
            for cell in row:
                while (r,k) in occupied:k+=1
                # Logical provenance records the column after covered slots.
                k=cell.get('source',{}).get('cell',k+1)-1
                rs=cell.get('row_span',1);cs=cell.get('col_span',1)
                cells.append({'row':r,'col':k,'rowspan':rs,'colspan':cs,'text':'\n'.join(strings(cell.get('blocks',[])))})
                occupied.update((i,j) for i in range(r,r+rs) for j in range(k,k+cs));k+=cs
        result.append({'page':block['source']['page'],'cells':cells,'block_id':block.get('id')})
    return result

def assignment(weights):
    """Hungarian minimum-cost assignment with zero-weight dummy rows/columns."""
    nr=len(weights);nc=len(weights[0]) if nr else 0;n=max(nr,nc)
    if not n:return []
    a=[[0.]*(n+1) for _ in range(n+1)]
    for i,row in enumerate(weights):
        for j,w in enumerate(row):a[i+1][j+1]=-w
    u=[0.]*(n+1);v=[0.]*(n+1);p=[0]*(n+1);way=[0]*(n+1)
    for i in range(1,n+1):
        p[0]=i;j0=0;minv=[float('inf')]*(n+1);used=[False]*(n+1)
        while True:
            used[j0]=True;i0=p[j0];delta=float('inf');j1=0
            for j in range(1,n+1):
                if used[j]:continue
                cur=a[i0][j]-u[i0]-v[j]
                if cur<minv[j]:minv[j]=cur;way[j]=j0
                if minv[j]<delta:delta=minv[j];j1=j
            for j in range(n+1):
                if used[j]:u[p[j]]+=delta;v[j]-=delta
                else:minv[j]-=delta
            j0=j1
            if not p[j0]:break
        while True:
            j1=way[j0];p[j0]=p[j1];j0=j1
            if not j0:break
    return [(p[j]-1,j-1) for j in range(1,n+1) if 0<p[j]<=nr and j<=nc]

def table_tokens(t):return Counter(tokens(' '.join(c['text'] for c in t['cells'])))
def score_page(gold,pred):
    gt=[table_tokens(t) for t in gold];pt=[table_tokens(t) for t in pred]
    weights=[]
    for a in gt:
        row=[]
        for b in pt:
            denom=sum(a.values())+sum(b.values());f=2*sum((a&b).values())/denom if denom else 0
            row.append(f if f>=.1 else 0)
        weights.append(row)
    pairs=[];evaluators=[TEDSEvaluator(),TEDSEvaluator(structure_only=True)]
    for i,j in assignment(weights):
        if weights[i][j]<.1:continue
        pair={'gold':i,'prediction':j,'text_f1':weights[i][j]}
        for metric,e in zip(['teds','teds_s'],evaluators):pair[metric]=e.evaluate(canonical(pred[j]['cells']),canonical(gold[i]['cells']))
        pairs.append(pair)
    den=max(len(gold),len(pred));return {'gold':len(gold),'predicted':len(pred),'matched':len(pairs),'denominator':den,'pairs':pairs,**{m:sum(p[m] for p in pairs)/den if den else None for m in ['teds','teds_s']}}

def aggregate(pages):
    den=sum(p['denominator'] for p in pages);matches=sum(p['matched'] for p in pages);ng=sum(p['gold'] for p in pages);np=sum(p['predicted'] for p in pages)
    sums={m:sum(pair[m] for p in pages for pair in p['pairs']) for m in ['teds','teds_s']}
    return {'pages':len(pages),'gold_regions':ng,'predicted_regions':np,'matched_regions':matches,'detection_precision':matches/np if np else 0,'detection_recall':matches/ng if ng else 0,'end_to_end':{m:s/den if den else None for m,s in sums.items()},'macro_page':{m:statistics.mean(p[m] for p in pages) if pages else None for m in sums},'matched_only':{m:s/matches if matches else None for m,s in sums.items()}}

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    for k in ['before-cli','after-cli','before-worker','after-worker','before-source-report','after-source-report','out']:ap.add_argument('--'+k,type=Path,required=True)
    a=ap.parse_args();out=a.out.resolve()
    if out.exists():raise ValueError('Use a fresh output directory')
    freeze=read(SPEC/'freeze.json')
    for name,digest in freeze['files'].items():assert sha(SPEC/name)==digest
    manifest=read(SPEC/'manifest.json');rows=[r for r in manifest['rows'] if not r['excluded']];refs=read(SPEC/'references.json')
    binaries={'before':a.before_cli.resolve(),'current':a.after_cli.resolve()};workers={'before':a.before_worker.resolve(),'current':a.after_worker.resolve()}
    sources={}
    for v,p in [('before',a.before_source_report),('current',a.after_source_report)]:
        sources[v]=read(p/'source-manifest.json')
        for name,digest in sources[v].items():
            assert sha(p/'source-snapshot'/name)==digest
            if v=='current':assert sha(ROOT/name)==digest
    scratch=ROOT/'target/inspector-generalization'/out.name
    write(out/'identity.json',{'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'protocol_freeze_sha256':sha(SPEC/'freeze.json'),'harness_sha256':sha(Path(__file__)),'evaluator_sha256':sha(ROOT/'target/opendataloader-bench/src/evaluator_table.py'),'cli_sha256':{v:sha(p) for v,p in binaries.items()},'worker_sha256':{v:sha(p) for v,p in workers.items()},'source_manifests':sources,'scratch':str(scratch.relative_to(ROOT)),'command':sys.argv})
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
        results.append(result)
        write(out/'quality.json',{'complete':False,'documents':results})
        print(row['id'],'evaluated',flush=True)
    summary={v:aggregate(pages_by_version[v]) for v in binaries}
    write(out/'quality.json',{'complete':True,'summary':summary,'documents':results});print(json.dumps(summary,indent=2),flush=True)
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
