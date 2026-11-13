#!/usr/bin/env python3
"""Evaluate existing checks on applicable outputs and compare identical subsets."""
import argparse,json,re,statistics
from pathlib import Path
from benchmark_opendataloader import ROOT,sha256,write_json

def main():
    ap=argparse.ArgumentParser();ap.add_argument('run',type=Path);a=ap.parse_args();out=a.run.resolve()
    summary=json.loads((out/'summary.json').read_text());manifest=json.loads((out/'manifest.json').read_text())
    runs={e:[json.loads((out/f'{e}-round-{i}.json').read_text())['documents'] for i in range(1,6)] for e in summary}
    spec=json.loads((ROOT/'reports/multiformat-legacy-layout-2026-11-12/quality-checks.json').read_text())
    check_results=[]
    for c in spec['checks']:
        assert sha256(ROOT/c['path'])==c['source']['sha256']
        outcomes={}
        for e,passes in runs.items():
            path=str(ROOT/c['path']);matching=[[r for r in rs if r['path']==path] for rs in passes]
            if not matching[0]:continue
            verdicts=[]
            for match in matching:
                r=match[0];md=re.sub(r'\\([\\`*_{}\[\]<>#!|~+\-=.])',r'\1',r['markdown'])
                missing=[p for p in c['required_patterns'] if not re.search(p,md)]
                forbidden=[p for p in c['forbidden_patterns'] if re.search(p,md)]
                ids=set(re.findall(r'<a\s+id="([^"]+)"',md))
                for label in c.get('resolved_links',[]):
                    links=re.findall(r'\['+re.escape(label)+r'\]\(#([^)]+)\)',md)
                    if not links or not all(x in ids for x in links):missing.append('resolved anchor for '+label)
                verdicts.append(dict(passed=bool(r['ok'] and not missing and not forbidden),missing=missing,forbidden=forbidden))
            outcomes[e]=dict(passed_all_measured_passes=all(v['passed'] for v in verdicts),verdicts=verdicts)
        check_results.append(dict(id=c['id'],path=c['path'],area=c['area'],results=outcomes))
    checks={e:dict(passed=sum(r['results'][e]['passed_all_measured_passes'] for r in check_results if e in r['results']),applicable=sum(e in r['results'] for r in check_results),total_available=len(check_results)) for e in summary}
    shared=[]
    for names in [('bulkparse','anydoc'),('bulkparse','anydoc','docling','markitdown'),tuple(summary)]:
        attempted=set.intersection(*[{r['path'] for r in runs[e][0]} for e in names])
        returned={p for p in attempted if all(all(next(r for r in rs if r['path']==p)['ok'] for rs in runs[e]) for e in names)}
        times={}
        for e in names:
            times[e]=dict(median_attempted_seconds=statistics.median(sum(r['elapsed_ns'] for r in rs if r['path'] in attempted)/1e9 for rs in runs[e]),median_common_returned_seconds=statistics.median(sum(r['elapsed_ns'] for r in rs if r['path'] in returned)/1e9 for rs in runs[e]))
        shared.append(dict(engines=names,attempted=len(attempted),returned_by_every_engine_every_pass=len(returned),paths_attempted=sorted(attempted),paths_returned=sorted(returned),times=times))
    old={r['path']:r for r in json.loads((ROOT/'reports/multiformat-legacy-layout-2026-11-12/benchmark-final/bulkparse-pass-1.json').read_text())['documents']}
    changed=[]
    for r in runs['bulkparse'][0]:
        name=str(Path(r['path']).relative_to(ROOT))
        if name in old and old[name]['markdown']!=r['markdown']:changed.append(name)
    write_json(out/'quality-checks.json',spec);write_json(out/'quality-results.json',dict(totals=checks,checks=check_results));write_json(out/'shared-subsets.json',shared);write_json(out/'prior-output-comparison.json',dict(previous_documents=len(old),changed=changed))
    print(json.dumps(dict(checks=checks,shared=[{k:v for k,v in x.items() if not k.startswith('paths')} for x in shared],previous_outputs_changed=changed),indent=2))

if __name__=='__main__':main()
