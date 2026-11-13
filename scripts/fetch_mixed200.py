#!/usr/bin/env python3
"""Fetch exact frozen public inputs. Fail on changed bytes; never substitute files."""
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
for d in json.loads((ROOT/'reports/mixed-200-2026-11-13/corpus-manifest.json').read_text()):
 p=ROOT/d['path'];p.parent.mkdir(parents=True,exist_ok=True)
 if p.exists():
  assert hashlib.sha256(p.read_bytes()).hexdigest()==d['sha256'],f'Changed existing input: {p}'
  continue
 tmp=p.with_suffix('.download')
 try:
  subprocess.run(['curl','-sS','-L','--fail','--max-time','90','--max-filesize','15728640',d['url'],'-o',str(tmp)],check=True)
  assert hashlib.sha256(tmp.read_bytes()).hexdigest()==d['sha256'],f'Source changed: {d["url"]}'
  tmp.rename(p)
 finally:tmp.unlink(missing_ok=True)
 print(d['id'],flush=True)
print('All 200 input hashes verified')
