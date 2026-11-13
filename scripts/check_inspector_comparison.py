#!/usr/bin/env python3
"""Recompute paired benchmark accounting and summaries from saved samples."""
import itertools
import json
from pathlib import Path
import statistics
import sys

report=Path(sys.argv[1])
meta=json.loads((report/'run.json').read_text())
cases=json.loads((report/'manifest.json').read_text())
rows=[json.loads(s) for s in (report/'samples.jsonl').read_text().splitlines()]
resources=json.loads((report/'resources.json').read_text())
summary=json.loads((report/'summary.json').read_text())
keys=[(r['engine'],r['mode'],r['id'],r['repetition']) for r in rows]
expected=set(itertools.product(['bulkparse','firecrawl'],['inspect','convert'],[c['id'] for c in cases],range(meta['repeats'])))
assert len(keys)==len(set(keys)) and set(keys)==expected
assert all(isinstance(r['ns'],int) and r['ns']>0 for r in rows)
for group in summary:
    selected=[r for r in rows if r['engine']==group['engine'] and r['mode']==group['mode']]
    assert group['attempts']==len(selected)
    assert group['successes']==sum(r['ok'] for r in selected)
    times=[sum(r['ns'] for r in selected if r['repetition']==rep)/1e6 for rep in range(meta['repeats'])]
    assert group['median_corpus_ms']==statistics.median(times)
    assert group['corpus_ms_range']==[min(times),max(times)]
    medians=[];routes=[]
    for c in cases:
        rs=[r for r in selected if r['id']==c['id']]
        assert len({r['output_sha256'] for r in rs})==1
        medians.append(statistics.median(r['ns']/1e6 for r in rs))
        if 'expected_ocr_pages' in c:
            truth=all(r['ok'] and r['ocr_pages']==c['expected_ocr_pages'] for r in rs)
            assert all(r['routing_exact']==truth for r in rs)
            routes.append(truth)
    assert group['median_document_ms']==statistics.median(medians)
    assert group['routing_exact_cases']==sum(routes)
    usage=resources[group['engine']+'-'+group['mode']]
    assert usage['exit_code']==0 and usage['max_rss_bytes']>0
    assert group['max_rss_mib']==usage['max_rss_bytes']/1024**2
print(f'Validated {len(rows)} timed attempts, {len(cases)} inputs, determinism, routing and resource summaries.')
