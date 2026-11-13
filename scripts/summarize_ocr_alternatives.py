#!/usr/bin/env python3
"""Verify raw OCR benchmark records and export comparison tables."""
import argparse
import csv
import hashlib
import json
import statistics
from pathlib import Path
from benchmark_ocr_alternatives import aggregate, score
from evaluate_ocr import distance, normalized


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('report', type=Path)
    args = ap.parse_args()
    run = json.loads((args.report / 'run.json').read_text())
    assert not run['smoke'] and run['passes'] == 5
    cases = run['cases']
    summaries = json.loads((args.report / 'summary.json').read_text())
    assert len(summaries) == len(run['profiles_order'])
    comparison, case_scores = [], []
    for summary in summaries:
        name = summary['profile']
        assert not summary.get('failed'), name
        rows = [json.loads(s) for s in (args.report / name / 'records.jsonl').read_text().splitlines()]
        assert len(rows) == len(cases) * 6, name
        for i, row in enumerate(rows):
            c = cases[i % len(cases)]
            assert row['id'] == c['id'] and row['run'] == i // len(cases)
            actual = '\n'.join(p['text'] for p in row['pages'])
            assert score(actual, c['reference_text']) == row['quality'], (name, c['id'])
            assert hashlib.sha256(actual.encode()).hexdigest() == row['text_sha256']
        measured = [r for r in rows if r['run'] == 1]
        assert aggregate([r['quality'] for r in measured]) == summary['quality']
        for kind in ['wall', 'cpu']:
            totals = [sum(r[kind + '_seconds'] for r in rows if r['run'] == i) for i in range(1, 6)]
            assert totals == summary['full_corpus_' + kind + '_seconds']
            assert statistics.median(totals) == summary['median_' + kind + '_seconds']
        no_space_errors = no_space_chars = 0
        for case, row in zip(cases, measured):
            actual = ''.join(normalized('\n'.join(p['text'] for p in row['pages'])).split())
            ref = ''.join(normalized(case['reference_text']).split())
            errors = distance(ref, actual)
            no_space_errors += errors
            no_space_chars += len(ref)
            case_scores.append({'profile': name, 'case': case['id'], 'track': case['track'],
                                'language': case['language'], **row['quality'],
                                'character_errors_ignoring_spaces': errors,
                                'reference_characters_ignoring_spaces': len(ref),
                                'page_errors': len(row['errors'])})
        comparison.append({'profile': name,
                           'median_wall_seconds_26_pages': summary['median_wall_seconds'],
                           'median_cpu_seconds_26_pages': summary['median_cpu_seconds'],
                           'peak_rss_mib': summary['peak_rss_bytes'] / 1048576,
                           'init_seconds_once': summary['startup']['init_wall_seconds'],
                           'container_launch_to_ready_seconds_once': summary['startup']['container_launch_to_ready_seconds'],
                           'exact_documents_of_24': summary['quality']['exact'],
                           'character_errors_all': summary['quality']['character_errors'],
                           'character_error_rate_all': summary['quality']['character_error_rate'],
                           'word_error_rate_all': summary['quality']['word_error_rate'],
                           'character_errors_stress': summary['by_track']['authored_stress']['character_errors'],
                           'reference_characters_stress': summary['by_track']['authored_stress']['reference_characters'],
                           'character_error_rate_stress': summary['by_track']['authored_stress']['character_error_rate'],
                           'exact_stress_of_16': summary['by_track']['authored_stress']['exact'],
                           'character_error_rate_ignoring_spaces_posthoc': no_space_errors / no_space_chars,
                           'error_documents_first_pass': summary['error_documents_first_pass'],
                           'changed_text_across_runs': len(summary['changed_text_across_runs'])})
    comparison.sort(key=lambda r: r['median_wall_seconds_26_pages'])
    for filename, values in [('comparison.csv', comparison), ('case-scores.csv', case_scores)]:
        with (args.report / filename).open('w') as f:
            writer = csv.DictWriter(f, fieldnames=list(values[0]))
            writer.writeheader()
            writer.writerows(values)
    (args.report / 'comparison.json').write_text(json.dumps(comparison, indent=2) + '\n')
    table = ['| Configuration | Warm time / 26 pages | CPU seconds / 26 pages | Peak RSS | All-case CER | Stress text errors / 1,940 chars | Exact docs / 24 |',
             '| --- | ---: | ---: | ---: | ---: | ---: | ---: |']
    for r in comparison:
        table.append(f"| {r['profile']} | {r['median_wall_seconds_26_pages']:.2f}s | "
                     f"{r['median_cpu_seconds_26_pages']:.2f}s | {r['peak_rss_mib']:.0f} MiB | "
                     f"{100*r['character_error_rate_all']:.2f}% | "
                     f"{r['character_errors_stress']} | {r['exact_documents_of_24']} |")
    (args.report / 'table.md').write_text('\n'.join(table) + '\n')
    print('\n'.join(table))


if __name__ == '__main__':
    main()
