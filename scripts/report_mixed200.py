#!/usr/bin/env python3
"""Render verified mixed-corpus results; never overwrite the README or published evidence."""
import argparse
import collections
import json
from pathlib import Path
import statistics

from check_mixed200 import check


def render(run):
    check(run)
    identity = json.loads((run/'identity.json').read_text())
    manifest = json.loads((run/'manifest.json').read_text())
    resources = json.loads((run/'resources.json').read_text())
    engines = list(identity['commands'])
    counts = collections.Counter(d['format'] for d in manifest)
    rows = {}
    for engine in engines:
        rows[engine] = [json.loads((run/f'{engine}-round-{i}.json').read_text())['documents']
                       for i in range(1, identity['measured_passes']+1)]
    def median(values):
        return f'{statistics.median(values):.2f}' if values else '—'
    def document_medians(runs, fmt=None):
        return [statistics.median(run[i]['elapsed_ns']/1e6 for run in runs)
                for i, row in enumerate(runs[0])
                if (fmt is None or row['format'] == fmt)
                and all(run[i]['ok'] and run[i]['output_bytes'] for run in runs)]
    lines = [f'# Mixed-document results: {len(manifest)} documents, {len(counts)} formats', '',
             f"{identity['measured_passes']} measured pass(es); {identity['warmup']} excluded warm-up pass(es). OCR off.", '',
             '| Engine | Nonempty outputs (each pass) | Formats with output (each pass) | Median successful ms | Median corpus seconds | Peak worker MiB |',
             '| --- | --- | --- | ---: | ---: | ---: |']
    for engine, runs in rows.items():
        successful = [[r for r in run if r['ok'] and r['output_bytes']] for run in runs]
        peak = resources[engine].get('lifetime_peak_rss_bytes')
        coverage = ', '.join(f'{len(rs)}/{len(manifest)}' for rs in successful)
        formats = ', '.join(f'{len({r["format"] for r in rs})}/{len(counts)}' for rs in successful)
        times = median(document_medians(runs))
        total = median([sum(r['elapsed_ns'] for r in rs)/1e9 for rs in runs])
        memory = f'{peak/2**20:.1f}' if peak is not None else '—'
        lines.append(f'| {engine} | {coverage} | {formats} | {times} | {total} | {memory} |')
    lines += ['', 'Coverage is not quality. Successful subsets differ. Document times are medians across measured passes, then across documents with nonempty output in every pass. Corpus time includes failed attempts; unsupported inputs take no conversion time. Memory is worker lifetime peak RSS, including initialization; terminated workers can leave incomplete accounting.', '',
              'Per-format cells show nonempty outputs (%) per pass on the first line and median successful conversion time (ms) on the second. Percentages measure coverage, not accuracy; — means unsupported or no successful timing.', '',
              '| Format | ' + ' | '.join(engines) + ' |', '| --- | ' + ' | '.join('---:' for _ in engines) + ' |']
    for fmt, count in sorted(counts.items()):
        cells = []
        for engine in engines:
            if all(r.get('status') == 'unsupported' for rs in rows[engine] for r in rs if r['format'] == fmt):
                cells.append('—')
                continue
            samples = [[r for r in rs if r['format']==fmt and r['ok'] and r['output_bytes']] for rs in rows[engine]]
            coverage = ', '.join(f'{100*len(rs)/count:.1f}'.removesuffix('.0') + '%' for rs in samples)
            timing = median(document_medians(rows[engine], fmt))
            cells.append(f'{coverage}<br>{timing}' + (' ms' if timing != '—' else ''))
        lines.append(f'| {fmt} | ' + ' | '.join(cells) + ' |')
    return '\n'.join(lines) + '\n'


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    result = render(args.run)
    with args.out.open('x') as output:
        output.write(result)
