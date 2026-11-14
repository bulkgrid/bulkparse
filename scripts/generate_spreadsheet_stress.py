#!/usr/bin/env python3
"""Generate deterministic plain, rich-text and merged-cell spreadsheet stress cases."""
import argparse
import zipfile
from pathlib import Path
from benchmark_support import digest, write

def generate(out, scale=1):
    folder = out / 'stress-inputs'
    folder.mkdir(parents=True, exist_ok=False)
    docs = []
    for kind, rows in [('plain', 3000), ('rich', 1500), ('merged', 1500)]:
        rows *= scale
        parts = {
            'xl/workbook.xml': '<workbook xmlns:r="urn:r"><sheets><sheet name="Generated" r:id="a"/></sheets></workbook>',
            'xl/_rels/workbook.xml.rels': '<Relationships><Relationship Id="a" Type="worksheet" Target="a.xml"/></Relationships>',
        }
        sheet = ['<worksheet><sheetData>']
        for r in range(1, rows + 1):
            sheet.append(f'<row r="{r}">')
            for c in 'ABCDEFGH':
                value = f'{r}-{c}'
                if kind == 'plain':
                    cell = f'<c r="{c}{r}" t="str"><v>{value}</v></c>'
                else:
                    cell = f'<c r="{c}{r}" t="inlineStr"><is><r><rPr><b/></rPr><t>{value} 東京</t></r><r><t> tail &amp; |</t></r></is></c>'
                sheet.append(cell)
            sheet.append('</row>')
        sheet.append('</sheetData>')
        if kind == 'merged':
            sheet.append('<mergeCells>')
            for r in range(1, rows + 1, 5):
                sheet.append(f'<mergeCell ref="A{r}:B{r}"/>')
            sheet.append('</mergeCells>')
        if kind != 'plain':
            sheet.append('<hyperlinks>')
            for r in range(1, rows + 1, 50):
                sheet.append(f'<hyperlink ref="B{r}:D{r}" location="target-{r}"/>')
            sheet.append('</hyperlinks>')
        sheet.append('</worksheet>')
        parts['xl/a.xml'] = ''.join(sheet)
        path = folder / f'{kind}.xlsx'
        with zipfile.ZipFile(path, 'w', compression=zipfile.ZIP_DEFLATED) as z:
            for name, value in parts.items():
                info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                z.writestr(info, value.encode())
        docs.append(dict(id=kind, path=str(path.resolve()), sha256=digest(path), rows=rows, cells=rows*8))
    write(out / 'stress-manifest.json', docs)
    return docs


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--scale', type=int, default=1)
    args = parser.parse_args()
    if args.scale < 1:
        parser.error('--scale must be positive')
    generate(args.out.resolve(), args.scale)
