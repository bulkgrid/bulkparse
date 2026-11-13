#!/usr/bin/env python3
"""Evaluate transparent source-grounded Office development checks and all 67 partials."""
import argparse,collections,hashlib,json,re,zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def read(p):return json.loads(p.read_text())
def write(p,data):p.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--run',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);ap.add_argument('--baseline',type=Path,default=ROOT/'reports/multiformat-2026-11-11/benchmark-final');ap.add_argument('--spec',type=Path,default=ROOT/'reports/multiformat-quality-2026-11-12/quality-checks.json');args=ap.parse_args()
 baseline=args.baseline
 spec=read(args.spec)
 rows={'before':read(baseline/'bulkparse-pass-1.json')['documents'],'after':read(args.run/'bulkparse-pass-1.json')['documents'],'anydoc':read(args.run/'anydoc-pass-1.json')['documents']}
 maps={k:{r['path']:r for r in rs} for k,rs in rows.items()}
 results=[]
 for check in spec['checks']:
  path=ROOT/check['path'];assert hashlib.sha256(path.read_bytes()).hexdigest()==check['source']['sha256']
  if check['source'].get('part_sha256'):
   with zipfile.ZipFile(path) as z:
    for part,digest in check['source']['part_sha256'].items():assert hashlib.sha256(z.read(part)).hexdigest()==digest
  outcome={}
  for engine,m in maps.items():
   row=m[check['path']];text=re.sub(r'\\([\\`*_{}\[\]<>#!|~+\-=.])',r'\1',row['markdown'])
   missing=[p for p in check['required_patterns'] if not re.search(p,text)]
   forbidden=[p for p in check['forbidden_patterns'] if re.search(p,text)]
   ids=set(re.findall(r'<a\s+id="([^"]+)"',text))
   for label in check.get('resolved_links',[]):
    links=re.findall(r'\['+re.escape(label)+r'\]\(#([^)]+)\)',text)
    if not links or not all(link in ids for link in links):missing.append('resolved anchor for '+label)
   outcome[engine]={'passed':bool(row['ok'] and not missing and not forbidden),'missing':missing,'forbidden':forbidden}
  results.append({**check,'results':outcome})
 byarea={area:{k:sum(r['results'][k]['passed'] for r in results if r['area']==area) for k in maps}|{'checks':sum(r['area']==area for r in results)} for area in dict.fromkeys(r['area'] for r in results)}
 totals={k:sum(r['results'][k]['passed'] for r in results) for k in maps}
 losses=[r['id'] for r in results if r['results']['before']['passed'] and not r['results']['after']['passed']]
 cohorts={name:{k:sum(r['results'][k]['passed'] for r in results if r.get('cohort','selected')==name) for k in maps}|{'checks':sum(r.get('cohort','selected')==name for r in results)} for name in dict.fromkeys(r.get('cohort','selected') for r in results)}
 report={'cohorts':cohorts,'baseline':str(baseline),'scope':spec['scope'],'checks':len(results),'documents':len({r['path'] for r in results}),'totals':totals,'by_area':byarea,'regressed_checks':losses,'results':results}
 args.out.mkdir(parents=True,exist_ok=True);write(args.out/'quality-results.json',report)
 audit=[]
 for old in rows['before']:
  if old.get('status')!='partial':continue
  new=maps['after'][old['path']]
  diag=lambda r: sorted({d['code'] for d in r.get('diagnostics',[]) if d.get('content_loss')})
  messages={d['code']:d['message'] for d in new.get('diagnostics',[]) if d.get('content_loss')}
  audit.append({'path':old['path'],'before_status':old['status'],'after_status':new['status'],'markdown_changed':old['markdown']!=new['markdown'],'before_codes':diag(old),'remaining_gaps':messages,'resolved_codes':sorted(set(diag(old))-set(diag(new))),'new_codes':sorted(set(diag(new))-set(diag(old))),'quality_checks':[r['id'] for r in results if r['path']==old['path']]})
 assert len(audit)==sum(r.get('status')=='partial' for r in rows['before'])
 write(args.out/'partial-audit-after.json',audit)
 stats={'conversion_documents':len(rows['after']),'returned':sum(r['ok'] for r in rows['after']),'statuses':dict(collections.Counter(r.get('status') for r in rows['after'])),'changed_outputs':sum(maps['after'][r['path']]['markdown']!=r['markdown'] for r in rows['before']),'original_partial_outputs_changed':sum(r['markdown_changed'] for r in audit)}
 write(args.out/'change-summary.json',stats)
 lines=[f'# Audit of the {len(audit)} baseline partial outputs','','Each row records the before/after output change and remaining declared gaps. A changed output is not automatically an improvement. Source-grounded checks are reported separately.','', '| Document | Output changed | Status now | Remaining gaps |','| --- | --- | --- | --- |']
 for r in audit:lines.append(f"| {r['path']} | {'yes' if r['markdown_changed'] else 'no'} | {r['after_status']} | {'; '.join(r['remaining_gaps']) or 'none detected'} |")
 (args.out/'partial-audit.md').write_text('\n'.join(lines)+'\n')
 print(json.dumps({'totals':totals,'regressed_checks':losses,'changes':stats,'by_area':byarea},indent=2))
if __name__=='__main__':main()
