#!/usr/bin/env python3
"""Freeze an independent cell-structure test before producing predictions."""
from pathlib import Path
import datetime, hashlib, json, subprocess, xml.etree.ElementTree as ET
from audit_doclaynet_overlap import shingles
ROOT=Path(__file__).resolve().parents[1]
DATA=ROOT/'target/inspector-generalization'
SOURCE=DATA/'icdar/ICDAR-2013-Table-Competition-Corrected'
SPEC=ROOT/'benchmarks/icdar2013-tables'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,v): p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(v,indent=2,ensure_ascii=False)+'\n')
def regions(path):
    out=[]
    for table in ET.parse(path).getroot().findall('table'):
        for region in table.findall('region'):
            cells=[]; occupied=set()
            for cell in region.findall('cell'):
                a=cell.attrib;r=int(a['start-row']);c=int(a['start-col']);re=int(a.get('end-row',r));ce=int(a.get('end-col',c))
                if min(r,c)<0 or re<r or ce<c: raise ValueError('invalid cell range')
                points={(i,j) for i in range(r,re+1) for j in range(c,ce+1)}
                if points & occupied: raise ValueError('overlapping cell ranges')
                occupied |= points
                cells.append({'row':r,'col':c,'rowspan':re-r+1,'colspan':ce-c+1,'text':''.join(cell.find('content').itertext()) if cell.find('content') is not None else ''})
            if not cells: raise ValueError('empty annotated region')
            r0=min(c['row'] for c in cells);c0=min(c['col'] for c in cells)
            for c in cells:c['row']-=r0;c['col']-=c0
            out.append({'page':int(region.get('page')),'table_id':table.get('id'),'region_id':region.get('id'),'cells':cells})
    return out

def main():
    if SPEC.exists(): raise RuntimeError('Already frozen; use a new spec instead of overwriting')
    archive=DATA/'icdar2013-corrected.tar.gz'
    assert sha(archive)=='fbea3d39d186256c56cca5640c197af458d06e1f73210adb78526b92068c193a'
    prior={}
    paths=subprocess.check_output(['rg','--files','--hidden','--no-ignore','-g','*.pdf'],cwd=ROOT,text=True).splitlines()
    for s in paths:
        if s.startswith('target/inspector-generalization/'):continue
        p=ROOT/s
        if p.is_file():prior.setdefault(sha(p),s)
    old=[]
    for digest,path in sorted(prior.items()):
        dest=ROOT/'target/independent-pdf-v2/prior-text'/(digest+'.txt')
        entry={'sha256':digest,'path':path}
        if not dest.exists():
            dest=DATA/'prior-text'/(digest+'.txt');dest.parent.mkdir(exist_ok=True)
            try:
                run=subprocess.run([str(ROOT/'target/holdout-pdfkit'),'extract',str(ROOT/path),str(dest)],capture_output=True,timeout=20)
                if run.returncode:entry['error']='PDFKit exit '+str(run.returncode)
            except subprocess.TimeoutExpired:entry['error']='PDFKit timeout'
        if dest.exists():entry.update(text_sha256=sha(dest),shingles=shingles(dest.read_text()))
        old.append(entry)
    # Human supplied native cells supplement PDFKit for the observed DocLayNet pages.
    m=json.loads((ROOT/'benchmarks/independent-pdf-v2/manifest.json').read_text())
    for row in m['rows']:
        p=ROOT/row['paths']['cells'];d=json.loads(p.read_text())
        old.append({'path':row['paths']['cells'],'sha256':sha(p),'text_sha256':sha(p),'shingles':shingles(' '.join(c['text'] for c in d['cells']))})
    rows=[];refs={}
    for p in sorted(SOURCE.glob('competition-*/*.pdf')):
        xml=p.with_name(p.stem+'-str.xml');digest=sha(p)
        row={'id':p.stem,'pdf':str(p.relative_to(ROOT)),'pdf_sha256':digest,'xml':str(xml.relative_to(ROOT)),'xml_sha256':sha(xml),'excluded':False,'reasons':[]}
        try: refs[p.stem]=regions(xml)
        except ValueError as e:row['reasons'].append(str(e))
        if digest in prior:row['reasons'].append('identical prior PDF: '+prior[digest])
        dest=DATA/'icdar-native'/(p.stem+'.txt');dest.parent.mkdir(exist_ok=True)
        try:
            run=subprocess.run([str(ROOT/'target/holdout-pdfkit'),'extract',str(p),str(dest)],capture_output=True,timeout=30)
            if run.returncode:raise ValueError('PDFKit exit '+str(run.returncode))
            row['native_text_sha256']=sha(dest);s=shingles(dest.read_text());matches=[]
            for r in old:
                if 'shingles' not in r:continue
                shared=len(s & r['shingles']);denom=min(len(s),len(r['shingles']))
                if shared>=50 and denom and shared/denom>=.8:matches.append({'path':r['path'],'sha256':r['sha256'],'shared':shared,'containment':shared/denom})
            row['probable_overlap']=matches
            if matches:row['reasons'].append('probable prior text overlap')
        except (ValueError,subprocess.TimeoutExpired) as e: row['reasons'].append('overlap check incomplete: '+str(e))
        row['excluded']=bool(row['reasons']);rows.append(row)
    for r in old:r.pop('shingles',None)
    write(SPEC/'manifest.json',{'archive_sha256':sha(archive),'source_url':'https://huggingface.co/datasets/bsmock/ICDAR-2013-Table-Competition-Corrected/resolve/bdb9585/ICDAR-2013-Table-Competition-Corrected.tar.gz','rows':rows})
    write(SPEC/'references.json',refs);write(SPEC/'prior-inventory.json',old)
    (SPEC/'PROTOCOL.md').write_text('''# Independent table structure test\n\nFrozen before any BulkParse predictions. Use all 67 competition PDFs, no training PDFs. Exact PDF-stem XML only; four unmatched b-variant XML alternatives are excluded. Regions are page-local table units; row/column offsets are normalized to each region origin. Inclusive end indices become spans. Empty grid holes become empty cells. Invalid overlapping cell annotations exclude the document before predictions.\n\nExclude exact prior PDF hashes or 5-token normalized text containment >=0.8 with >=50 shared shingles against all prior local PDFs and observed DocLayNet native cells. PDFKit performs independent text extraction. No replacements. Failed new overlap checks exclude the document; old extraction failures are reported as an audit limitation. Source-family overlap cannot be completely excluded.\n\nParse full PDFs with no gold crops or labels, local native engine, OCR off. Before is the saved heading-fields release; after is frozen before this test is run. No tuning on test outcomes. Score all annotated pages; predictions on unannotated pages are reported separately because completeness of annotations outside selected table pages is unknown.\n\nMatch predicted and gold table regions within each page by maximum-weight one-to-one assignment on NFKC/casefold token-multiset F1, minimum 0.10. Matching uses text only, never structural similarity. Unmatched tables score zero. Per-page score is sum of matched upstream TEDS (or TEDS-S) divided by max(gold count,predicted count). Primary aggregate weights pages by this denominator. Also report macro page scores, matched-only scores, detection precision/recall, and all per-table pairs. These custom end-to-end aggregates are not comparable numerically with opendataloader overall/TEDS.\n\nUse unchanged pinned opendataloader TEDSEvaluator on canonical HTML, all cells td, escaped content, explicit spans. Do not change cell content or manually repair predictions. Conversion errors/timeouts score zero. Timing uses the saved sequential single-process workers: one warm-up full corpus followed by five full passes, alternating before/after. Report median, all runs, hashes, and hardware; exclude loading and evaluator time consistently with the main study.\n\nCorrected annotations are published under CDLA-Permissive-2.0 according to the source card; the card says original PDF licensing is unspecified, believed public domain by the original creator. Do not redistribute PDFs in result bundles.\n''')
    write(SPEC/'freeze.json',{'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'predictions_inspected':False,'documents':len(rows),'eligible':sum(not r['excluded'] for r in rows),'files':{p.name:sha(p) for p in SPEC.iterdir() if p.is_file()},'script_sha256':sha(Path(__file__))})
    print(json.dumps({'eligible':sum(not r['excluded'] for r in rows),'excluded':[{'id':r['id'],'reasons':r['reasons']} for r in rows if r['excluded']],'old_extraction_failures':[r for r in old if 'error' in r]},indent=2))
if __name__=='__main__':main()
