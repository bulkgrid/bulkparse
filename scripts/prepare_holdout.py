#!/usr/bin/env python3
"""Write original expected text first, render via CoreText, then freeze manifest."""
import json
from pathlib import Path
import subprocess
from benchmark_parsers import sha

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT/'target/independent-inputs'


def span(text, x=48, y=730, size=12, font='Helvetica', **extra):
    return dict(text=text, x=x, y=y, size=size, font=font, **extra)


def specifications():
    docs = []
    def add(name, category, pages, lines, **extra):
        docs.append(dict(id=name, category=category, pages=pages,
                         expected='\n'.join(lines)+'\n', ordered_blocks=lines, **extra))
    prose = ['Harbor staff recorded the morning tide.', 'Three vessels arrived before noon.',
             'The eastern gate reopened after inspection.', 'Evening crews checked every berth.']
    add('prose', 'native', [dict(spans=[span(t,y=730-i*20) for i,t in enumerate(prose)])], prose)
    for cols, size, gap in [(2,11,270),(3,9,180)]:
        lines = [[f'{name} section records item {i+1}.' for i in range(4)]
                 for name in ['Cedar','Maple','Willow'][:cols]]
        # Paint row-major; expected reading order remains column-major.
        spans = [span(lines[c][r],x=36+c*gap,y=700-r*22,size=size)
                 for r in range(4) for c in range(cols)]
        add(f'columns-{cols}', 'columns', [dict(spans=spans)], sum(lines,[]))
    left=['North team repaired the pump.', 'Fresh water reached the village.']
    right=['South team tested the valves.', 'Pressure remained within limits.']
    title='Regional water maintenance report'
    add('spanning-title','columns',[dict(spans=[span(title,size=20)]+
        [span(t,x=x,y=675-i*20,size=11) for x,ls in [(40,left),(320,right)] for i,t in enumerate(ls)])],
        [title]+left+right)
    cells=[['Item','Units','Cost'],['Copper','17','420'],['Zinc','28','310'],['Nickel','39','650']]
    add('ruled-table','table',[dict(spans=[span(t,x=x,y=700-i*30) for i,row in enumerate(cells)
        for x,t in zip([60,260,420],row)],rules=[[45,y,550,y] for y in [720,690,660,630,600]])],
        [' '.join(row) for row in cells],table_rows=cells)
    rtl=['تقرير الميناء اليومي','وصلت السفينة صباح اليوم','إصدار Python 3.12 متاح الآن','المجموع 123 وحدة']
    add('arabic-mixed','direction',[dict(spans=[span(t,x=120,y=730-i*35,size=18,font='Arial')
        for i,t in enumerate(rtl)])],rtl)
    fonts=['Office affinity: efficient filing.', 'Ångström café naïve façade.', 'Σύνολο 42 — résumé accepted.']
    add('font-subsets','fonts',[dict(spans=[span(t,y=730-i*35,font=f)
        for i,(t,f) in enumerate(zip(fonts,['TimesNewRomanPSMT','ArialMT','TimesNewRomanPS-ItalicMT']))])],fonts)
    rotation=['Vertical inventory record','Seven crates cleared inspection']
    add('quarter-turn','rotation',[dict(spans=[span(t,x=80+i*25,y=120,angle=90)
        for i,t in enumerate(rotation)])],rotation)
    scan=['Warehouse receipt','Delivered twelve sealed boxes','Inspection completed on Tuesday']
    scanpage=dict(scan=True,spans=[span(t,y=730-i*30,size=16) for i,t in enumerate(scan)])
    add('scan-only','scan',[scanpage],scan)
    add('native-and-scan','mixed-scan',[dict(spans=[span(prose[0])]),scanpage],[prose[0]]+scan)
    add('two-pages','multipage',[dict(spans=[span(t,y=730-i*20) for i,t in enumerate(prose[:2])]),
        dict(spans=[span(t,y=730-i*20) for i,t in enumerate(prose[2:])])],prose)
    near=['Oak plot: ten seedlings.','Oak plot: soil restored.','Elm plot: nine seedlings.','Elm plot: water added.']
    add('narrow-gutter','columns',[dict(spans=[span(near[c*2+r],x=45+c*155,y=700-r*20,size=11)
        for r in range(2) for c in range(2)])],near)
    return docs


def main():
    if (ROOT/'benchmarks/holdout/manifest.jsonl').exists():
        raise RuntimeError('manifest exists: evaluated inputs must not be overwritten')
    specpath=ROOT/'benchmarks/holdout/synthetic-spec.json'
    specs=specifications()
    if specpath.exists(): assert json.loads(specpath.read_text())==specs, 'specifications frozen'
    else: specpath.write_text(json.dumps(specs,ensure_ascii=False,indent=2)+'\n')
    executable=ROOT/'target/holdout-pdfkit'
    subprocess.run(['swiftc','-module-cache-path','/tmp/bulkparse-swift-cache','-Xcc',
        '-fmodules-cache-path=/tmp/bulkparse-clang-cache',str(ROOT/'scripts/holdout_pdfkit.swift'),
        '-o',str(executable)],check=True)
    subprocess.run([str(executable),'generate',str(specpath),str(DEST/'synthetic')],check=True)
    rows=[]
    for spec in specs:
        path=DEST/'synthetic'/(spec['id']+'.pdf')
        ref=path.with_suffix('.txt'); ref.write_text(spec['expected'])
        rows.append(dict(id='synthetic/'+spec['id'],source_path=str(path),reference_path=str(ref),
            reference_kind='authored_gold',category=spec['category'],ordered_blocks=spec['ordered_blocks'],
            table_rows=spec.get('table_rows'),source_sha256=sha(path),reference_sha256=sha(ref)))
    selection=json.loads((ROOT/'benchmarks/holdout/selection.json').read_text())
    for name in selection['paths']:
        path=DEST/'external'/name
        if name.endswith('scotus-transcript-p1.pdf'):
            ref=DEST/'external/tests/comparisons/scotus-transcript-p1.txt';kind='upstream_reference'
        else:
            ref=path.with_suffix('.pdfkit.txt');kind='silver_pdfkit'
            subprocess.run([str(executable),'extract',str(path),str(ref)],check=True)
        rows.append(dict(id='external/'+path.stem,source_path=str(path),reference_path=str(ref),
            reference_kind=kind,category='external',source_sha256=sha(path),reference_sha256=sha(ref),
            upstream_url=f'https://github.com/jsvine/pdfplumber/blob/{selection["revision"]}/{name}'))
    prior={json.loads(s)['source_sha256'] for s in
        (ROOT/'reports/positioned-pdf-2026-11-10/manifest.jsonl').read_text().splitlines()}
    assert not prior.intersection(r['source_sha256'] for r in rows)
    path=ROOT/'benchmarks/holdout/manifest.jsonl'
    # CoreGraphics may change document IDs and metadata on regeneration.
    # Preserve an evaluated corpus; use a new version for new generated bytes.
    if path.exists(): raise RuntimeError('manifest exists: use a new corpus version, never overwrite evaluated inputs')
    for row in rows:
        for key in ['source_path','reference_path']:
            row[key]=str(Path(row[key]).relative_to(ROOT))
    path.write_text(''.join(json.dumps(r,ensure_ascii=False)+'\n' for r in rows))
    print(f'Prepared {len(rows)} cases with no development-input hash overlap')


if __name__=='__main__': main()
