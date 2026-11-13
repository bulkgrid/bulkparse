#!/usr/bin/env python3
"""Anonymous reading copies for same-chat assistant review, not independent blinding."""
import hashlib,json,os,secrets
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];REPORT=ROOT/'reports/realworld-2026-11-12';RUN=REPORT/'conversion';OUT=ROOT/'target/realworld/assistant-review'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 OUT.mkdir(parents=True,exist_ok=True);private=OUT/'private-label-map.json'
 if private.exists():raise SystemExit('Packets already exist; preserve their identity mapping')
 seed=secrets.token_hex(32);corpus=json.loads((REPORT/'corpus-manifest.json').read_text());refs={r['id']:r for r in json.loads((REPORT/'reference-manifest.json').read_text())};mapping=[];index=[]
 engines=['bulkparse','anydoc','docling','markitdown','pandoc','mammoth','libreoffice','unstructured']
 runs={}
 for e in engines:
  p=RUN/f'{e}-round-1.json'
  if not p.exists():raise SystemExit('Wait for the complete first measured pass: '+e)
  runs[e]={r['id']:r for r in json.loads(p.read_text())['documents']}
 for r in corpus:
  doc=r['id'];folder=OUT/'packets'/doc;folder.mkdir(parents=True,exist_ok=True)
  supported=[e for e in engines if doc in runs[e]];order=sorted(supported,key=lambda e:hashlib.sha256((seed+'|'+doc+'|'+e).encode()).hexdigest());outputs=[]
  for i,e in enumerate(order):
   label=chr(65+i);src=RUN/'prediction'/e/'round-1'/(doc+'.md');dest=folder/(label+'.md');assert sha(src)==runs[e][doc]['output_sha256'];os.link(src,dest)
   outputs.append({'label':label,'path':str(dest.relative_to(ROOT)),'sha256':sha(dest),'bytes':dest.stat().st_size})
   mapping.append({'document_id':doc,'label':label,'engine':e,'output_sha256':sha(dest),'round':1,'ok':runs[e][doc]['ok']})
  packet={'id':doc,'format':r['format'],'source_title':r['title'],'reference':refs[doc],'outputs':outputs,'method':'Same-chat assistant assessment. Tool labels concealed during inspection; reviewer retains project context. No claim of independent blind scoring.'}
  (folder/'packet.json').write_text(json.dumps(packet,indent=2,ensure_ascii=False)+'\n');index.append({'id':doc,'format':r['format'],'packet':str((folder/'packet.json').relative_to(ROOT)),'candidates':len(outputs)})
 private.write_text(json.dumps({'seed':seed,'mapping':mapping},indent=2)+'\n');(OUT/'index.json').write_text(json.dumps(index,indent=2)+'\n')
 print(json.dumps({'documents':len(index),'candidate_assessments':len(mapping),'outputs_are_read_only_hardlinks':'Do not edit candidate Markdown files; they share the immutable benchmark outputs.'}))
if __name__=='__main__':main()
