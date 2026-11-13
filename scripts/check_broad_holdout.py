#!/usr/bin/env python3
"""Small fresh W3C spot checks. No parser tuning, timing claims or gold accuracy score."""
import argparse,hashlib,json,os,re,shutil
from pathlib import Path
from markdown_it import MarkdownIt
from bs4 import BeautifulSoup
from benchmark_broad import ROOT,PDF,OFFICE,SUPPORT,commands
from benchmark_same_machine import Worker
from benchmark_opendataloader import write_json,sha256

HEADERS=['School','First grade','Second grade','Third grade','Fourth grade']
ROWS=[['Elm Street','23','24','26','22'],['Spruce Street','25','30','26','23'],['Locust Street','24','28','22','27'],['Maple Street','26','26','21','29']]
ANCHORS=['Application for this and that','List','Header One','Another list','Another Header One']

def normalized(s):return ' '.join(s.split())

def check(md,spec):
    soup=BeautifulSoup(MarkdownIt('commonmark',{'html':True}).enable('table').render(md),'html.parser')
    text=normalized(soup.get_text(' ',strip=True));result={}
    if spec['kind']=='columns':
        positions=[text.find(x) for x in ANCHORS]
        result['all_section_labels']=all(x>=0 for x in positions)
        result['section_sequence']=result['all_section_labels'] and positions==sorted(positions) and len(set(positions))==len(positions)
    else:
        expected=spec['rows'];tables=[]
        for table in soup.find_all('table'):
            tables.append([[normalized(c.get_text(' ',strip=True)) for c in row.find_all(['th','td'])] for row in table.find_all('tr')])
        result['table_structure']=bool(tables)
        result['header_cells']=any(HEADERS in table for table in tables)
        for i,row in enumerate(expected):result[f'data_row_{i+1}']=any(row in table for table in tables)
    return result

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args();out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
    rows=json.loads((ROOT/'target/broad/fresh-w3c/manifest.json').read_text())
    for r in rows:
        assert sha256(ROOT/r['path'])==r['sha256'];assert not r['prior_manifest_matches']
        r['kind']='columns' if 'reading-order' in r['path'] else 'table'
        if r['kind']=='table':
            r['rows']=[list(x) for x in ROWS]
            if r['format']=='odt':r['rows'][1][1]='30'
    # Freeze the checks and input identities before invoking any engine.
    write_json(out/'spec-before-conversion.json',rows)
    write_json(out/'harness-hashes.json',{str(Path(__file__).relative_to(ROOT)):sha256(Path(__file__)), 'scripts/worker_broad.py':sha256(ROOT/'scripts/worker_broad.py')})
    env=dict(os.environ,RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1',HF_HUB_OFFLINE='1',TRANSFORMERS_OFFLINE='1')
    results=[]
    for mode,engines in [('pdf',PDF),('office',OFFICE)]:
        cmds=commands(mode)
        for e in engines:
            chosen=[r for r in rows if (r['format']=='pdf')==(mode=='pdf') and (mode=='pdf' or e not in SUPPORT or r['format'] in SUPPORT[e])]
            if not chosen:continue
            w=Worker(cmds[e],out/f'{mode}-{e}.stderr',env)
            try:
                if e not in ('bulkparse','anydoc','pdf-inspector'):assert w.receive(timeout=240)['ready']
                folder=out/'predictions'/mode/e;folder.mkdir(parents=True)
                q={'documents':[str(ROOT/r['path']) for r in chosen]}
                if mode=='pdf':q['output_dir']=str(folder)
                reply=w.call(q);write_json(out/f'{mode}-{e}.json',reply)
                assert len(reply['documents'])==len(chosen)
                for spec,row in zip(chosen,reply['documents']):
                    p=folder/((Path(spec['path']).name if mode=='office' else Path(spec['path']).stem)+'.md')
                    if mode=='office':p.write_text(row['markdown'])
                    checks=check(p.read_text(),spec)
                    results.append(dict(mode=mode,engine=e,path=spec['path'],output_path=str(p.relative_to(out)),ok=row['ok'],checks=checks,passed=sum(checks.values()),total=len(checks),output_sha256=sha256(p)))
            finally:assert w.close()==0
            print(mode,e,[(x['passed'],x['total']) for x in results if x['mode']==mode and x['engine']==e],flush=True)
    write_json(out/'results.json',results)
    shutil.copy2(__file__,out/Path(__file__).name)

if __name__=='__main__':main()
