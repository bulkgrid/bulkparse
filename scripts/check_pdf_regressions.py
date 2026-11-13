#!/usr/bin/env python3
"""Verify native PDF fixture improvements and preserve baseline input identity."""
import argparse
import json
from pathlib import Path
import re
from benchmark_parsers import bigram_recall
from evaluate_fixtures import reference


def check(evaluation, baseline):
    assert (evaluation/'manifest.jsonl').read_bytes() == (baseline/'manifest.jsonl').read_bytes(), 'fixture manifest changed'
    rows = {r['id']:r for r in map(json.loads,(evaluation/'results.jsonl').read_text().splitlines())}
    old = {r['id']:r for r in map(json.loads,(baseline/'results.jsonl').read_text().splitlines())}
    manifest = {r['id']:r for r in map(json.loads,(evaluation/'manifest.jsonl').read_text().splitlines())}
    checked=[]
    for name, minimum in [('newspaper-00',.88),('elsevier-00',.97),('right_to_left_01',.90),('table_mislabeled_as_picture',.94)]:
        key=f'docling/pdf/sources/{name}.pdf';row=rows[key]
        assert row['outcome']=='partial'
        assert row['comparison']['token_recall_proxy'] >= minimum
        text=Path(row['markdown_run']['stdout_path']).read_text()
        if name=='newspaper-00':
            normalized=re.sub(r'\s+',' ',text.replace('\\',''))
            anchors=['Heute lest ihr','hohen Kosten für Druck','und Verteilung unserer']
            indexes=[normalized.index(a) for a in anchors]
            assert indexes==sorted(indexes), 'newspaper columns interleave'
        if name=='right_to_left_01':
            assert 'تحسين الإنتاجية وحل المشكلات' in text
            assert 'Python' in text and 'nohtyP' not in text
        checked.append({'id':key,'before_token_recall_proxy':old[key]['comparison']['token_recall_proxy'],
                        'after_token_recall_proxy':row['comparison']['token_recall_proxy'],
                        'bigram_recall_proxy':bigram_recall(text,reference(manifest[key]))})
    # No material regressions in primary PDF lexical coverage. Structural scores
    # remain separate; supplemental ODF exports are reported rather than hidden.
    for key,row in rows.items():
        if row['group']=='quality' and row['format']=='pdf':
            assert row['outcome'] not in {'failed','crashed','stopped'}
            before=old[key].get('comparison',{}).get('token_recall_proxy')
            after=row.get('comparison',{}).get('token_recall_proxy')
            if before is not None and after is not None: assert after >= before-.005, key
    return {'checks':checked, 'primary_pdf_conversion_failures':0,
            'note':'Token/bigram checks are triage measures. Layout, tables and OCR are not thereby proven correct.'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('evaluation',type=Path)
    parser.add_argument('--baseline',type=Path,default=Path(__file__).resolve().parents[1]/'reports/fixture-fixes-2026-11-10-run2')
    args=parser.parse_args();print(json.dumps(check(args.evaluation,args.baseline),indent=2))
