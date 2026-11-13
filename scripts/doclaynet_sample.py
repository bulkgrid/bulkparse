#!/usr/bin/env python3
"""Download bounded ranges from the public versioned DocLayNet ZIP archives.
Never fetch a whole archive. Retain archive identity and CRC-check each member.
"""
import argparse, hashlib, io, json, struct, subprocess, zipfile, zlib
from pathlib import Path
BASE='https://codait-cos-dax.s3.us.cloud-object-storage.appdomain.cloud/dax-doclaynet/1.0.0/'
class Archive(io.RawIOBase):
    def __init__(self, name, cache):
        self.url=BASE+name; self.cache=Path(cache); self.cache.mkdir(parents=True,exist_ok=True)
        raw=subprocess.check_output(['curl','-fsSI','--connect-timeout','10','--max-time','60',self.url],text=True)
        headers={k.lower():v.strip() for l in raw.splitlines() if ':' in l for k,v in [l.split(':',1)]}
        self.size=int(headers['content-length']); self.etag=headers['etag']
        self.position=0
        self.identity={'url':self.url,'size':self.size,'etag':self.etag}
    def seekable(self): return True
    def readable(self): return True
    def tell(self): return self.position
    def seek(self, offset, whence=0):
        self.position=offset if whence==0 else self.position+offset if whence==1 else self.size+offset
        return self.position
    def fetch(self,start,count):
        if count==0:return b''
        assert 0<=start<self.size and 0<count<=64*1024*1024
        path=self.cache/f'{start}-{count}.bin'
        if path.exists():
            data=path.read_bytes();assert len(data)==count;return data
        header=path.with_suffix('.headers'); partial=path.with_suffix('.part')
        subprocess.run(['curl','-fsSL','--connect-timeout','10','--max-time','90','--retry','2',
            '--max-filesize','67108864','-H',f'Range: bytes={start}-{start+count-1}',
            '-H','If-Match: '+self.etag,'-D',str(header),'-o',str(partial),self.url],check=True)
        headers={k.lower():v.strip() for l in header.read_text().splitlines() if ':' in l for k,v in [l.split(':',1)]}
        assert headers.get('content-range','').startswith(f'bytes {start}-{start+count-1}/'), headers
        data=partial.read_bytes()
        partial.unlink()
        assert len(data)==count
        path.write_bytes(data)
        return data
    def read(self,count=-1):
        if count<0:count=self.size-self.position
        count=min(count,self.size-self.position)
        data=self.fetch(self.position,count);self.position+=len(data);return data
    def members(self):
        with zipfile.ZipFile(self) as z:
            return {i.filename:{'offset':i.header_offset,'compressed':i.compress_size,'size':i.file_size,'method':i.compress_type,'crc':i.CRC} for i in z.infolist()}
    def extract(self,info):
        header=self.fetch(info['offset'],30)
        assert header[:4]==b'PK\x03\x04'
        fn,extra=struct.unpack_from('<HH',header,26)
        data=self.fetch(info['offset']+30+fn+extra,info['compressed'])
        data=zlib.decompress(data,-15) if info['method']==8 else data
        assert info['method'] in (0,8)
        assert len(data)==info['size'] and zlib.crc32(data)==info['crc']
        return data

def main():
    p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);a=p.parse_args()
    a.out.mkdir(parents=True,exist_ok=True)
    metadata={}
    for kind in ['core','extra']:
        arc=Archive('DocLayNet_'+kind+'.zip',a.out/('range-cache-'+kind))
        members=arc.members();metadata[kind]={'archive':arc.identity,'members':members}
        (a.out/(kind+'-index.json')).write_text(json.dumps(metadata[kind],indent=2)+'\n')
        if kind=='core':
            names=[n for n in members if n.endswith('test.json') or n.lower().endswith(('license','license.txt','readme.md'))]
            print('metadata members',names,flush=True)
            for n in names:
                data=arc.extract(members[n]);(a.out/Path(n).name).write_bytes(data)
    print(json.dumps({k:{'archive':v['archive'],'member_count':len(v['members'])} for k,v in metadata.items()},indent=2))
if __name__=='__main__':main()
