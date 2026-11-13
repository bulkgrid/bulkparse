#!/usr/bin/env python3
"""Independent exact-row content audit for the nine historically failing CSVs.

This is a narrow source-content check, not a layout/quality score. Non-tabular
outputs are flagged for review, rather than assigned a fabricated zero score.
"""
import argparse
from collections import Counter
import csv
import io
import itertools
import json
from pathlib import Path
import unicodedata

from bs4 import BeautifulSoup
from markdown_it import MarkdownIt
from benchmark_realworld import ROOT, digest, write


def normalize(value):
    return ' '.join(unicodedata.normalize('NFC', value).split())


def reference(path):
    data = path.read_bytes()
    verified = True
    if data.startswith((b'\xff\xfe\x00\x00', b'\x00\x00\xfe\xff')):
        encoding = 'utf-32'
    elif data.startswith((b'\xff\xfe', b'\xfe\xff')):
        encoding = 'utf-16'
    else:
        try:
            data.decode('utf-8-sig')
            encoding = 'utf-8-sig'
        except UnicodeDecodeError:
            encoding = 'cp1252'
            verified = False
    text = data.decode(encoding)
    try:
        dialect = csv.Sniffer().sniff(text[:65536], delimiters=',;\t|')
    except csv.Error:
        # Sniffer counts physical lines incorrectly for some quoted newlines.
        # Accept a fallback only when one delimiter gives consistent multi-cell
        # logical records; ambiguous references remain unscored.
        candidates = []
        for delimiter in ',;\t|':
            try:
                sample = list(itertools.islice(csv.reader(io.StringIO(text, newline=''), delimiter=delimiter, strict=True), 128))
                widths = {len(row) for row in sample if any(row)}
                if len(sample) >= 2 and len(widths) == 1 and next(iter(widths)) > 1:
                    candidates.append(delimiter)
            except csv.Error:
                continue
        if len(candidates) != 1:
            raise csv.Error('Reference delimiter is ambiguous')
        dialect = type('ReferenceDialect', (csv.excel,), {'delimiter': candidates[0]})
    rows = [tuple(normalize(c) for c in row) for row in csv.reader(io.StringIO(text, newline=''), dialect, strict=True)]
    rows = [row for row in rows if any(row)]
    return rows, dict(encoding=encoding, encoding_verified=verified,
                     encoding_note='Strict Unicode decoding succeeded' if verified else 'Exploratory cp1252 decoding only; author encoding not established. No content scores assigned.',
                     delimiter=dialect.delimiter,
                     column_counts=dict(Counter(len(row) for row in rows)))


def audit(markdown, wanted):
    rendered = MarkdownIt('commonmark').enable('table').render(markdown)
    soup = BeautifulSoup(rendered, 'html.parser')
    rows = []
    for table in soup.find_all('table'):
        for tr in table.find_all('tr'):
            if tr.find_parent('table') is not table:
                continue
            cells = tr.find_all(['td', 'th'], recursive=False)
            if cells:
                for cell in cells:
                    for br in cell.find_all('br'):
                        br.replace_with('\n')
                    for block in cell.find_all(['p', 'div', 'li']):
                        block.insert_after('\n')
                rows.append(tuple(normalize(c.get_text(separator='')) for c in cells))
    if not rows:
        return dict(auditable=False, reason='No HTML/GFM table rows; inspect output directly', matched_source_rows=None,
                    extracted_table_rows=0)
    matches = Counter(wanted) & Counter(rows)
    unmatched = Counter(wanted) - matches
    return dict(auditable=True, matched_source_rows=sum(matches.values()), extracted_table_rows=len(rows),
                missing_source_rows=sum(unmatched.values()),
                missing_examples=[list(row) for row in list(unmatched)[:3]])


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--run', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    docs = [d for d in json.loads((ROOT/'benchmarks/realworld-failures/manifest.json').read_text()) if d['format']=='csv']
    assert len(docs) == 9
    engines = [e for e in ['bulkparse', 'anydoc', 'docling', 'bulkparse_markdown', 'bulkparse_recovery', 'bulkparse_markdown_recovery']
               if (args.run/f'{e}-round-1.json').exists()]
    records = {e:{r['id']:r for r in json.loads((args.run/f'{e}-round-1.json').read_text())['documents']} for e in engines}
    results = []
    for d in docs:
        path = ROOT/d['path']
        assert digest(path) == d['sha256']
        try:
            wanted, metadata = reference(path)
            source_count = len(wanted) if metadata['encoding_verified'] else None
        except (csv.Error, UnicodeDecodeError) as error:
            wanted, source_count = [], None
            metadata = dict(encoding_verified=False, reference_error=str(error))
        row = dict(id=d['id'], title=d['title'], source_sha256=d['sha256'], reference=metadata, source_rows=source_count, engines={})
        for e in engines:
            p = args.run/'prediction'/e/'round-1'/(d['id']+'.md')
            assert digest(p) == records[e][d['id']]['output_sha256']
            result = audit(p.read_text(), wanted) if metadata['encoding_verified'] else dict(
                auditable=False, reason='Reference encoding requires independent review',
                matched_source_rows=None, extracted_table_rows=None)
            result['returned'] = records[e][d['id']]['ok']
            result['output_sha256'] = digest(p)
            if not result['returned'] and metadata['encoding_verified']:
                result.update(auditable=True, reason='Conversion failed; no source rows returned', matched_source_rows=0, missing_source_rows=len(wanted))
            row['engines'][e] = result
        results.append(row)
    write(args.out, dict(method=__doc__, documents=results,
        scope='Nine CSVs retained; only strictly decoded Unicode sources audited. Other source encodings require independent review and remain unscored for every engine. Nonblank records including header; Unicode/whitespace normalized; cell order and row multiplicity retained; exact full-row content match. This does not assess row ordering, formatting or other formats. No complete-quality claim.'))


if __name__ == '__main__':
    main()
