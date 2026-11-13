#!/usr/bin/env python3
"""Validate both blind orders before publishing any aggregate quality score."""
import argparse,collections,hashlib,json,statistics
from pathlib import Path
DIMS=['completeness','structure','formatting','cleanliness']
def write(p,x):p.write_text(json.dumps(x,indent=2,ensure_ascii=False)+'\n')
def aggregate(mapping,verdicts):
 expected={r['custom_id'] for r in mapping}
 if set(verdicts)!=expected:raise ValueError(f'Incomplete or foreign verdict set: missing {len(expected-set(verdicts))}, extra {len(set(verdicts)-expected)}')
 pairs=collections.defaultdict(list)
 for row in mapping:
  v=verdicts[row['custom_id']]
  if type(v.get('reference_usable')) is not bool:raise ValueError('Invalid reference usability')
  for side in ['A','B']:
   scores=v.get(side,{})
   for d in DIMS:
    if type(scores.get(d)) is not int or not 0<=scores[d]<=100:raise ValueError('Missing or out-of-range score')
   if v['reference_usable'] and row[side+'_metadata']['normalized_chars']==0 and any(scores[d] for d in DIMS):raise ValueError('Empty extraction received nonzero scores')
  pairs[row['pair_id']].append((row,v))
 docs=collections.defaultdict(list);issues=[];review=[];excluded=[]
 for pair,rs in pairs.items():
  if len(rs)!=2 or {r['order'] for r,v in rs}!={0,1}:raise ValueError('A pair does not have both orders')
  if rs[0][0]['A']!=rs[1][0]['B'] or rs[0][0]['B']!=rs[1][0]['A']:raise ValueError('Pair order was not swapped')
  r0=rs[0][0];item={'pair_id':pair,'document_id':r0['document_id'],'format':r0['format'],'engines':[r0['A'],r0['B']]}
  if not all(v['reference_usable'] for r,v in rs):excluded.append({**item,'reasons':[v['reference_problem'] for r,v in rs]});continue
  gap=0
  for e in item['engines']:
   values=[v['A' if r['A']==e else 'B'] for r,v in rs]
   scores={d:statistics.mean(v[d] for v in values) for d in DIMS};gap=max(gap,max(abs(values[0][d]-values[1][d]) for d in DIMS))
   docs[(e,r0['document_id'],r0['format'])].append(scores)
  if gap>15:issues.append({**item,'max_dimension_order_gap':gap})
  review.append(item)
 by_engine=collections.defaultdict(list)
 for (e,doc,fmt),rows in docs.items():by_engine[e].append({'document_id':doc,'format':fmt,'pairings':len(rows),**{d:statistics.mean(r[d] for r in rows) for d in DIMS}})
 summary={}
 for e,rows in by_engine.items():
  formats={}
  for f in sorted({r['format'] for r in rows}):
   fr=[r for r in rows if r['format']==f];scores={d:statistics.mean(r[d] for r in fr) for d in DIMS};formats[f]={'docs_judged':len(fr),'score':statistics.mean(scores.values()),**scores}
  dims={d:statistics.mean(r[d] for r in formats.values()) for d in DIMS}
  summary[e]={'docs_judged':len(rows),'formats_judged':len(formats),'score':statistics.mean(dims.values()),**dims,'per_format':formats,'documents':rows}
 seeded=sorted(review,key=lambda x:hashlib.sha256(('human-audit-v1|'+x['pair_id']).encode()).hexdigest())[:20]
 return {'summary':summary,'excluded_pairs':excluded,'large_order_disagreements':issues,'human_review_queue':seeded+issues,'human_review_completed':False}
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--prepared',type=Path,required=True);ap.add_argument('--results',type=Path,nargs='+',required=True);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
 manifest=json.loads((a.prepared/'submission-manifest.json').read_text());mapping_path=a.prepared/'blind-mapping.json'
 assert hashlib.sha256(mapping_path.read_bytes()).hexdigest()==manifest['mapping_sha256']
 mapping=json.loads(mapping_path.read_text());verdicts={};failures=[];usage=collections.Counter()
 for p in a.results:
  for line in p.read_text().splitlines():
   row=json.loads(line);cid=row['custom_id']
   if cid in verdicts:raise ValueError('Duplicate verdict: '+cid)
   resp=row.get('response') or {};body=resp.get('body') or {}
   if row.get('error') or resp.get('status_code')!=200 or body.get('status')!='completed':failures.append({'custom_id':cid,'error':row.get('error') or body.get('error') or body.get('incomplete_details')});continue
   text=''.join(c.get('text','') for item in body.get('output',[]) if item.get('type')=='message' for c in item.get('content',[]) if c.get('type')=='output_text')
   try:verdicts[cid]=json.loads(text)
   except (ValueError,TypeError):failures.append({'custom_id':cid,'error':'No valid structured JSON verdict'});continue
   for k in ['input_tokens','output_tokens','total_tokens']:usage[k]+=body.get('usage',{}).get(k,0)
 a.out.mkdir(parents=True,exist_ok=False);write(a.out/'api-failures.json',failures)
 if failures:raise ValueError(f'{len(failures)} API/verdict failures. Do not publish partial scores or retry paid jobs without a new cost review.')
 result=aggregate(mapping,verdicts);write(a.out/'quality.json',result);write(a.out/'usage.json',dict(usage));write(a.out/'verdicts.json',verdicts)
if __name__=='__main__':main()
