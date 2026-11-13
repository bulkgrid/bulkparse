#!/usr/bin/env python3
"""Freeze already-authored OCR cases and identical 200-DPI engine inputs."""
from pathlib import Path
import json,hashlib
import pypdfium2 as pdfium
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'target/ocr-alternatives/data'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 # Provisioning is separate and explicit; rendering never downloads assets.
 OUT.mkdir(exist_ok=True);(OUT/'images').mkdir(exist_ok=False)
 cases=[]
 for c in json.loads((ROOT/'benchmarks/ocr-profiles/manifest.json').read_text()):
  if c['track']=='external_smoke':continue
  assert sha(ROOT/c['path'])==c['sha256'];assert sha(ROOT/c['reference'])==c['reference_sha256']
  doc=pdfium.PdfDocument(str(ROOT/c['path']));images=[]
  for i in range(len(doc)):
   page=doc[i];w,h=page.get_size();scale=200/72
   assert w*h*scale*scale<=8_000_000
   bitmap=page.render(scale=scale,rev_byteorder=True,limit_image_cache=True)
   p=OUT/'images'/f"{c['id']}-{i+1}.png";im=bitmap.to_pil().convert('RGB');im.save(p)
   images.append({'path':str(p.relative_to(OUT)),'sha256':sha(p),'size':list(im.size)})
   bitmap.close();page.close()
  doc.close();cases.append({**c,'images':images,'reference_text':(ROOT/c['reference']).read_text()})
 (OUT/'cases.json').write_text(json.dumps(cases,indent=2,ensure_ascii=False)+'\n')
 print('cases',len(cases),'pages',sum(len(c['images']) for c in cases))
if __name__=='__main__':main()
