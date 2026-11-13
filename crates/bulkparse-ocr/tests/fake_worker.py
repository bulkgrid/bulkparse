"""Fault injection for supervised worker lifecycle tests; never OCR."""
import argparse,json,os,sys,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--models',type=Path,required=True);p.add_argument('--mode',default='ok')
a,_=p.parse_known_args()
a.models.with_suffix('.started').write_text(str(os.getpid()))
with a.models.with_suffix('.pids').open('a') as f:f.write(str(os.getpid())+'\n')
if a.mode=='ignore-input':time.sleep(60)
for line in sys.stdin:
 r=json.loads(line)
 with a.models.with_suffix('.inputs').open('a') as f:f.write(r['input']+'\n')
 fail=a.models.with_suffix('.failed')
 if a.mode=='crash-once' and not fail.exists():fail.touch();sys.exit(7)
 if a.mode=='hang':time.sleep(60)
 if a.mode=='flood':sys.stdout.write('x'*4000000);sys.stdout.flush();time.sleep(60)
 if a.mode=='bad-json':print('broken',flush=True);continue
 text=Path(r['input']).read_text()
 response={'version':1,'page':r['page'],'text':text}
 if a.mode=='regions':
  response={'version':3,'page':r['page'],'image_size':[600,800],'regions':[
   {'order':0,'label':'formula','bbox':[10,20,300,100],'content':{'type':'formula','tex':text}}]}
 if a.mode=='bad-page':response['page']+=1
 out={'id':r['id']+(a.mode=='bad-id'),'response':response}
 print(json.dumps(out),flush=True)
