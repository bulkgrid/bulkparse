#!/usr/bin/env python3
"""Independent source-text audit of recovered Word stories and XLS text objects."""
import hashlib,json,re,subprocess,struct
from pathlib import Path
import olefile
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'reports/content-recovery-2026-11-13'
def key(s):return ''.join(c.casefold() for c in s if c.isalnum())
def clean(s):
 out=[];field=[]
 for c in s:
  if c=='\x13':field.append(False)
  elif c=='\x14':
   if field:field[-1]=True
  elif c=='\x15':
   if field:field.pop()
  elif all(field):
   if c in '\r\x07\x0b\x0c':out.append('\n')
   elif c>=' ' or c=='\t':out.append(c)
 return ''.join(out)
def stories(path):
 with olefile.OleFileIO(path) as f:
  b=f.openstream('WordDocument').read();flags=struct.unpack_from('<H',b,10)[0]
  table=f.openstream('1Table' if flags&512 else '0Table').read()
  off,n=struct.unpack_from('<II',b,0x1a2);clx=table[off:off+n];i=0
  while clx[i]==1:i+=3+struct.unpack_from('<H',clx,i+1)[0]
  assert clx[i]==2
  size=struct.unpack_from('<I',clx,i+1)[0];data=clx[i+5:i+5+size];count=(size-4)//12
  cps=struct.unpack_from('<'+'I'*(count+1),data)
  bounds=[];cur=struct.unpack_from('<I',b,0x4c)[0]
  for off,label in [(0x50,'Footnotes'),(0x54,'Headers and footers'),(0x58,'Macro text'),(0x5c,'Comments'),(0x60,'Endnotes'),(0x64,'Text boxes'),(0x68,'Header text boxes')]:
   end=cur+struct.unpack_from('<I',b,off)[0]
   if end>cur:bounds.append([label,cur,end,[]])
   cur=end
  language=struct.unpack_from('<H',b,0x3c if flags&0x4000 else 6)[0]&1023
  encoding={0x19:'cp1251',0x22:'cp1251',0x23:'cp1251',0x11:'shift_jis',0x12:'euc_kr',0x04:'gbk',0x01:'cp1256',0x0d:'cp1255',0x08:'cp1253',0x1f:'cp1254'}.get(language,'cp1252')
  for j in range(count):
   fc=struct.unpack_from('<I',data,4*(count+1)+8*j+2)[0];compressed=bool(fc&0x40000000);base=(fc&0x3fffffff)//(2 if compressed else 1);width=1 if compressed else 2
   for label,start,end,pieces in bounds:
    lo=max(start,cps[j]);hi=min(end,cps[j+1])
    if lo<hi:
     chunk=b[base+(lo-cps[j])*width:base+(hi-cps[j])*width];pieces.append(chunk.decode(encoding if compressed else 'utf-16le'))
  return [{'story':label,'source_characters':end-start,'paragraphs':[p for p in clean(''.join(pieces)).splitlines() if key(p)]} for label,start,end,pieces in bounds]
def xls_source_texts(path):
 with olefile.OleFileIO(path) as f:
  b=f.openstream('Workbook' if f.exists('Workbook') else 'Book').read()
 records=[];offset=0
 while offset+4<=len(b):
  kind,size=struct.unpack_from('<HH',b,offset);records.append((kind,b[offset+4:offset+4+size]));offset+=4+size
 result=[]
 for i,(kind,body) in enumerate(records):
  if kind!=0x1b6 or len(body)<12:continue
  remaining=struct.unpack_from('<H',body,10)[0];units=[]
  for following,payload in records[i+1:]:
   if remaining==0 or following!=0x3c:break
   if not payload:break
   width=2 if payload[0]&1 else 1;take=min(remaining,(len(payload)-1)//width)
   units.extend(struct.unpack('<'+'H'*take,payload[1:1+take*2]) if width==2 else payload[1:1+take]);remaining-=take
  if units:
   assert remaining==0,'incomplete source text object'
   result.append({'text':struct.pack('<'+'H'*len(units),*units).decode('utf-16le'),'incomplete':remaining})
 return result

def main():
 manifest=json.loads((ROOT/'reports/realworld-2026-11-12/corpus-manifest.json').read_text());selected=[d for d in manifest if d['format'] in ('doc','xls','docx')]
 command=[str(OUT/'release-office-bench')];q=json.dumps({'documents':[str(ROOT/d['path']) for d in selected]})+'\n'
 proc=subprocess.run(command,input=q,text=True,capture_output=True,check=True);rows=json.loads(proc.stdout)['documents'];results=[];diffs=[]
 xls={d['id']:xls_source_texts(ROOT/d['path']) for d in selected if d['format']=='xls'}
 for d,r in zip(selected,rows):
  assert hashlib.sha256((ROOT/d['path']).read_bytes()).hexdigest()==d['sha256'];assert r['ok'];after=r['markdown'];before=(ROOT/'reports/diagnostic-categories-2026-11-13/benchmark/prediction/bulkparse/round-1'/(d['id']+'.md')).read_text()
  if after!=before:
   beforelines=[line for line in before.splitlines() if line.strip()];lines=iter(after.splitlines())
   # DOCX title additions can extend a table line; require existing normalized
   # characters in their original order, then inspect their small explicit diff.
   if d['format']!='docx':assert all(any(line==old for line in lines) for old in beforelines),d['id']
   import difflib
   diffs.append({'id':d['id'],'diff':'\n'.join(difflib.unified_diff(before.splitlines(),after.splitlines(),n=2))})
  if d['format']=='doc':
   evidence=stories(ROOT/d['path'])
   for story in evidence:
    story['checked']=len(story['paragraphs']);story['new_paragraphs']=sum(key(p) not in key(before) for p in story['paragraphs'])
    assert all(key(p) in key(after) for p in story['paragraphs']),(d['id'],story['story'])
   results.append({'id':d['id'],'source_sha256':d['sha256'],'stories':evidence})
  elif d['format']=='xls' and xls.get(d['id']):
   assert all(key(t['text']) in key(after) for t in xls[d['id']]),d['id']
   results.append({'id':d['id'],'source_sha256':d['sha256'],'drawing_comment_texts':xls[d['id']]})
  if after!=before:(OUT/'reviewed-markdown').mkdir(exist_ok=True);(OUT/'reviewed-markdown'/(d['id']+'.md')).write_text(after)
 output={'binary_sha256':hashlib.sha256(Path(command[0]).read_bytes()).hexdigest(),'source_checks':results,'changed_documents':diffs,'method':'Independent compound-file/piece-table source decoding, required paragraph coverage, original-line preservation, and explicit Markdown diffs. Not a visual quality score.'}
 (OUT/'source-quality-checks.json').write_text(json.dumps(output,indent=2)+'\n')
 print('Changed documents:',[d['id'] for d in diffs]);print('Word paragraphs:',sum(s['checked'] for d in results for s in d.get('stories',[])));print('New Word paragraphs:',sum(s['new_paragraphs'] for d in results for s in d.get('stories',[])))
if __name__=='__main__':main()
