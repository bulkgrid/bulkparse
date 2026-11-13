#!/usr/bin/env python3
"""Frozen DocLayNet heading/content checks; never changes parser code or labels."""
from bulkparse_cli import parse_command
from pathlib import Path
from collections import Counter,defaultdict
import argparse,difflib,hashlib,json,re,subprocess,unicodedata,datetime,random,statistics
from check_pdf_quality_fix import page_characters
ROOT=Path(__file__).resolve().parents[1];DATA=ROOT/'target/independent-pdf-v2';SPEC=ROOT/'benchmarks/independent-pdf-v2'
VERSIONS=['pre_reading_heading_series','previous_release','current']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def norm(s):return ''.join(c for c in unicodedata.normalize('NFKC',s).casefold() if c.isalnum())
def tokens(s):return re.findall(r'[^\W_]+',unicodedata.normalize('NFKC',s).casefold())
def strings(value):
 if isinstance(value,dict):
  if isinstance(value.get('text'),str):yield value['text']
  for k,v in value.items():
   if k not in ('text','source'):yield from strings(v)
 elif isinstance(value,list):
  for v in value:yield from strings(v)
def headings(blocks):
 out=[]
 def visit(v):
  if isinstance(v,dict):
   if v.get('type')=='heading':out.append(''.join(strings(v)))
   else:
    for k,x in v.items():
     if k!='source':visit(x)
  elif isinstance(v,list):
   for x in v:visit(x)
 visit(blocks);return out

def match_counts(pred,gold,threshold=.9):
 ps,gs=list(map(norm,pred)),list(map(norm,gold))
 edges=[]
 for p in ps:
  edges.append(sorted([(j,difflib.SequenceMatcher(None,p,g,autojunk=False).ratio()) for j,g in enumerate(gs) if p and g and (p==g if threshold==1 else difflib.SequenceMatcher(None,p,g,autojunk=False).ratio()>=threshold)],key=lambda v:-v[1]))
 owners={}
 def assign(i,seen):
  for j,score in edges[i]:
   if j in seen:continue
   seen.add(j)
   if j not in owners or assign(owners[j],seen):owners[j]=i;return True
  return False
 for i in range(len(ps)):assign(i,set())
 tp=len(owners)
 return {'tp':tp,'fp':len(ps)-tp,'fn':len(gs)-tp,'pairs':[{'prediction':i,'reference':j} for j,i in sorted(owners.items())]}
def prf(counts):
 tp,fp,fn=(counts[k] for k in ['tp','fp','fn'])
 return {'precision':tp/(tp+fp) if tp+fp else None,'recall':tp/(tp+fn) if tp+fn else None,'f1':2*tp/(2*tp+fp+fn) if 2*tp+fp+fn else None}
def refs(row,annotations):
 d=json.loads((ROOT/row['paths']['cells']).read_text());cells=d['cells'];regions=[]
 for a in annotations:
  x,y,w,h=a['bbox'];inside=[]
  for c in cells:
   cx,cy,cw,ch=c['bbox']
   if x<=cx+cw/2<=x+w and y<=cy+ch/2<=y+h:inside.append(c)
  inside.sort(key=lambda c:(c['bbox'][1],c['bbox'][0]))
  regions.append({'annotation_id':a['id'],'category_id':a['category_id'],'bbox':a['bbox'],'text':' '.join(c['text'] for c in inside),'cells':[c['text'] for c in inside]})
 gold=[r['text'] for r in regions if r['category_id'] in (8,11)]
 normalized_cells=[norm(c['text']) for c in cells];freq=Counter(normalized_cells)
 pairs=[]
 for r in regions:
  if r['category_id'] not in (1,4,10):continue
  anchors=[norm(t) for t in r['cells'] if len(norm(t))>=20 and freq[norm(t)]==1]
  pairs.extend(zip(anchors,anchors[1:]))
 return {'headings':gold,'regions':regions,'order_pairs':pairs,'text':' '.join(c['text'] for c in cells),'unscorable_heading_regions':sum(not norm(t) for t in gold)}
def clean_table(value):
 if isinstance(value,dict):return {k:clean_table(v) for k,v in value.items() if k not in ('id','source')}
 if isinstance(value,list):return [clean_table(v) for v in value]
 return value

def evaluate(pred,reference):
 blocks=pred['document']['blocks'] if pred else []
 text='\n'.join(strings(blocks));heads=headings(blocks);pt,rt=Counter(tokens(text)),Counter(tokens(reference['text']))
 normalized=norm(text);ordered=0;found=0
 for a,b in reference['order_pairs']:
  pa,pb=normalized.find(a),normalized.find(b)
  both=pa>=0 and pb>=0
  found+=both;ordered+=both and pa<pb
 return {'heading_fuzzy':match_counts(heads,reference['headings']), 'heading_exact':match_counts(heads,reference['headings'],1),'headings':heads,'reference_headings':reference['headings'],'unscorable_heading_regions':reference['unscorable_heading_regions'],'text_token_matches':sum((pt&rt).values()),'reference_tokens':sum(rt.values()),'text_token_recall':sum((pt&rt).values())/sum(rt.values()) if rt else None,'order_correct_pairs':ordered,'order_found_pairs':found,'order_reference_pairs':len(reference['order_pairs']),'order_pair_recall':ordered/len(reference['order_pairs']) if reference['order_pairs'] else None,'text_sha256':hashlib.sha256(text.encode()).hexdigest(),'normalized_text_sha256':hashlib.sha256(normalized.encode()).hexdigest(),'table_structures':[clean_table(b) for b in blocks if b.get('type')=='table']}
def aggregate(rows):
 result={}
 for label in VERSIONS:
  r=[x['versions'][label] for x in rows]
  stats={}
  for kind in ['heading_fuzzy','heading_exact']:
   counts={k:sum(v[kind][k] for v in r) for k in ['tp','fp','fn']};stats[kind]={**counts,**prf(counts)}
  stats['conversion_errors']=sum(v['exit_code'] not in (0,2) for v in r)
  stats['text_token_recall']=sum(v['text_token_matches'] for v in r)/sum(v['reference_tokens'] for v in r)
  n=sum(v['order_reference_pairs'] for v in r)
  stats['within_region_order_pair_recall']=sum(v['order_correct_pairs'] for v in r)/n if n else None
  stats['order_reference_pairs']=n
  result[label]=stats
 return result

def main():
 p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);a=p.parse_args();a.out.mkdir(parents=True,exist_ok=True)
 if (a.out/'quality.json').exists():raise ValueError('Use a fresh output report')
 freeze=json.loads((SPEC/'freeze.json').read_text())
 assert sha(SPEC/'PROTOCOL.md')==json.loads((SPEC/'protocol-freeze.json').read_text())['sha256']
 for label,v in freeze['versions'].items():
  source=ROOT/'reports'/v['report']/'source-snapshot'
  assert all(sha(source/n)==h for n,h in v['source_manifest'].items())
  if label=='current':assert all(sha(ROOT/n)==h for n,h in v['source_manifest'].items())
 manifest=json.loads((SPEC/'manifest.json').read_text());overlap=json.loads((SPEC/'overlap-audit.json').read_text())
 excluded={r['id'] for r in overlap['rows'] if r['excluded']}
 annotation_data=json.loads((DATA/'test.json').read_text());by_id=defaultdict(list)
 for ann in annotation_data['annotations']:
  if ann.get('precedence',0)==0:by_id[ann['image_id']].append(ann)
 reference_rows={};selected=[r for r in manifest['rows'] if Path(r['image']['file_name']).stem not in excluded]
 for row in selected:
  for k,h in row['sha256'].items():assert sha(ROOT/row['paths'][k])==h
  id=Path(row['image']['file_name']).stem;reference_rows[id]=refs(row,by_id[row['image']['id']])
 refpath=DATA/'references.json';refpath.write_text(json.dumps(reference_rows,indent=2)+'\n')
 binaries={label:ROOT/'target/independent-pdf-v2'/('build-'+label)/'release/bulkparse' for label in VERSIONS}
 identity={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'reference_sha256':sha(refpath),'harness_sha256':sha(Path(__file__)),'protocol_sha256':sha(SPEC/'PROTOCOL.md'),'binary_sha256':{k:sha(v) for k,v in binaries.items()},'selected_count':len(selected),'excluded_count':len(excluded),'parser_frozen':True}
 (a.out/'evaluation-start.json').write_text(json.dumps(identity,indent=2)+'\n')
 results=[]
 for row in selected:
  id=Path(row['image']['file_name']).stem;entry={'id':id,'source':row['image'],'versions':{},'comparisons':{}}
  raws={}
  for label,bin in binaries.items():
   dest=DATA/'outputs'/label;dest.mkdir(parents=True,exist_ok=True)
   parsed={}
   for raw in [False,True]:
    cmd=[str(bin), parse_command(bin),str(ROOT/row['paths']['pdfs']),'--to','json']
    if raw:cmd.append('--pdf-normalize-unicode=false')
    run=subprocess.run(cmd,capture_output=True,timeout=60)
    name=id+('.raw' if raw else '')
    (dest/(name+'.json')).write_bytes(run.stdout);(dest/(name+'.stderr')).write_bytes(run.stderr)
    value=json.loads(run.stdout) if run.stdout else None
    parsed[raw]=(run.returncode,value)
   code,value=parsed[False];rawcode,raw=parsed[True];raws[label]=(rawcode,raw)
   scores=evaluate(value,reference_rows[id]);scores['exit_code']=code;scores['raw_exit_code']=rawcode
   scores['prediction_json_sha256']=sha(dest/(id+'.json'));scores['raw_json_sha256']=sha(dest/(id+'.raw.json'))
   scores['status']=value.get('status') if value else None
   entry['versions'][label]=scores
  for old_label in VERSIONS[:-1]:
   oc,old=raws[old_label];nc,new=raws['current'];prev=entry['versions'][old_label];now=entry['versions']['current']
   if old and new:
    before,after=page_characters(old['document']),page_characters(new['document']);pages=[]
    for page in sorted(set(before)|set(after),key=str):
     removed=before.get(page,Counter())-after.get(page,Counter());added=after.get(page,Counter())-before.get(page,Counter())
     if removed or added:pages.append({'page':page,'removed':dict(removed),'added':dict(added)})
    inspection_equal=old['pdf_inspection']==new['pdf_inspection']
    diagnostics=[d for d in new['diagnostics'] if d['code']=='pdf_offpage_text']
   else:pages=None;inspection_equal=False;diagnostics=[]
   entry['comparisons'][old_label]={'native_character_changes':pages,'inspection_equal':inspection_equal,'status_equal':oc==nc,'offpage_diagnostics':diagnostics,'headings_changed':prev['headings']!=now['headings'],'normalized_text_order_changed':prev['normalized_text_sha256']!=now['normalized_text_sha256'],'table_structure_changed':prev['table_structures']!=now['table_structures'],'delta_heading_tp':now['heading_fuzzy']['tp']-prev['heading_fuzzy']['tp'],'delta_heading_fp':now['heading_fuzzy']['fp']-prev['heading_fuzzy']['fp'],'delta_heading_fn':now['heading_fuzzy']['fn']-prev['heading_fuzzy']['fn'],'delta_text_token_recall':now['text_token_recall']-prev['text_token_recall'] if now['text_token_recall'] is not None and prev['text_token_recall'] is not None else None,'delta_order_pair_recall':now['order_pair_recall']-prev['order_pair_recall'] if now['order_pair_recall'] is not None and prev['order_pair_recall'] is not None else None}
  results.append(entry)
  print('Evaluated',len(results),'/',len(selected),row['image']['doc_category'],flush=True)
  (a.out/'partial-quality.json').write_text(json.dumps(results,indent=2)+'\n')
 summary=aggregate(results);categories={cat:aggregate([r for r in results if r['source']['doc_category']==cat]) for cat in sorted(set(r['source']['doc_category'] for r in results))}
 rng=random.Random('bulkparse-independent-v2-bootstrap');deltas={k:[] for k in VERSIONS[:-1]}
 for _ in range(2000):
  sample=[rng.choice(results) for _ in results];agg=aggregate(sample)
  for old in deltas:deltas[old].append(agg['current']['heading_fuzzy']['f1']-agg[old]['heading_fuzzy']['f1'])
 ci={k:{'delta':summary['current']['heading_fuzzy']['f1']-summary[k]['heading_fuzzy']['f1'],'paired_bootstrap_95_percentile':[sorted(v)[49],sorted(v)[1949]],'draws':2000} for k,v in deltas.items()}
 report={'identity':identity,'summary':summary,'categories':categories,'heading_f1_change':ci,'documents':results}
 (a.out/'quality.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps({'summary':summary,'heading_f1_change':ci},indent=2))
if __name__=='__main__':main()
