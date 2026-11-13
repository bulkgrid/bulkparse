#!/usr/bin/env python3
"""Historical PDF asset audit; requires the pre-parity image-decoding build, not current PDF text output."""
import argparse,base64,io,json,re,collections,hashlib,importlib.metadata
from pathlib import Path
import pymupdf
from PIL import Image,ImageCms,ImageChops,ImageStat
import bulkparse

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);args=ap.parse_args();args.out.mkdir(parents=True,exist_ok=True)
 rows=[];counts=collections.Counter();inputs={}
 for path in sorted(Path('target/opendataloader-bench/pdfs').glob('*.pdf')):
  inputs[path.name]=hashlib.sha256(path.read_bytes()).hexdigest()
  parsed=bulkparse.parse_file(path);doc=pymupdf.open(path)
  for asset in parsed['document']['assets']:
   match=re.fullmatch(r'PDF/object-(\d+)-(\d+)',asset['source'].get('part',''))
   if not match:continue
   xref=int(match[1]);data=base64.b64decode(asset['data']);image=Image.open(io.BytesIO(data));image.load()
   if image.info.get('icc_profile'):
    try:
     profile=ImageCms.ImageCmsProfile(io.BytesIO(image.info['icc_profile']))
     alpha=image.convert('RGBA').getchannel('A')
     image=ImageCms.profileToProfile(image.convert('L') if image.mode=='LA' else image,profile,ImageCms.createProfile('sRGB'),outputMode='RGB').convert('RGBA')
     image.putalpha(alpha)
    except (ValueError,ImageCms.PyCMSError):
     counts['icc_conversion_failed']+=1;image=image.convert('RGBA')
   else:image=image.convert('RGBA')
   pix=pymupdf.Pixmap(doc,xref)
   if pix.colorspace is None:
    counts['stencil_requires_paint_color']+=1;continue
   if pix.colorspace.n not in (1,3):pix=pymupdf.Pixmap(pymupdf.csRGB,pix)
   mask_type,mask=doc.xref_get_key(xref,'SMask')
   if mask_type=='xref':
    if pix.alpha:pix=pymupdf.Pixmap(pix,0)
    alpha=pymupdf.Pixmap(doc,int(mask.split()[0]));pix=pymupdf.Pixmap(pix,alpha)
   reference=Image.open(io.BytesIO(pix.tobytes('png'))).convert('RGBA')
   if image.size!=reference.size:raise AssertionError((path,xref,image.size,reference.size))
   # Compare composited pixels; invisible RGB is undefined when alpha is zero.
   a=Image.new('RGBA',image.size,'white');a.alpha_composite(image)
   b=Image.new('RGBA',reference.size,'white');b.alpha_composite(reference)
   diff=ImageChops.difference(a,b);mean=max(ImageStat.Stat(diff).mean[:3]);maximum=max(v[1] for v in diff.getextrema()[:3]);alpha_diff=ImageChops.difference(image.getchannel('A'),reference.getchannel('A')).getextrema()[1]
   row=dict(document=path.stem,xref=xref,media_type=asset['media_type'],size=image.size,mean_absolute_channel_error=mean,max_channel_error=maximum,max_alpha_error=alpha_diff)
   rows.append(row);counts['compared']+=1;counts['exact' if maximum==0 and alpha_diff==0 else 'different']+=1
   if mean>2 or alpha_diff>2:
    counts['review']+=1
    if counts['review']<=6:
     image.save(args.out/f'{path.stem}-{xref}-actual.png');reference.save(args.out/f'{path.stem}-{xref}-reference.png')
 if not rows:raise RuntimeError('This historical audit requires a build with PDF image export; current BulkParse intentionally omits it.')
 (args.out/'pixels.json').write_text(json.dumps(dict(counts=counts,documents=rows,input_sha256=inputs,versions={n:importlib.metadata.version(n) for n in ['PyMuPDF','Pillow']},harness_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),native_sha256=hashlib.sha256(Path(bulkparse.__file__).with_name('_native.abi3.so').read_bytes()).hexdigest(),method='Decoded assets compared with PyMuPDF xref images, soft masks composed, ICC transformed to sRGB by Pillow, compared over white; not a full-page visual score.'),indent=2)+'\n')
 print(json.dumps(counts,indent=2));print('worst',sorted(rows,key=lambda r:r['mean_absolute_channel_error'],reverse=True)[:5])
if __name__=='__main__':main()
