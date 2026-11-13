#!/usr/bin/env python3
"""Explicitly approved submission only. Default is validation with zero API calls."""
import argparse,hashlib,json,os,math
from datetime import datetime,timezone
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,v):
 temp=p.with_suffix('.tmp');temp.write_text(json.dumps(v,indent=2)+'\n');temp.replace(p)
def validate(folder,approval,budget):
 p=folder/'submission-manifest.json';manifest=json.loads(p.read_text());cost=json.loads((folder/'cost-estimate.json').read_text())
 if sha(p)!=approval:raise ValueError('Approval digest does not match the prepared jobs')
 for f,k in [('blind-mapping.json','mapping_sha256'),('cost-estimate.json','cost_estimate_sha256')]:
  if sha(folder/f)!=manifest[k]:raise ValueError('Prepared evidence changed: '+f)
 if not math.isfinite(budget) or budget<cost['conservative_planning_ceiling_usd']:raise ValueError('Approved budget is below the conservative planning ceiling; revise the scope and obtain a new approval')
 for b in manifest['batches']:
  p=ROOT/b['path']
  if sha(p)!=b['sha256'] or p.stat().st_size!=b['bytes']:raise ValueError('Prepared batch changed')
 return manifest,cost
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--prepared',type=Path,required=True);ap.add_argument('--approved-manifest-sha256',required=True);ap.add_argument('--approved-budget-usd',type=float,required=True);ap.add_argument('--submit',action='store_true');a=ap.parse_args()
 manifest,cost=validate(a.prepared,a.approved_manifest_sha256,a.approved_budget_usd)
 if not a.submit:print('Validated. No uploads or API calls made. Submission requires --submit after explicit user approval.');return
 key=os.environ.get('OPENAI_API_KEY')
 if not key:raise SystemExit('Configure OPENAI_API_KEY securely before submission; no requests made.')
 import requests
 session=requests.Session();session.headers['Authorization']='Bearer '+key
 journal_path=a.prepared/'submission-journal.json'
 if journal_path.exists():raise SystemExit('Submission journal already exists. Inspect recorded job IDs and reconcile any uncertain request; never blindly resubmit.')
 journal={'approved_manifest_sha256':a.approved_manifest_sha256,'approved_budget_usd':a.approved_budget_usd,'started_utc':datetime.now(timezone.utc).isoformat(),'jobs':[]}
 fd=os.open(journal_path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
 with os.fdopen(fd,'w') as f:json.dump(journal,f,indent=2)
 for b in manifest['batches']:
  entry={'path':b['path'],'state':'upload_in_progress'};journal['jobs'].append(entry);write(journal_path,journal)
  with (ROOT/b['path']).open('rb') as f:r=session.post('https://api.openai.com/v1/files',data={'purpose':'batch'},files={'file':(Path(b['path']).name,f,'application/jsonl')},timeout=240)
  r.raise_for_status();entry.update(file_id=r.json()['id'],state='uploaded');write(journal_path,journal)
  entry['state']='submission_in_progress';write(journal_path,journal)
  # No automatic retry: an ambiguous response must be reconciled first to avoid double billing.
  r=session.post('https://api.openai.com/v1/batches',json={'input_file_id':entry['file_id'],'endpoint':'/v1/responses','completion_window':'24h','metadata':{'study':'bulkparse-realworld-2026-11-12','approval_sha256':a.approved_manifest_sha256}},timeout=120)
  r.raise_for_status();entry.update(batch_id=r.json()['id'],state='submitted');write(journal_path,journal);print('Submitted',entry['batch_id'],flush=True)
if __name__=='__main__':main()
