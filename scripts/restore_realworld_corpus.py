#!/usr/bin/env python3
"""Retrieve exactly the frozen public inputs; reject changed upstream content."""
import argparse,hashlib,json,subprocess,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--manifest',type=Path,default=ROOT/'reports/realworld-2026-11-12/corpus-manifest.json');a=ap.parse_args()
 corpus=json.loads(a.manifest.read_text());failures=[]
 for r in corpus:
  dest=(ROOT/r['path']).resolve()
  if not dest.is_relative_to(ROOT/'target/realworld/corpus'):raise ValueError('Manifest path is outside the corpus directory')
  if dest.exists():
   if digest(dest)!=r['sha256']:raise ValueError('Existing input has changed: '+r['id'])
   continue
  dest.parent.mkdir(parents=True,exist_ok=True)
  try:
   with tempfile.NamedTemporaryFile(dir=dest.parent,prefix='.download-',delete=True) as f:
    subprocess.run(['curl','--fail','--location','--silent','--show-error','--max-time','120','--max-filesize','15728640','--output',f.name,r['url']],check=True)
    tmp=Path(f.name)
    if digest(tmp)!=r['sha256']:raise ValueError('Upstream bytes differ from the frozen input')
    dest.write_bytes(tmp.read_bytes())
  except (ValueError,subprocess.CalledProcessError) as e:failures.append({'id':r['id'],'url':r['url'],'error':str(e)})
 print(json.dumps({'documents':len(corpus),'available':sum((ROOT/r['path']).exists() for r in corpus),'failures':failures},indent=2))
 if failures:raise SystemExit(1)
if __name__=='__main__':main()
