#!/usr/bin/env python3
"""Fresh authored table cases, created after implementation freeze, not external gold.

--prepare writes expected structures before rendering with CoreText. Later runs
reuse the pinned original PDFs. No parser-specific text is present in the engine.
"""
from bulkparse_cli import parse_command
import argparse
import json
from pathlib import Path
import re
import subprocess
from benchmark_parsers import sha
from evaluate_fixtures import source_digest

ROOT=Path(__file__).resolve().parents[1]
BASE=ROOT/'benchmarks/pdf-tables'
FIXTURES=ROOT/'fixtures/pdf-tables'


def span(text,x,y,size=11):
    return dict(text=text,x=x,y=y,size=size,font='Helvetica')


def specs():
    out=[]
    def grid(rows,x=40,top=730,width=520,height=28,size=11):
        cols=len(rows[0]);spans=[]
        for r,row in enumerate(rows):
            for c,text in enumerate(row):
                for line,part in enumerate(text.split('\n')):
                    spans.append(span(part,x+8+c*width/cols,top-r*height-size-4-line*(size+2),size))
        return dict(spans=spans,rules=[[x,top-i*height,x+width,top-i*height] for i in range(len(rows)+1)])
    def add(name,page,expected):out.append(dict(id=name,pages=[page],expected_tables=expected))
    inventory=[['Material','Quantity','Batch','Price'],['Brass','47','R8','615'],['Steel','','T3','920'],['Glass','63','V6','130']]
    add('four-columns-empty-cell',grid(inventory),[inventory])
    numeric=[['11','22'],['33','44'],['55','66']]
    add('no-header-numeric',grid(numeric,width=300,size=14,height=34),[numeric])
    wrapped=[['Description','Amount'],['Packed\ncarefully','41'],['Sent\ntoday','82']]
    add('wrapped-cells',grid(wrapped,height=46),[wrapped])
    a=[['Fruit','Count'],['Peach','14'],['Plum','27']];b=[['Tool','Count'],['Drill','38'],['Saw','59']]
    p=grid(a,width=360);q=grid(b,top=420,width=360)
    add('two-separated-tables',dict(spans=p['spans']+q['spans'],rules=p['rules']+q['rules']),[a,b])
    p=grid(a,top=650,width=360)
    p['spans']=[span('Opening paragraph',40,740)]+p['spans']+[span('Closing paragraph',40,500)]
    add('surrounding-prose',p,[a])
    rows=[['Label','Value'],['One','101'],['Two','202']]
    p=grid(rows,x=170,top=520,width=280,height=22,size=9)
    add('offset-small-font',p,[rows])
    p=grid(rows);p['rules']=[p['rules'][0],p['rules'][-1]]
    add('border-only-ambiguous',p,[])
    p=grid([['A long sentence with several words.'],['Another ordinary sentence follows.'],['The final paragraph remains prose.']])
    add('ruled-single-column-prose',p,[])
    p=grid(rows);p['spans']=[s for s in p['spans'] if s['y']<700]
    p['spans'].append(span('A wide spanning label that crosses every column gutter in this region',48,715))
    add('spanning-cell-ambiguous',p,[])
    return out


def tables(result):
    def text(blocks):
        return '\n'.join(''.join(i.get('text','') for i in b.get('inlines',[])) for b in blocks)
    return [[[text(c['blocks']) for c in row] for row in b['rows']]
            for b in result['document']['blocks'] if b['type']=='table']


def compact(value):
    if isinstance(value,str):return ' '.join(value.split())
    return [compact(v) for v in value]


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--prepare',action='store_true')
    parser.add_argument('--freeze',type=Path,default=BASE/'freeze.json');args=parser.parse_args()
    freeze=json.loads(args.freeze.read_text())
    assert source_digest()==freeze['rust_source_sha256']
    assert sha(ROOT/'target/release/bulkparse')==freeze['binary_sha256']
    spec=BASE/'fresh-spec.json';manifest=BASE/'fresh-manifest.json'
    if args.prepare:
        if spec.exists() or manifest.exists():raise ValueError('fresh evaluation must not overwrite an observed version')
        spec.write_text(json.dumps(specs(),indent=2)+'\n')
        subprocess.run([str(ROOT/'target/holdout-pdfkit'),'generate',str(spec),str(FIXTURES)],check=True)
        manifest.write_text(json.dumps([{'id':s['id'],'sha256':sha(FIXTURES/(s['id']+'.pdf'))} for s in specs()],indent=2)+'\n')
    known={x['id']:x['sha256'] for x in json.loads(manifest.read_text())};results=[]
    artifacts=ROOT/'target/pdf-tables-fresh';artifacts.mkdir(exist_ok=True)
    for case in json.loads(spec.read_text()):
        path=FIXTURES/(case['id']+'.pdf');assert sha(path)==known[case['id']]
        run=subprocess.run([str(ROOT/'target/release/bulkparse'), parse_command(ROOT/'target/release/bulkparse'),str(path),'--to','json'],capture_output=True,text=True,timeout=15)
        (artifacts/(case['id']+'.json')).write_text(run.stdout)
        actual=tables(json.loads(run.stdout)) if run.returncode in (0,2) else None
        passed=actual is not None and compact(actual)==compact(case['expected_tables'])
        results.append(dict(id=case['id'],passed=passed,expected_tables=case['expected_tables'],actual_tables=actual,exit_code=run.returncode,source_sha256=known[case['id']]))
    report={'freeze':freeze,'generator_sha256':sha(ROOT/'scripts/holdout_pdfkit.swift'),'harness_sha256':sha(__file__),'cases':results}
    (BASE/'fresh-results.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2));
    if not all(x['passed'] for x in results):raise SystemExit(1)


if __name__=='__main__':main()
