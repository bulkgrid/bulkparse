#!/usr/bin/env python3
"""Fresh-process per-document RSS, including parser, Markdown and worker serialization."""
import argparse,hashlib,json,os,re,subprocess,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--binary',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);ap.add_argument('--ids',nargs='*');ap.add_argument('--modes',nargs='+',default=['default','bounded']);ap.add_argument('--repeats',type=int,default=1);a=ap.parse_args();a.out.mkdir(parents=True,exist_ok=False)
 corpus=json.loads((ROOT/'reports/realworld-2026-11-12/corpus-manifest.json').read_text());corpus=[d for d in corpus if not a.ids or d['id'] in a.ids];binary=a.binary.resolve();identity={'method':__doc__,'binary':str(binary),'binary_sha256':sha(binary),'modes':a.modes,'repeats_per_process':a.repeats,'documents':len(corpus)};(a.out/'identity.json').write_text(json.dumps(identity,indent=2)+'\n');rows=[]
 env=dict(os.environ,RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1')
 for mode in a.modes:
  for d in corpus:
   path=ROOT/d['path'];assert sha(path)==d['sha256'];request=(json.dumps({'documents':[str(path)]})+'\n')*a.repeats
   cmd=['/usr/bin/time','-l',str(binary)]+(['--partial-on-limit'] if mode in ('bounded','stream_bounded') else [])+(['--stream-markdown'] if mode in ('stream','stream_bounded') else [])
   result=subprocess.run(cmd,input=request,text=True,capture_output=True,env=env,timeout=120)
   (a.out/(mode+'-'+d['id']+'.log')).write_text(result.stderr)
   outputs=[json.loads(line)['documents'][0] for line in result.stdout.splitlines()];assert len(outputs)==a.repeats
   row=outputs[-1];md=row.pop('markdown');rss=re.search(r'(\d+)\s+maximum resident set size',result.stderr);foot=re.search(r'(\d+)\s+peak memory footprint',result.stderr)
   r={'id':d['id'],'format':d['format'],'title':d['title'],'mode':mode,'input_bytes':path.stat().st_size,'output_bytes':len(md.encode()),'ok':row['ok'],'status':row.get('status'),'error':row.get('error'),'output_sha256':hashlib.sha256(md.encode()).hexdigest(),'peak_rss_bytes':int(rss[1]) if rss else None,'peak_footprint_bytes':int(foot[1]) if foot else None,'exit_code':result.returncode}
   rows.append(r)
   with (a.out/'documents.jsonl').open('a') as f:f.write(json.dumps(r)+'\n')
   print(mode,d['id'],r['peak_rss_bytes'],flush=True)
 assert sha(binary)==identity['binary_sha256'];(a.out/'summary.json').write_text(json.dumps(rows,indent=2)+'\n')
 for mode in a.modes:
  print(mode,'TOP',json.dumps(sorted([r for r in rows if r['mode']==mode],key=lambda r:r['peak_rss_bytes'] or 0,reverse=True)[:8],indent=2))
if __name__=='__main__':main()
