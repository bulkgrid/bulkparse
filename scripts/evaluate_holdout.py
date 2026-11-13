#!/usr/bin/env python3
"""Frozen-corpus, failure-inclusive evaluation of explicitly configured engines.

One measured process per case by default: timings are observations, not stable
performance estimates. Use >=3 repeats for repeated-process summaries. No OCR.
"""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import platform
import re
import shutil
import statistics
import unicodedata
from benchmark_parsers import run, sha, sequence, bigram_recall
from evaluate_fixtures import compare, source_digest

ROOT=Path(__file__).resolve().parents[1]


def ordered_pair_recall(actual, blocks):
    """Strict authored block-order check; missing blocks count against recall."""
    text=' '+' '.join(sequence(actual))+' '
    positions=[text.find(' '+' '.join(sequence(b))+' ') for b in blocks]
    pairs=len(blocks)*(len(blocks)-1)//2
    correct=sum(0<=positions[i]<positions[j] for i in range(len(blocks)) for j in range(i+1,len(blocks)))
    return {'block_coverage':sum(p>=0 for p in positions)/len(blocks) if blocks else None,
            'ordered_pair_recall':correct/pairs if pairs else None}


def table_recall(actual, expected):
    rows=[]
    for line in actual.splitlines():
        if '|' not in line: continue
        cells=[c.strip() for c in line.strip().strip('|').split('|')]
        if all(re.fullmatch(r':?-+:?',c.replace(' ','')) for c in cells): continue
        rows.append(cells)
    return sum(row in rows for row in expected)/len(expected)


def quality(actual, expected, case):
    result=compare(actual,expected)
    result['bigram_recall_proxy']=bigram_recall(actual,expected)
    # Supplemental diagnostic added after the initial Arabic observation.
    # Keep literal scores; compatibility normalization is not a parser fix.
    normalized=compare(unicodedata.normalize('NFKC',actual),unicodedata.normalize('NFKC',expected))
    result['nfkc_token_recall_proxy']=normalized['token_recall_proxy']
    if case.get('ordered_blocks'):
        result.update(ordered_pair_recall(actual,case['ordered_blocks']))
    if case.get('table_rows'):
        result['exact_markdown_table_row_recall']=table_recall(actual,case['table_rows'])
    return result


def fingerprint_engine(engine):
    if not re.fullmatch(r'[a-zA-Z0-9_-]+',engine['name']): raise ValueError('unsafe engine name')
    if not engine.get('version') or not engine.get('configuration'): raise ValueError('explicit version/configuration required')
    executable=shutil.which(engine['argv'][0])
    if not executable: raise FileNotFoundError(engine['argv'][0])
    # Keep the venv's executable path: resolving its symlink loses pyvenv.cfg.
    engine={**engine,'argv':[str(Path(executable).absolute()),*engine['argv'][1:]],
            'executable_sha256':sha(executable),'executable_bytes':Path(executable).stat().st_size,
            'pin_sha256':{p:sha(p) for p in engine.get('pin_files',[])}}
    from hashlib import sha256
    engine['directory_sha256']={}
    for directory in engine.get('pin_dirs',[]):
        base=Path(directory)
        if not base.is_dir(): raise FileNotFoundError(directory)
        digest=sha256()
        for p in sorted(base.rglob('*')):
            if p.is_file() and '.cache' not in p.relative_to(base).parts:
                digest.update(p.relative_to(base).as_posix().encode()+b'\0'+sha(p).encode()+b'\0')
        engine['directory_sha256'][directory]=digest.hexdigest()
    return engine


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--manifest',type=Path,default=ROOT/'benchmarks/holdout/manifest.jsonl')
    p.add_argument('--freeze',type=Path,default=ROOT/'benchmarks/holdout/freeze.json')
    p.add_argument('--engines',type=Path,required=True)
    p.add_argument('--out',type=Path,required=True)
    p.add_argument('--timeout',type=float,default=120)
    p.add_argument('--repeats',type=int,default=1)
    p.add_argument('--case',action='append')
    args=p.parse_args()
    if args.repeats<1 or args.timeout<=0: p.error('positive repeats and timeout required')
    freeze=json.loads(args.freeze.read_text())
    assert source_digest()==freeze['rust_source_sha256'],'parser source changed after freeze'
    assert sha(ROOT/'target/release/bulkparse')==freeze['binary_sha256'],'parser binary changed after freeze'
    cases=[json.loads(s) for s in args.manifest.read_text().splitlines()]
    if len({c['id'] for c in cases})!=len(cases): p.error('duplicate case IDs')
    if args.case:
        if set(args.case)-{c['id'] for c in cases}: p.error('unknown case ID')
        cases=[c for c in cases if c['id'] in args.case]
    for case in cases:
        assert sha(case['source_path'])==case['source_sha256'],'input changed'
        assert sha(case['reference_path'])==case['reference_sha256'],'reference changed'
    engines=[fingerprint_engine(e) for e in json.loads(args.engines.read_text())]
    if not engines or len({e['name'] for e in engines})!=len(engines): p.error('unique engines required')
    args.out.mkdir(parents=True,exist_ok=False)
    artifacts=ROOT/'target/holdout-results'/args.out.name
    artifacts.mkdir(parents=True,exist_ok=False)
    metadata={'created_utc':datetime.now(timezone.utc).isoformat(),'platform':platform.platform(),
        'python':platform.python_version(),'engines':engines,'manifest_sha256':sha(args.manifest),
        'freeze':freeze,'harness_sha256':sha(__file__),'measurement_runner_sha256':sha(ROOT/'scripts/benchmark_parsers.py'),
        'metric_source_sha256':sha(ROOT/'scripts/evaluate_fixtures.py'),
        'repeats':args.repeats,'warmups':0,'timeout_seconds':args.timeout,
        'scope':'Full CLI process including startup; first-load effects included, no warmups; sequential, no load test. CPU/RSS per child, no hard memory limit on macOS. Failures remain in denominator. Silver references are differential checks, not accuracy gold.',
        'case_ids':[c['id'] for c in cases]}
    snapshots=args.out/'harness'
    snapshots.mkdir()
    for name in ['evaluate_holdout.py','benchmark_parsers.py','evaluate_fixtures.py','benchmark_docling.py']:
        shutil.copyfile(ROOT/'scripts'/name,snapshots/name)
    (args.out/'run.json').write_text(json.dumps(metadata,indent=2)+'\n')
    rows=[]
    with (args.out/'samples.jsonl').open('w') as log:
        for ci,case in enumerate(cases):
            expected=Path(case['reference_path']).read_text()
            for rep in range(args.repeats):
                shift=(ci+rep)%len(engines)
                for engine in engines[shift:]+engines[:shift]:
                    output=artifacts/f'{ci}-{engine["name"]}-{rep}.md'
                    argv=[s.replace('{input}',case['source_path']) for s in engine['argv']]
                    measurement=run(argv,output,args.timeout)
                    success=not measurement['timeout'] and measurement['exit_code'] in engine.get('success_exit_codes',[0])
                    actual=output.read_text(errors='replace') if success else ''
                    row={'id':case['id'],'engine':engine['name'],'category':case['category'],
                        'reference_kind':case['reference_kind'],'repetition':rep,
                        'success':success,'empty':not actual.strip(),'quality':quality(actual,expected,case),**measurement}
                    rows.append(row);log.write(json.dumps(row)+'\n');log.flush()
                    print(f'{case["id"]} {engine["name"]}: {"ok" if success else "FAILED"}, {measurement["wall_ms"]:.0f} ms',flush=True)
    summary=[]
    for case in cases:
        for engine in engines:
            selected=[r for r in rows if r['id']==case['id'] and r['engine']==engine['name']]
            good=[r for r in selected if r['success']]
            stable=len({(r['success'],r['output_sha256']) for r in selected})==1
            summary.append({'id':case['id'],'engine':engine['name'],'category':case['category'],
                'reference_kind':case['reference_kind'],'successful_runs':len(good),'runs':len(selected),
                'deterministic_across_repeats':stable if args.repeats>1 else None,
                'empty':all(r['empty'] for r in selected),
                'quality':selected[0]['quality'] if stable else None,
                **{f'median_{key}':statistics.median(r[key] for r in good) if good else None
                   for key in ['wall_ms','cpu_ms','peak_rss_kib']}})
    assert source_digest()==freeze['rust_source_sha256']
    assert sha(ROOT/'target/release/bulkparse')==freeze['binary_sha256']
    for case in cases:
        assert sha(case['source_path'])==case['source_sha256']
        assert sha(case['reference_path'])==case['reference_sha256']
    for engine in engines:
        assert sha(engine['argv'][0])==engine['executable_sha256']
        for path,expected_hash in engine['pin_sha256'].items(): assert sha(path)==expected_hash
    (args.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')


if __name__=='__main__': main()
