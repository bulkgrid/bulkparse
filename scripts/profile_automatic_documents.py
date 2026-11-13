#!/usr/bin/env python3
import argparse,json,os,re,subprocess,hashlib
from pathlib import Path
from benchmark_realworld import ROOT,REPORT,digest,write

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--binary',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);ap.add_argument('--old-profile',action='store_true');a=ap.parse_args();a.out.mkdir(parents=True,exist_ok=False)
 corpus=json.loads((REPORT/'corpus-manifest.json').read_text());ids=['xlsm-1c382d45c18005cd','ods-b3185d3043cb268a','csv-fbeb7b5e9b4f3ed9'];rows=[]
 for doc in [d for d in corpus if d['id'] in ids]:
  cmd=['/usr/bin/time','-l',str(a.binary.resolve()),'--verify-json']+(['--large-documents'] if a.old_profile else [])
  run=subprocess.run(cmd,input=json.dumps({'documents':[str(ROOT/doc['path'])]})+'\n',text=True,capture_output=True,env=dict(os.environ,RAYON_NUM_THREADS='1'),timeout=90)
  (a.out/(doc['id']+'.log')).write_text(run.stderr);r=json.loads(run.stdout)['documents'][0];md=r.pop('markdown');r.update(id=doc['id'],output_sha256=hashlib.sha256(md.encode()).hexdigest(),rss=int(re.findall(r'(\d+)\s+maximum resident set size',run.stderr)[-1]));rows.append(r);print(doc['id'],r['rss'],r['elapsed_ns']/1e9,flush=True)
 write(a.out/'results.json',dict(binary_sha256=digest(a.binary),old_profile=a.old_profile,rows=rows))
if __name__=='__main__':main()
