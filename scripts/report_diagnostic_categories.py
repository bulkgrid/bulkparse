#!/usr/bin/env python3
"""Summarize diagnostic evidence without treating warnings as a quality oracle."""
import argparse, collections, json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def read(p): return json.loads(p.read_text())
def summarize(rows):
    counts = collections.Counter()
    codes = collections.defaultdict(lambda: {'documents': set(), 'stages': set()})
    documents = []
    for r in rows:
        seen = set()
        categories = set()
        diagnostics = []
        for stage, field in [('parser', 'diagnostics'), ('markdown', 'render_diagnostics')]:
            for d in r.get(field, []):
                assert d['category'] in ('missing_content','formatting_limitation','informational')
                assert d['evidence'] in ('detected','unverified')
                key = (d['category'], d['evidence'], d['code'])
                codes[key]['documents'].add(r['id']); codes[key]['stages'].add(stage)
                codes[key]['message'] = d['message']
                seen.add(key[:2]); categories.add(d['category'])
                diagnostics.append(dict(stage=stage, **d))
        if not r['ok']: bucket = 'failed'
        elif ('missing_content', 'detected') in seen: bucket = 'detected_missing_content'
        elif ('missing_content', 'unverified') in seen: bucket = 'unverified_content_risk_only'
        elif 'formatting_limitation' in categories: bucket = 'formatting_without_content_warning'
        elif 'informational' in categories: bucket = 'informational_only'
        else: bucket = 'no_diagnostics'
        counts[bucket] += 1
        documents.append(dict(id=r['id'], format=r['format'], bucket=bucket, status=r.get('status'), diagnostics=diagnostics))
    by_code = [dict(category=k[0], evidence=k[1], code=k[2], documents=sorted(v['documents']), count=len(v['documents']), stages=sorted(v['stages']), message=v['message']) for k,v in sorted(codes.items())]
    overlapping = {c:sum(any(d['category']==c for d in r['diagnostics']) for r in documents) for c in ['missing_content','formatting_limitation','informational']}
    return dict(exclusive_counts=dict(counts), overlapping_category_counts=overlapping, documents=documents, codes=by_code)

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('records', type=Path, help='JSON list or a round file containing documents')
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    data = read(args.records)
    rows = data if isinstance(data, list) else data['documents']
    with args.out.open('x') as output:
        output.write(json.dumps(summarize(rows), indent=2) + '\n')

if __name__ == '__main__':
    main()
