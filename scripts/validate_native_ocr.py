#!/usr/bin/env python3
"""Score saved OCR outputs; this development set is not an unseen benchmark."""
import argparse
import json
from collections import Counter
from pathlib import Path

from benchmark_icdar_tables import aggregate, prediction_tables, score_page
from evaluate_independent_pdf import strings, tokens

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('predictions', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--docling', type=Path, default=ROOT / 'target/scan-parity/docling')
    args = parser.parse_args()
    manifest = json.loads((ROOT / 'target/scan-parity/manifest.json').read_text())
    references = json.loads((ROOT / 'benchmarks/icdar2013-tables/references.json').read_text())
    native = {Path(v['path']).stem: v for v in map(json.loads, args.predictions.read_text().splitlines())}
    results = []
    tables = {'rust': [], 'docling': [], 'previous_worker': []}
    for row in manifest:
        result = {'id': row['id'], 'kind': row['kind']}
        current = native.get(row['id'])
        gold = Counter(tokens(row['reference']))
        if current:
            output = current['result']
            pred = Counter(tokens(' '.join(strings(output['document']['blocks']))))
            result.update(nonempty=bool(output['markdown'].strip()), seconds=current['seconds'],
                          reference_token_recall=sum((gold & pred).values()) / max(1, sum(gold.values())))
        else:
            output = {}
            result.update(nonempty=False, reference_token_recall=0, error='missing prediction')
        if row['kind'] == 'ICDAR':
            source = row['id'].rsplit('-', 1)[0]
            gold_tables = [r for r in references[source] if r['page'] == row['page']]
            previous = json.loads((ROOT / 'target/scan-parity/before' / (row['id'] + '.json')).read_text())
            docling = json.loads((args.docling / (row['id'] + '.json')).read_text())
            predictions = {
                'rust': prediction_tables(output),
                'previous_worker': prediction_tables(previous),
                'docling': [{'cells': [{'row': c['start_row_offset_idx'], 'col': c['start_col_offset_idx'],
                                       'rowspan': c['row_span'], 'colspan': c['col_span'], 'text': c['text']}
                                      for c in t['data']['table_cells']]} for t in docling['tables']],
            }
            for engine, predicted in predictions.items():
                score = score_page(gold_tables, predicted)
                result[engine] = score
                tables[engine].append(score)
        results.append(result)
    report = {'scope': '14 development scans, including 8 ICDAR pages with 11 annotated tables',
              'predictions': str(args.predictions), 'nonempty': sum(r['nonempty'] for r in results),
              'mean_reference_token_recall': sum(r['reference_token_recall'] for r in results) / len(results),
              'tables': {engine: aggregate(pages) for engine, pages in tables.items()}, 'documents': results}
    args.out.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: value for key, value in report.items() if key != 'documents'}, indent=2))


if __name__ == '__main__':
    main()
