#!/usr/bin/env python3
"""Source-scoped DOCX checks; not a general quality score or blind judgment."""
import argparse
from collections import Counter
from html import unescape
from html.parser import HTMLParser
import json
from pathlib import Path
import posixpath
import re
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[1]
W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'
MC = '{http://schemas.openxmlformats.org/markup-compatibility/2006}'
R = '{http://schemas.openxmlformats.org/officeDocument/2006/relationships}'


def active(node):
    """One representation per compatibility container, current revision only."""
    if node.tag in (W+'del', W+'moveFrom'):
        return
    if node.tag == MC+'AlternateContent':
        choice = node.find(MC+'Choice')
        branch = choice if choice is not None else node.find(MC+'Fallback')
        if branch is not None:
            yield from active(branch)
        return
    yield node
    for child in node:
        yield from active(child)


def paragraph(node):
    return ''.join(n.text or '' for n in active(node) if n.tag == W+'t')


def normalize(text):
    # Punctuation/emphasis boundaries are irrelevant to this text-presence check.
    text = re.sub(r'<[^>]*>', '', text)
    return ''.join(c.casefold() for c in unescape(text) if c.isalnum())


class Spans(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.spans = Counter()
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        if tag in ('td', 'th'):
            a = dict(attrs)
            row, col = int(a.get('rowspan', '1')), int(a.get('colspan', '1'))
            if row > 1:
                self.spans[(row, col)] += 1


def expected_spans(root):
    counts = Counter()
    for table in (n for n in active(root) if n.tag == W+'tbl'):
        chains = {}
        for row in (n for n in table if n.tag == W+'tr'):
            def value(node, key, default):
                e = node.find(key)
                return int(e.get(W+'val', default)) if e is not None else int(default)
            column = value(row, W+'trPr/'+W+'gridBefore', '0')
            following = {}
            for cell in (n for n in row if n.tag == W+'tc'):
                span = value(cell, W+'tcPr/'+W+'gridSpan', '1')
                merge = cell.find(W+'tcPr/'+W+'vMerge')
                key = column, span
                if merge is not None:
                    if merge.get(W+'val') == 'restart':
                        following[key] = [1, span]
                    elif key in chains:
                        chain = chains.pop(key)
                        chain[0] += 1
                        following[key] = chain
                column += span
            for height, width in chains.values():
                if height > 1:
                    counts[(height, width)] += 1
            chains = following
        for height, width in chains.values():
            if height > 1:
                counts[(height, width)] += 1
    return counts


def source_checks(doc):
    checks = []
    with zipfile.ZipFile(ROOT/doc['path']) as package:
        main = 'word/document.xml'
        root = ET.fromstring(package.read(main))
        for box in (n for n in active(root) if n.tag == W+'txbxContent'):
            for p in (n for n in active(box) if n.tag == W+'p'):
                text = paragraph(p)
                if len(normalize(text)) >= 12:
                    checks.append(dict(kind='textbox', part=main, text=text))
        rels_path = 'word/_rels/document.xml.rels'
        rels = {n.get('Id'):n for n in ET.fromstring(package.read(rels_path))} if rels_path in package.namelist() else {}
        seen = set()
        for n in active(root):
            if n.tag not in (W+'headerReference', W+'footerReference'):
                continue
            rel = rels.get(n.get(R+'id'))
            if rel is None or rel.get('TargetMode') == 'External':
                continue
            path = posixpath.normpath(posixpath.join('word', rel.get('Target'))).lstrip('/')
            if path in seen or path not in package.namelist():
                continue
            seen.add(path)
            story = ET.fromstring(package.read(path))
            for p in (n for n in active(story) if n.tag == W+'p'):
                text = paragraph(p)
                if len(normalize(text)) >= 12:
                    checks.append(dict(kind='auxiliary', part=path, text=text))
        return checks, expected_spans(root)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--run', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    corpus = json.loads((args.run/'manifest.json').read_text())
    folders = {'before': ROOT/'reports/automatic-large-documents-2026-11-13/allocation-comparison/prediction/after_structured'}
    folders.update({e:args.run/'prediction'/e/'round-1' for e in ('bulkparse','anydoc','docling')})
    documents = []
    for doc in corpus:
        if doc['format'] != 'docx':
            continue
        checks, spans = source_checks(doc)
        outputs = {e:(p/(doc['id']+'.md')).read_text() for e,p in folders.items()}
        for check in checks:
            needle = normalize(check['text'])
            check['present'] = {e:needle in normalize(text) for e,text in outputs.items()}
        documents.append(dict(id=doc['id'], checks=checks,
            expected_vertical_spans=[dict(rows=h,columns=w,count=n) for (h,w),n in spans.items()],
            matched_vertical_spans={e:sum((spans & Spans(text).spans).values()) for e,text in outputs.items()}))
    summary = {}
    for kind in ('textbox','auxiliary'):
        checks = [c for d in documents for c in d['checks'] if c['kind']==kind]
        summary[kind] = dict(checks=len(checks), documents=sum(any(c['kind']==kind for c in d['checks']) for d in documents),
            found={e:sum(c['present'][e] for c in checks) for e in folders})
    summary['vertical_spans'] = dict(expected=sum(s['count'] for d in documents for s in d['expected_vertical_spans']),
        matched={e:sum(d['matched_vertical_spans'][e] for d in documents) for e in folders})
    args.out.write_text(json.dumps(dict(method=__doc__, limitations=[
        'Targeted checks selected after inspecting failures; not representative quality scores.',
        'Text presence normalizes markup, whitespace and punctuation; it does not establish placement, completeness or uniqueness.',
        'Span checks count matching explicit HTML rowspan/colspan pairs, not cell-label alignment or visual correctness.',
        'Only DOCX text boxes and referenced headers/footers with at least 12 alphanumeric characters are checked.'], summary=summary, documents=documents),indent=2,ensure_ascii=False)+'\n')
    print(json.dumps(summary,indent=2))


if __name__ == '__main__':
    main()
