#!/usr/bin/env python3
"""Generated XLSX stress fixtures, separate from the real-world corpus and quality scores."""
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import zipfile

from benchmark_realworld import ROOT, Worker, digest, write

OUT = ROOT / 'reports/xlsx-output-streaming-2026-11-13'


def generate():
    folder = OUT / 'stress-inputs'
    folder.mkdir(exist_ok=False)
    docs = []
    for kind, rows in [('plain', 3000), ('rich', 1500), ('merged', 1500)]:
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
        docs.append(dict(id=kind, path=str(path.relative_to(ROOT)), sha256=digest(path), rows=rows, cells=rows*8))
    write(OUT / 'stress-manifest.json', docs)
    return docs


def main():
    docs = generate()
    out = OUT / 'stress-benchmark'
    out.mkdir(exist_ok=False)
    binaries = {'before': OUT / 'before-office-bench', 'after': ROOT / 'target/release/examples/office_bench'}
    hashes = {k: digest(p) for k,p in binaries.items()}
    env = dict(os.environ, RAYON_NUM_THREADS='1', OMP_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1', MKL_NUM_THREADS='1')
    results = []
    fields = ['ok','status','output_sha256','diagnostics','render_diagnostics','error_code','error']
    for doc in docs:
        for repetition in range(3):
            logs = {k: out / f'{doc["id"]}-{repetition}-{k}.log' for k in binaries}
            workers = {k: Worker([str(p), '--stream-markdown'], logs[k], env) for k,p in binaries.items()}
            runs = {k: [] for k in binaries}
            try:
                for run in range(6):
                    order = list(binaries)
                    if (run+repetition)%2: order.reverse()
                    for name in order:
                        row = workers[name].call(ROOT / doc['path'])
                        row['output_sha256'] = hashlib.sha256(row.pop('markdown').encode()).hexdigest()
                        assert row['ok'], (doc['id'], row)
                        runs[name].append(row)
                    assert all(runs['before'][-1].get(k)==runs['after'][-1].get(k) for k in fields)
            finally:
                for worker in workers.values(): worker.close()
            write(out / f'{doc["id"]}-{repetition}-passes.json', runs)
            for name, passes in runs.items():
                peak = re.search(r'(\d+)\s+maximum resident set size', logs[name].read_text())
                results.append(dict(id=doc['id'], repetition=repetition, version=name,
                                    median_ms=statistics.median(r['elapsed_ns']/1e6 for r in passes[1:]),
                                    peak_rss_bytes=int(peak[1]), status=passes[1]['status']))
            print(doc['id'], repetition, 'matched', flush=True)
    assert hashes == {k: digest(p) for k,p in binaries.items()}
    assert all(digest(ROOT/d['path'])==d['sha256'] for d in docs)
    write(out / 'summary.json', dict(method=__doc__, binary_sha256=hashes, warmup=1, passes=5, independent_pairs=3, output_differences=[], results=results))


if __name__ == '__main__':
    main()
