#!/usr/bin/env python3
"""Validate and summarize measured OCR reuse runs; never fill in missing scores."""
import hashlib
import json
from pathlib import Path
import statistics

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'reports/ocr-reuse-2026-11-11'

def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def rows(folder):return [json.loads(line) for line in (folder/'records.jsonl').read_text().splitlines()]

def validate_run(folder):
    meta=read(folder/'run.json');summary=read(folder/'summary.json')
    for path,digest in meta['source_sha256'].items():assert sha(ROOT/path)==digest,(path,'source drift')
    for item in summary:
        records=rows(folder/item['profile']);assert len(records)==len(meta['cases'])*(meta['passes']+1)
        assert not item['errors'],item['errors']
        assert not item['changed_text_across_runs'],item['changed_text_across_runs']
        for field in ['wall_seconds','cpu_seconds','rpc_wall_seconds']:
            values=[sum(r[field] for r in records if r['run']==n) for n in range(1,meta['passes']+1)]
            assert values==item[field]
            assert statistics.median(values)==item['median_'+field]
    return summary

def comparison(folder, before='legacy', after='reuse-adaptive'):
    a={r['id']:r for r in rows(folder/before) if r['run']==1}
    b={r['id']:r for r in rows(folder/after) if r['run']==1}
    assert a.keys()==b.keys()
    return [{'id':key,'before_character_errors':a[key]['quality']['character_errors'],
             'after_character_errors':b[key]['quality']['character_errors'],
             'text_identical':a[key]['text_sha256']==b[key]['text_sha256']}
            for key in a]

def table(items):
    labels={'legacy':'Before: one process per page','reuse-low-memory':'Model reuse only',
            'reuse-adaptive':'Model reuse + adaptive detection (default)',
            'adaptive-one-shot':'Adaptive detection, one process per page'}
    lines=['| Configuration | Median wall time | Median CPU time | Peak sampled RSS | Character errors | Exact text outputs |',
           '|---|---:|---:|---:|---:|---:|']
    for s in items:
        q=s['quality'];lines.append(f"| {labels[s['profile']]} | {s['median_wall_seconds']:.3f}s | {s['median_cpu_seconds']:.3f}s | {s['peak_rss_bytes']/1048576:.1f} MiB | {q['character_errors']}/{q['reference_characters']} ({100*q['character_error_rate']:.3f}%) | {q['exact']}/{q['cases']} |")
    return '\n'.join(lines)

def main():
    main=validate_run(OUT/'linux');single=validate_run(OUT/'linux-one-shot');fine=validate_run(OUT/'fine-print')
    delta=comparison(OUT/'linux');fine_delta=comparison(OUT/'fine-print')
    regressions=[r for r in delta+fine_delta if r['after_character_errors']>r['before_character_errors']]
    assert not regressions,regressions
    for name,digest in read(OUT/'source-manifest.json').items():
        assert sha(OUT/'source-snapshot'/name)==digest
    native=read(ROOT/'reports/opendataloader-ocr-reuse-2026-11-11/comparison.json')[0]
    old=read(ROOT/'reports/opendataloader-ocr-table-alignment-2026-11-11/comparison.json')[0]
    for key in ['overall_mean','nid_mean','teds_mean','mhs_mean']:assert native[key]==old[key]
    assert read(OUT/'native-regression.json')['identical_markdown_predictions']==200
    before=next(s for s in main if s['profile']=='legacy');after=next(s for s in main if s['profile']=='reuse-adaptive')
    measurements={'main':main+single,'fine_print':fine,'case_comparison':delta,
                  'fine_print_comparison':fine_delta,'native':native,
                  'speedup':before['median_wall_seconds']/after['median_wall_seconds'],
                  'peak_rss_change_percent':100*(after['peak_rss_bytes']/before['peak_rss_bytes']-1),
                  'cpu_reduction_percent':100*(1-after['median_cpu_seconds']/before['median_cpu_seconds'])}
    (OUT/'comparison.json').write_text(json.dumps(measurements,indent=2)+'\n')
    (OUT/'table.md').write_text(table(main+single)+'\n')
    (OUT/'fine-print-table.md').write_text(table(fine)+'\n')
    (OUT/'validation.json').write_text(json.dumps({'passed':True,'source_hashes_verified':True,
        'timing_medians_recomputed':True,'all_outputs_retained':True,'character_error_regressions':regressions,
        'native_markdown_identical':200,'no_cross_run_text_changes':True},indent=2)+'\n')
    print(json.dumps({k:v for k,v in measurements.items() if k in ['speedup','peak_rss_change_percent','cpu_reduction_percent']},indent=2))

if __name__=='__main__':main()
