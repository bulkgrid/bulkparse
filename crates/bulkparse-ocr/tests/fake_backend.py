"""Fault injection only; not shipped or used as an OCR engine."""
import argparse
import json
import os
from pathlib import Path
import sys
import time

p=argparse.ArgumentParser()
p.add_argument('--mode', required=True)
p.add_argument('--page', type=int, required=True)
p.add_argument('--models', type=Path, required=True)
a,_=p.parse_known_args()
a.models.with_suffix('.started').write_text(str(os.getpid()))
if a.mode=='hang': time.sleep(60)
if a.mode=='crash': sys.exit(7)
if a.mode in {'regions', 'bad-regions', 'ambiguous-regions'}:
    response={'version':3,'page':a.page,'image_size':[600,800],
              'regions':[{'order':0,'label':'formula','bbox':[10,20,300,100],
                          'content':{'type':'formula','tex':'E=mc^2'}}]}
    if a.mode=='bad-regions': response['regions'][0]['bbox']=[-1,20,300,100]
    if a.mode=='ambiguous-regions': response['text']='ambiguous'
    print(json.dumps(response))
elif a.mode=='oversize':
    sys.stdout.write('x'*4000000);sys.stdout.flush();time.sleep(60)
elif a.mode in {'geometry','bad-geometry','mixed-envelope','wrong-version','bad-confidence'}:
    reply={'version':2,'page':a.page,'image_size':[600,800],
           'lines':[{'text':'Åtta lådor','quad':[[111.78385925292969,20],[211.78385925292969,20],[211.78385925292969,40],[111.78385925292969,40]],'confidence':0.875}]}
    if a.mode=='bad-geometry': reply['lines'][0]['quad'][0][0]=-10
    if a.mode=='bad-confidence': reply['lines'][0]['confidence']=1.5
    if a.mode=='mixed-envelope': reply['text']='ambiguous'
    if a.mode=='wrong-version': reply['version']=4
    print(json.dumps(reply))
elif a.mode=='invalid': print('not json')
elif a.mode=='error': print(json.dumps({'version':1,'page':a.page,'error':{'code':'backend_error','message':'test failure'}}))
else: print(json.dumps({'version':1,'page':a.page+(a.mode=='wrong-page'),'text':'Åtta lådor'}))
