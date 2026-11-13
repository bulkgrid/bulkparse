#!/usr/bin/env python3
"""Package measured comparison artifacts; omit downloaded PDFs/models/binaries."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile
ROOT=Path(__file__).resolve().parents[1]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('report',type=Path);a=ap.parse_args()
 report=a.report.resolve();dest=report.with_suffix('.zip');paths=set()
 for directory in [report,ROOT/'benchmarks/same-machine']:
  paths.update(p for p in directory.rglob('*') if p.is_file() and '__pycache__' not in p.parts and p.suffix not in ['.pyc','.pdf'])
 # Original tiny project-authored fixture PDFs are part of the source manifest.
 paths.update((report/'source-snapshot').rglob('*.pdf'))
 paths.add(Path(__file__).resolve())
 paths.discard(report/'bundle-manifest.json')
 manifest={str(p.relative_to(ROOT)):sha(p) for p in sorted(paths)}
 (report/'bundle-manifest.json').write_text(json.dumps({'files':manifest,'downloaded_pdfs_models_binaries_included':False},indent=2)+'\n')
 paths.add(report/'bundle-manifest.json')
 with zipfile.ZipFile(dest,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as z:
  for p in sorted(paths):z.write(p,str(p.relative_to(ROOT)))
 with zipfile.ZipFile(dest) as z:
  assert z.testzip() is None
  for name,digest in manifest.items():assert hashlib.sha256(z.read(name)).hexdigest()==digest
 print(json.dumps({'archive':str(dest),'bytes':dest.stat().st_size,'files':len(paths),'sha256':sha(dest),'hashes_verified':True},indent=2))
 dest.with_suffix('.zip.sha256').write_text(sha(dest)+'  '+dest.name+'\n')
if __name__=='__main__':main()
