#!/usr/bin/env python3
"""Original OCR expectations authored before evaluation; macOS CoreText producer."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def span(text, x=48, y=730, size=16):
    return {'text':text, 'x':x, 'y':y, 'size':size, 'font':'Arial'}


def specs():
    prose = ['Community library notice', 'The library opens at eight in the morning.',
             'Return borrowed books before Friday.', 'Please keep the reading room quiet.']
    numbers = ['Invoice number: BP-4827', 'Units: 18', 'Unit price: 24.50', 'Total: 441.00']
    swedish = ['Leveransrapport', 'Åtta lådor anlände på måndagen.', 'Kontrollen är klar.']
    left = ['Maple team planted oak trees.', 'Roots received fresh water.', 'Work finished before lunch.']
    right = ['Willow team repaired fences.', 'Posts were checked carefully.', 'Tools returned to storage.']
    def page(lines): return {'scan':True,'spans':[span(t,y=730-i*32) for i,t in enumerate(lines)]}
    return [
        {'id':'ocr-prose','pages':[page(prose)],'expected_lines':prose},
        {'id':'ocr-numbers','pages':[page(numbers)],'expected_lines':numbers},
        {'id':'ocr-swedish','pages':[page(swedish)],'expected_lines':swedish},
        {'id':'ocr-columns','pages':[{'scan':True,'spans':[
            span(t,x=x,y=730-i*26,size=11) for x,ls in [(42,left),(320,right)] for i,t in enumerate(ls)]}],
         'expected_lines':left+right},
        {'id':'ocr-blank','pages':[page([])],'expected_lines':[]},
        {'id':'ocr-two-pages','pages':[page(prose),page(numbers)],'expected_lines':prose+numbers},
    ]


def main():
    out = ROOT/'fixtures/ocr'
    out.mkdir(parents=True,exist_ok=False)
    data = specs()
    spec = out/'spec.json'; spec.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
    for case in data:
        (out/(case['id']+'.txt')).write_text('\n'.join(case['expected_lines'])+'\n')
    binary = ROOT/'target/holdout-pdfkit'
    if not binary.is_file():
        subprocess.run(['swiftc','-module-cache-path','/tmp/bulkparse-swift-cache',
                        str(ROOT/'scripts/holdout_pdfkit.swift'),'-o',str(binary)],check=True)
    subprocess.run([str(binary),'generate',str(spec),str(out)],check=True)


if __name__=='__main__': main()
