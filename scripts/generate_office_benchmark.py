#!/usr/bin/env python3
"""Generate small BulkParse-owned semantic checks; no upstream converter used."""
from pathlib import Path
import hashlib, json, struct, zipfile
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'fixtures/office-authored'
P16=lambda n:struct.pack('<H',n)
P32=lambda n:struct.pack('<I',n)
def package(name, parts):
    path=OUT/name
    with zipfile.ZipFile(path,'w',zipfile.ZIP_DEFLATED) as z:
        for p,s in parts.items():
            info=zipfile.ZipInfo(p,(2026,1,1,0,0,0));info.compress_type=zipfile.ZIP_DEFLATED
            z.writestr(info,s)
    return path

def compound(name, streams):
    # Minimal CFB v3 writer for test inputs, with ordinary (not mini) streams.
    FREE=0xffffffff; END=0xfffffffe
    streams=sorted(streams.items(),key=lambda x:(len(x[0]),x[0].upper()))
    header=bytearray(512);header[:8]=bytes.fromhex('d0cf11e0a1b11ae1');struct.pack_into('<HHHHH',header,24,0x3e,3,0xfffe,9,6)
    struct.pack_into('<IIIIIIIII',header,40,0,1,1,0,4096,END,0,END,0)
    for i in range(109):struct.pack_into('<I',header,76+i*4,0 if i==0 else FREE)
    fat=[FREE]*128;fat[0]=0xfffffffd;fat[1]=END;directory=bytearray(512);data=bytearray();sector=2
    def entry(i,title,kind,start,size,left=FREE,right=FREE,child=FREE):
        off=i*128;v=(title+'\0').encode('utf-16le');directory[off:off+len(v)]=v
        struct.pack_into('<HBBIII',directory,off+64,len(v),kind,1,left,right,child)
        struct.pack_into('<IQ',directory,off+116,start,size)
    entry(0,'Root Entry',5,END,0,child=1)
    for i,(title,b) in enumerate(streams,1):
        size=max(4096,(len(b)+511)//512*512);count=size//512
        entry(i,title,2,sector,size,right=i+1 if i<len(streams) else FREE)
        for j in range(count):fat[sector+j]=sector+j+1 if j+1<count else END
        sector+=count;data.extend(b);data.extend(bytes(size-len(b)))
    path=OUT/name;path.write_bytes(header+struct.pack('<128I',*fat)+directory+data);return path

def main():
    OUT.mkdir(parents=True,exist_ok=True);cases=[]
    def add(path,expected,excluded=()):cases.append({'path':str(path.relative_to(ROOT)),'format':path.suffix[1:],'track':'authored','sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'required_text':list(expected),'forbidden_text':list(excluded)})
    relns='http://schemas.openxmlformats.org/package/2006/relationships';rns='http://schemas.openxmlformats.org/officeDocument/2006/relationships'
    sheetns='http://schemas.openxmlformats.org/spreadsheetml/2006/main'
    add(package('ledger.xlsx',{'[Content_Types].xml':f'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>',
      '_rels/.rels':f'<Relationships xmlns="{relns}"><Relationship Id="main" Type="{rns}/officeDocument" Target="xl/workbook.xml"/></Relationships>',
      'xl/workbook.xml':f'<workbook xmlns="{sheetns}" xmlns:r="{rns}"><sheets><sheet name="Ledger" sheetId="1" r:id="s"/></sheets></workbook>',
      'xl/_rels/workbook.xml.rels':f'<Relationships xmlns="{relns}"><Relationship Id="s" Type="{rns}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>',
      'xl/worksheets/sheet1.xml':f'<worksheet xmlns="{sheetns}"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Sales</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>東京</t></is></c><c r="B2"><f>6*7</f><v>42</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells></worksheet>'}),['Sales','東京','42'])
    # Use an independent file writer for a complete, standards-conforming PPTX
    # package (including layout/master/theme relationships). No converter runs.
    from pptx import Presentation
    from io import BytesIO
    deck=Presentation();slide=deck.slides.add_slide(deck.slide_layouts[1])
    slide.shapes.title.text='Quarterly review';slide.placeholders[1].text='Revenue increased'
    buffer=BytesIO();deck.save(buffer);buffer.seek(0)
    with zipfile.ZipFile(buffer) as z:parts={name:z.read(name) for name in z.namelist()}
    add(package('review.pptx',parts),['Quarterly review','Revenue increased'])
    ns='xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"'
    for ext,kind,body,words in [('odt','text','<text:h text:outline-level="1">Project notes</text:h><text:p>Résumé français</text:p>',['Project notes','Résumé français']),('ods','spreadsheet','<table:table table:name="Inventory"><table:table-row><table:table-cell office:value-type="string"><text:p>Bolts</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="42"><text:p>42</text:p></table:table-cell></table:table-row></table:table>',['Bolts','42']),('odp','presentation','<draw:page draw:name="Overview"><draw:frame><draw:text-box><text:p>Launch schedule</text:p></draw:text-box></draw:frame></draw:page>',['Launch schedule'])]:
        add(package('document.'+ext,{'mimetype':'application/vnd.oasis.opendocument.'+kind,'content.xml':f'<office:document-content {ns} office:version="1.2"><office:body><office:{kind}>{body}</office:{kind}></office:body></office:document-content>'}),words)
    add(package('chapters.epub',{'mimetype':'application/epub+zip','META-INF/container.xml':'<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
      'OPS/book.opf':'<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata/><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="b" href="b.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="b"/><itemref idref="a"/></spine></package>',
      'OPS/a.xhtml':'<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Second chapter</p></body></html>', 'OPS/b.xhtml':'<!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml"><body><h1>First chapter</h1><p>Καλημέρα</p></body></html>'}),['First chapter','Second chapter','Καλημέρα'])
    path=OUT/'styled.rtf';path.write_bytes(br'{\rtf1\ansi Hello {\b world}\par \uc1\u945? end{\*\unknown Hidden payload}}');add(path,['Hello','world','α','end'],['Hidden payload'])
    text=b'Legacy report\rRevenue 42\r';word=bytearray(512);struct.pack_into('<H',word,0,0xa5ec);struct.pack_into('<H',word,2,0xc1);struct.pack_into('<I',word,0x18,512);struct.pack_into('<I',word,0x1c,512+len(text));struct.pack_into('<I',word,0x4c,len(text));word.extend(text);add(compound('report.doc',{'WordDocument':word}),['Legacy report','Revenue 42'])
    rec=lambda k,b:P16(k)+P16(len(b))+b
    bof=P16(0x600)+P16(5)+bytes(12);wb=bytearray(rec(0x809,bof));sheetpos=len(wb)+4;wb.extend(rec(0x85,bytes(6)+b'\x01\x00S'));wb.extend(rec(10,b''));struct.pack_into('<I',wb,sheetpos,len(wb));wb.extend(rec(0x809,P16(0x600)+P16(0x10)+bytes(12)));wb.extend(rec(0x204,bytes(6)+P16(6)+b'\x00Legacy'));wb.extend(rec(0x203,P16(1)+bytes(4)+struct.pack('<d',42)));wb.extend(rec(10,b''));add(compound('ledger.xls',{'Workbook':wb}),['Legacy','42'])
    wide=lambda s:P32(len(s.encode('utf-16le'))//2)+s.encode('utf-16le')
    def brt(k,b):
        h=bytes([k]) if k<128 else bytes([(k&127)|128,k>>7]);n=len(b);v=[]
        while n>127:v.append((n&127)|128);n>>=7
        return h+bytes(v+[n])+b
    add(package('ledger.xlsb',{'xl/workbook.bin':brt(156,bytes(8)+wide('s')+wide('Binary')), 'xl/_rels/workbook.bin.rels':f'<Relationships xmlns="{relns}"><Relationship Id="s" Type="{rns}/worksheet" Target="worksheets/s.bin"/></Relationships>', 'xl/worksheets/s.bin':brt(0,bytes(12))+brt(6,bytes(8)+wide('Binary values'))+brt(5,P32(1)+bytes(4)+struct.pack('<d',42))}),['Binary values','42'])
    pr=lambda k,b,v=0:P16(v)+P16(k)+P32(len(b))+b
    slide=pr(0x3ee,b'',15);sl=pr(0x3f3,P32(2)+bytes(12))+pr(0xf9f,P32(0))+pr(0xfa0,'Active slide'.encode('utf-16le'));doc=pr(0x3e8,pr(0xff0,sl,15),15);ppt=bytearray(8)+doc;so=len(ppt);ppt.extend(slide);directory=len(ppt);ppt.extend(pr(0x1772,P32((2<<20)|1)+P32(8)+P32(so)));edit=len(ppt);body=bytearray(28);struct.pack_into('<I',body,12,directory);struct.pack_into('<I',body,16,1);ppt.extend(pr(0xff5,body));user=bytearray(20);struct.pack_into('<I',user,16,edit);add(compound('review.ppt',{'Current User':user,'PowerPoint Document':ppt}),['Active slide'])
    (OUT/'manifest.json').write_text(json.dumps(cases,ensure_ascii=False,indent=2)+'\n')
    print(f'{len(cases)} authored cases generated')
if __name__=='__main__':main()
