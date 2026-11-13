#!/usr/bin/env python3
"""Validate and save manually assigned anonymous assessments with evidence hashes."""
import argparse,datetime,hashlib,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];BASE=ROOT/'target/realworld/assistant-review/packets';OUT=ROOT/'reports/realworld-2026-11-12/assistant-review/assessments'
DIMS=['completeness','structure','formatting','cleanliness']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 ap=argparse.ArgumentParser();ap.add_argument('document');a=ap.parse_args();manual=json.load(sys.stdin);packet_path=BASE/a.document/'packet.json';packet=json.loads(packet_path.read_text());labels={o['label'] for o in packet['outputs']}
 if set(manual['assessments'])!=labels:raise ValueError('Missing or extra anonymous candidate assessment')
 for o in packet['outputs']:
  assessment=manual['assessments'][o['label']];scores=assessment['scores']
  if scores is not None:
   if len(scores)!=4 or any(type(v) is not int or not 0<=v<=100 for v in scores):raise ValueError('Scores must be four integers 0–100')
   if o['bytes']==0 and any(scores):raise ValueError('Empty output cannot receive credit')
  if len(assessment.get('evidence',''))<20:raise ValueError('Concrete evidence required')
  if assessment.get('confidence') not in ['high','moderate','low','unreviewable']:raise ValueError('Confidence is required')
  assessment['dimensions']=dict(zip(DIMS,scores)) if scores is not None else None;assessment['output_sha256']=o['sha256']
  assert sha(ROOT/o['path'])==o['sha256']
 result={'document_id':a.document,'format':packet['format'],'reviewer':'Current Codex assistant; prior project context retained; not independent blind judging','reviewed_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'packet_sha256':sha(packet_path),'reference':packet['reference'],**manual}
 OUT.mkdir(parents=True,exist_ok=True);dest=OUT/(a.document+'.json')
 if dest.exists():raise ValueError('Assessment already exists; preserve original before revising')
 dest.write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n');print('Recorded',a.document,len(labels),'assessments')
if __name__=='__main__':main()
