#!/usr/bin/env python3
"""Current three-converter comparison, retaining historical failures and CPU data."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import statistics
import subprocess
import time

from benchmark_realworld import ROOT, REPORT, Worker, commands, digest, write
from benchmark_opendataloader import snapshot_sources

ENGINES = ['bulkparse', 'anydoc', 'docling']
EXTRA = {'bulkparse_markdown': ['--stream-markdown'],
         'bulkparse_recovery': ['--partial-on-limit'],
         'bulkparse_markdown_recovery': ['--stream-markdown', '--partial-on-limit']}


def power():
    return subprocess.check_output(['pmset', '-g', 'batt'], text=True).strip() if platform.system() == 'Darwin' else None


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--smoke', action='store_true')
    ap.add_argument('--passes', type=int, default=1)
    ap.add_argument('--warmup', action='store_true')
    ap.add_argument('--final-validation', action='store_true', help='Explicitly enable repeated timing after capability/performance targets are met')
    ap.add_argument('--engines', nargs='+', choices=ENGINES, default=ENGINES)
    ap.add_argument('--bulkparse-modes', action='store_true')
    ap.add_argument('--manifest', type=Path, default=REPORT/'corpus-manifest.json')
    ap.add_argument('--bulkparse-binary', type=Path, help='Use a preserved executable instead of the shared build output')
    args = ap.parse_args()
    if args.passes < 1 or (args.passes > 1 and not args.final_validation):
        ap.error('Use one pass for diagnostics; repeated timing requires --final-validation')
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    corpus = json.loads(args.manifest.read_text())
    if args.smoke:
        corpus = [next(d for d in corpus if d['format'] == f) for f in ['csv', 'rtf', 'xlsx']]
    hard = json.loads((ROOT/'benchmarks/realworld-failures/manifest.json').read_text())
    hard_ids = {d['id'] for d in hard}
    assert len(hard_ids) == 27
    if not args.smoke:
        assert len(corpus) == len({d['sha256'] for d in corpus})
        if args.manifest.resolve() == (REPORT/'corpus-manifest.json').resolve():
            assert hard_ids <= {d['id'] for d in corpus}
            assert len(corpus) == 112
    assert all(digest(ROOT/d['path']) == d['sha256'] for d in corpus)
    write(out/'manifest.json', corpus)
    cmds = {e: commands()[e] for e in args.engines}
    if args.bulkparse_binary and 'bulkparse' in cmds:
        cmds['bulkparse'] = [str(args.bulkparse_binary.resolve())]
    if args.bulkparse_modes:
        native = [str(args.bulkparse_binary.resolve())] if args.bulkparse_binary else commands()['bulkparse']
        cmds.update({e: [*native, *flags] for e,flags in EXTRA.items()})
    sources = snapshot_sources(out)
    paths = [Path(__file__), ROOT/'scripts/benchmark_realworld.py', ROOT/'scripts/worker_realworld.py',
             ROOT/'scripts/check_realworld_failures.py', ROOT/'crates/bulkparse/examples/office_bench.rs',
             ROOT/'benchmarks/multiformat/anydoc.rs', ROOT/'benchmarks/realworld/cpu.rs',
             ROOT/'benchmarks/realworld/comparison-2026-11-13.json', ROOT/'benchmarks/realworld/development-policy.json',
             ROOT/'benchmarks/realworld-failures/manifest.json', ROOT/'benchmarks/realworld-failures/expected.json']
    paths += list((ROOT/'legacy/anydoc-main/src').rglob('*.rs')) + list((ROOT/'legacy/anydoc-main').glob('Cargo.*'))
    paths += list((ROOT/'benchmarks/multiformat/anydoc').glob('Cargo.*'))
    for p in paths:
        p = p.resolve()
        name = str(p.relative_to(ROOT))
        sources[name] = digest(p)
        dest = out/'source-snapshot'/name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(p, dest)
    write(out/'source-manifest.json', sources)
    if 'docling' in cmds:
        py = cmds['docling'][0]
        (out/'docling-requirements.txt').write_text(subprocess.check_output([py, '-m', 'pip', 'freeze'], text=True))
    env = dict(os.environ, RAYON_NUM_THREADS='1', OMP_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1',
               MKL_NUM_THREADS='1', HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', TOKENIZERS_PARALLELISM='false')
    binary_hashes = {e: digest(Path(cmd[0])) for e,cmd in cmds.items()}
    write(out/'identity.json', dict(method=__doc__, commands=cmds, binary_sha256=binary_hashes,
        hardware=subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string','hw.memsize','hw.logicalcpu'],text=True).splitlines(),
        platform=platform.platform(), warmup=int(args.warmup), measured_passes=0 if args.smoke else args.passes, deadline_seconds=90,
        purpose='final validation' if args.final_validation else 'one-pass diagnostic',
        environment={k:env[k] for k in ['RAYON_NUM_THREADS','OMP_NUM_THREADS','OPENBLAS_NUM_THREADS','MKL_NUM_THREADS','HF_HUB_OFFLINE','TRANSFORMERS_OFFLINE']},
        started_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), power_before=power(),
        quality='Conversion coverage and performance, not a quality score; status conventions differ across tools.',
        extra_profiles='BulkParse writer and recovery profiles measured separately after the main comparison; never substituted into default results.'))
    if platform.system() == 'Darwin':
        (out/'power-settings.txt').write_text(subprocess.check_output(['pmset','-g','custom'],text=True))
    workers, starts, runs, ready = {}, {e:0 for e in cmds}, {e:[] for e in cmds}, {}
    progress = {'events': []}
    def start(e):
        worker = Worker(cmds[e], out/f'{e}-worker-{starts[e]}.log', env)
        starts[e] += 1
        if e == 'docling':
            ready[e] = worker.receive(240)
            assert ready[e]['ready']
            write(out/'effective-options.json', ready)
        workers[e] = worker
    def run(e, index, order):
        folder = out/'prediction'/e/f'round-{index}'
        folder.mkdir(parents=True)
        rows = []
        print(f'Start round {index} {e}: {len(corpus)} documents', flush=True)
        for n,doc in enumerate(corpus):
            if e not in workers:
                start(e)
            progress.update(engine=e, round=index, completed=n, documents=len(corpus), current=doc['id'],
                            current_started_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
            write(out/'progress.json', progress)
            started = time.perf_counter_ns()
            try:
                row = workers[e].call(ROOT/doc['path'])
                assert row['path'] == str(ROOT/doc['path'])
                assert row.get('cpu_seconds') is not None and row['cpu_seconds'] >= 0
            except (TimeoutError, RuntimeError, BrokenPipeError) as ex:
                row = dict(path=str(ROOT/doc['path']), elapsed_ns=time.perf_counter_ns()-started,
                           cpu_seconds=None, ok=False, status='timeout_or_worker_failure', error=str(ex), markdown='')
                workers.pop(e).close(kill=True)
            text = row.pop('markdown')
            p = folder/(doc['id']+'.md')
            p.write_text(text)
            row.update(id=doc['id'], format=doc['format'], historical_failure=doc['id'] in hard_ids,
                       output_sha256=digest(p), output_bytes=p.stat().st_size)
            rows.append(row)
            with (out/f'{e}-round-{index}.jsonl').open('a') as f:
                f.write(json.dumps(row)+'\n')
            if (n+1)%20 == 0 or not row['ok']:
                print(f'  {n+1}/{len(corpus)} {doc["id"]}: {row["elapsed_ns"]/1e9:.3f}s, returned={row["ok"]}', flush=True)
        write(out/f'{e}-round-{index}.json', dict(index=index, order=order, documents=rows))
        runs[e].append(rows)
        event = dict(engine=e, round=index, returned=sum(r['ok'] for r in rows), seconds=sum(r['elapsed_ns'] for r in rows)/1e9, power=power())
        progress['events'].append(event)
        write(out/'progress.json', progress)
        print('Done', json.dumps(event), flush=True)
    try:
        groups = [args.engines] + ([list(EXTRA)] if args.bulkparse_modes else [])
        for group in groups:
            indexes = [0] if args.smoke else range(0 if args.warmup else 1, args.passes+1)
            for index in indexes:
                order = group[index%len(group):]+group[:index%len(group)]
                for e in order:
                    run(e, index, order)
            for e in group:
                if e in workers:
                    workers.pop(e).close()
    finally:
        for worker in workers.values():
            worker.close()
        resources = {}
        for e in cmds:
            peaks = [int(x) for log in out.glob(e+'-worker-*.log') for x in re.findall(r'(\d+)\s+maximum resident set size', log.read_text(errors='replace'))]
            resources[e] = dict(worker_starts=starts[e], lifetime_peak_rss_bytes=max(peaks) if peaks else None,
                scope='OS worker-lifetime high-water RSS, including warmup and transport; excludes aggregate subprocess-tree peak. Killed workers can lack an OS high-water sample.')
        write(out/'resources.json', resources)
    source_edits = [p for p,h in sources.items() if not (ROOT/p).exists() or digest(ROOT/p) != h]
    # A frozen executable is independent of subsequent edits in a shared checkout.
    # Preserve and verify the original source snapshot, and disclose any drift.
    assert all(digest(out/'source-snapshot'/p) == h for p,h in sources.items())
    if not args.bulkparse_binary:
        assert not source_edits, 'Source changed during measurement'
    assert all(digest(Path(cmd[0])) == binary_hashes[e] for e,cmd in cmds.items())
    assert all(digest(ROOT/d['path']) == d['sha256'] for d in corpus)
    summary = {}
    for e, passes in runs.items():
        measured = passes[1:] if args.warmup and not args.smoke else passes
        docs = []
        for j,d in enumerate(corpus):
            samples = [p[j] for p in measured]
            assert all(r['id'] == d['id'] for r in samples)
            docs.append(dict(id=d['id'], format=d['format'], historical_failure=d['id'] in hard_ids,
                returned_all=all(r['ok'] for r in samples), nonempty_all=all(r['ok'] and r['output_bytes'] for r in samples),
                median_ms=statistics.median(r['elapsed_ns']/1e6 for r in samples),
                median_cpu_ms=statistics.median(r['cpu_seconds']*1000 for r in samples) if all(r.get('cpu_seconds') is not None for r in samples) else None,
                statuses=sorted({r.get('status','returned_unverified' if r['ok'] else 'error') for r in samples}),
                deterministic=len({r['output_sha256'] for r in samples})==1))
        summary[e] = dict(documents=docs, returned_all=sum(d['returned_all'] for d in docs), nonempty_all=sum(d['nonempty_all'] for d in docs),
            corpus_seconds=[sum(r['elapsed_ns'] for r in p)/1e9 for p in measured],
            corpus_cpu_seconds=[sum(r['cpu_seconds'] for r in p) if all(r.get('cpu_seconds') is not None for r in p) else None for p in measured],
            median_document_ms=statistics.median(d['median_ms'] for d in docs), resources=resources[e])
    write(out/'summary.json', summary)
    write(out/'verification.json', dict(complete=True, smoke=args.smoke, all_input_hashes_unchanged=True,
        all_source_hashes_unchanged=not source_edits, source_edits_during_measurement=source_edits,
        source_snapshot_verified=True, all_binary_hashes_unchanged=True,
        finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), power_after=power()))
    print('COMPLETE', flush=True)


if __name__ == '__main__':
    main()
