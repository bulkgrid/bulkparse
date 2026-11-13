#!/usr/bin/env python3
"""Compare current BulkParse, pinned AnyDoc and table-enabled Docling on 200 PDFs.

No parser tuning or output repair. One warm-up and five measured full-corpus
passes per persistent engine, with rotating engine order. Failures remain scored.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import selectors
import shutil
import statistics
import subprocess
import sys
import time

from benchmark_opendataloader import ROOT, REVISION, EVALUATOR_FILES, validate_corpus, evaluate, write_json, sha256

ENGINES = ['bulkparse', 'anydoc', 'docling']
BULK_REPORT = ROOT/'reports/opendataloader-table-boundaries-2026-11-11-v3'
BULK_WORKER = ROOT/'target/opendataloader-build/opendataloader-table-boundaries-2026-11-11-v3/release/bulkparse-opendataloader-bench'
ANY_WORKER = ROOT/'target/same-machine-benchmark/anydoc-build/release/anydoc-bench-worker'
DOCLING_PY = ROOT/'target/competitors/docling-venv/bin/python'
MODELS = ROOT/'target/competitors/models'


def read(p):
    return json.loads(p.read_text())


def validate_run(run, paths, directory):
    ids = [p.stem for p in paths]
    assert [d['document_id'] for d in run['documents']] == ids, 'Missing, duplicate or reordered outcomes'
    expected = {x+'.md' for x in ids}
    assert {p.name for p in directory.glob('*.md')} == expected, 'Missing or extra predictions'
    assert run['elapsed_ns'] > 0
    assert sum(d['elapsed_ns'] for d in run['documents']) <= run['elapsed_ns']
    for d in run['documents']:
        assert d['ok'] == (d['error'] is None)
        if not d['ok']:
            assert (directory/(d['document_id']+'.md')).read_bytes() == b'', 'Stale output on failure'
    return {p.name: sha256(p) for p in sorted(directory.glob('*.md'))}


class Worker:
    def __init__(self, argv, stderr, env):
        self.log = stderr.open('w')
        self.p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=self.log, cwd=ROOT, env=env)
        self.pending = b''
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.p.stdout, selectors.EVENT_READ)

    def receive(self, timeout=3600):
        deadline = time.monotonic()+timeout
        while b'\n' not in self.pending:
            remaining = deadline-time.monotonic()
            if remaining <= 0:
                raise TimeoutError('Worker response deadline exceeded; run is incomplete')
            if not self.selector.select(min(remaining, 1.0)):
                continue
            chunk = os.read(self.p.stdout.fileno(), 1 << 20)
            if not chunk:
                raise RuntimeError(f'Worker exited early ({self.p.poll()})')
            self.pending += chunk
        line, self.pending = self.pending.split(b'\n', 1)
        return json.loads(line)

    def call(self, request):
        self.p.stdin.write((json.dumps(request)+'\n').encode())
        self.p.stdin.flush()
        return self.receive()

    def close(self):
        self.p.stdin.close()
        try:
            code = self.p.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.p.kill()
            self.p.wait()
            code = -9
        self.selector.close()
        self.p.stdout.close()
        self.log.close()
        return code


def snapshot(out, bulk_report, bulk_worker):
    # Pin the previously tested current core and its already built worker.
    manifest = read(bulk_report/'source-manifest.json')
    for name, digest in manifest.items():
        assert sha256(ROOT/name) == digest, f'BulkParse changed: {name}'
        assert sha256(bulk_report/'source-snapshot'/name) == digest
    assert sha256(bulk_worker) == read(bulk_report/'environment.json')['binary_sha256']
    paths = {ROOT/name for name in manifest}
    paths.update((ROOT/'bindings').rglob('*.d.ts'))
    paths.update((ROOT/'anydoc-main/src').rglob('*.rs'))
    paths.update((ROOT/'anydoc-main').glob('Cargo.*'))
    paths.update((ROOT/'anydoc-main').glob('LICENSE*'))
    paths.update((ROOT/'benchmarks/same-machine').rglob('*'))
    paths.update(ROOT/'scripts'/name for name in ['worker_docling_corpus.py', 'benchmark_same_machine.py',
                                                'check_same_machine_results.py', 'test_same_machine_benchmark.py'])
    result = {}
    for p in sorted(paths):
        if not p.is_file():
            continue
        name = str(p.relative_to(ROOT))
        dest = out/'source-snapshot'/name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(p, dest)
        result[name] = sha256(p)
        assert sha256(dest) == result[name]
    write_json(out/'source-manifest.json', result)
    return result


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--bulk-report', type=Path, default=BULK_REPORT)
    ap.add_argument('--bulk-worker', type=Path, default=BULK_WORKER)
    a = ap.parse_args()
    bulk_report, bulk_worker = a.bulk_report.resolve(), a.bulk_worker.resolve()
    out = a.out.resolve()
    if out.exists():
        raise ValueError('Use a fresh report directory')
    upstream = ROOT/'target/opendataloader-bench'
    assert subprocess.check_output(['git', '-C', str(upstream), 'rev-parse', 'HEAD'], text=True).strip() == REVISION
    assert not subprocess.check_output(['git', '-C', str(upstream), 'status', '--porcelain', '--untracked-files=no'], text=True)
    pdfs = validate_corpus(upstream)
    previous_corpus = read(bulk_report/'corpus-manifest.json')
    for name, digest in previous_corpus['sha256'].items():
        assert sha256(upstream/name) == digest
    out.mkdir(parents=True)
    sources = snapshot(out, bulk_report, bulk_worker)
    write_json(out/'corpus-manifest.json', previous_corpus)
    for name in EVALUATOR_FILES+['../LICENSE', '../THIRD_PARTY_LICENSES.txt', '../THIRD_PARTY_NOTICES.md']:
        src = upstream/'src'/name
        dest = out/'evaluator'/Path(name).name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dest)
    # Capture installed package versions and actual implementation bytes.
    metadata = subprocess.check_output([str(DOCLING_PY), '-c',
        "import json,importlib.metadata as m,platform;print(json.dumps({'python':platform.python_version(),'packages':{d.metadata['Name']:d.version for d in m.distributions()}}))"], text=True)
    package_root = ROOT/'target/competitors/docling-venv/lib/python3.12/site-packages'
    installed = {str(p.relative_to(package_root)):sha256(p)
                 for name in ['docling','docling_core','docling_parse','docling_ibm_models']
                 for p in sorted((package_root/name).rglob('*')) if p.is_file() and p.suffix in ['.py','.so','.dylib']}
    write_json(out/'docling-installed-source-hashes.json', installed)
    model_files = {str(p.relative_to(MODELS)):sha256(p) for p in sorted(MODELS.rglob('*')) if p.is_file()}
    write_json(out/'models.json', {'sha256':model_files,
        'layout':read(ROOT/'target/competitors/docling-model.json'),
        'table':{'repo':'docling-project/docling-models','tag':'v2.3.0',
                 'revision':read(ROOT/'target/same-machine-benchmark/docling-model-api.json')['sha'],
                 'mode':'accurate','engine':'TableFormer V1'}})
    env = dict(os.environ, RAYON_NUM_THREADS='1', HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1',
               HF_HOME=str(ROOT/'target/competitors/hf-cache'), OMP_NUM_THREADS='4', MKL_NUM_THREADS='4',
               TOKENIZERS_PARALLELISM='false')
    commands = {'bulkparse':[str(bulk_worker)], 'anydoc':[str(ANY_WORKER)],
                'docling':[str(DOCLING_PY),str(ROOT/'scripts/worker_docling_corpus.py'),'--artifacts',str(MODELS)]}
    identity = {'started_utc':datetime.now(timezone.utc).isoformat(),'platform':platform.platform(),
        'bulkparse_reference_report':str(bulk_report.relative_to(ROOT)),
        'processor':'Apple M3 Max','memory_bytes':38654705664,'logical_cpus':14,
        'hardware_source':'sysctl machdep.cpu.brand_string hw.memsize hw.logicalcpu verified before run',
        'command':sys.argv,'worker_commands':commands,'docling_environment':json.loads(metadata),
        'binary_sha256':{'bulkparse':sha256(bulk_worker),'anydoc':sha256(ANY_WORKER),'docling_python':sha256(DOCLING_PY)},
        'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),
        'versions':{'bulkparse':'0.1.0','anydoc':'0.2.4 / pdf-inspector 1.14.2','docling':'2.150.0 / docling-parse 7.23.0'},
        'protocol':{'documents':200,'warmup_runs':1,'measured_runs':5,'document_order':'lexicographic',
          'engine_order':'rotate bulkparse/anydoc/docling by round index','persistent_processes_per_engine':1,
          'document_parallelism':1,'ocr':False,'docling_model_parsing':True,'native_model_parsing':False,
          'docling_cpu_threads':4,'native_rayon_threads':1,'docling_tables':'TableFormer V1 accurate, cell matching on',
          'export':'each engine default Markdown; no repair or custom table serialization',
          'failure_policy':'keep all 200; empty Markdown for failed conversion; partial outputs retained',
          'timed':'file reads, conversion, default Markdown export, output writes, diagnostic bookkeeping',
          'excluded':'startup/model load, compilation, request/response serialization, output hashing, evaluation',
          'cache':'warm filesystem; no result cache, fsync, cache flush or CPU pinning',
          'run_timeout_seconds':3600}}
    write_json(out/'identity.json', identity)
    runs = {engine:[] for engine in ENGINES}
    workers = {}
    try:
        for engine in ENGINES:
            workers[engine] = Worker(commands[engine],out/(engine+'.stderr.log'),env)
            if engine == 'docling':
                ready = workers[engine].receive(timeout=180)
                assert ready['ready'] and ready['options']['do_ocr'] is False and ready['options']['do_table_structure'] is True
                assert ready['options']['table_structure_options']['mode'] == 'accurate'
                assert ready['options']['table_structure_options']['do_cell_matching'] is True
                write_json(out/'docling-effective-options.json', ready)
        for index in range(6):
            order = ENGINES[index%3:]+ENGINES[:index%3]
            for engine in order:
                prediction = out/'prediction'/engine/f'round-{index}'/'markdown'
                print(f'Start round {index} {engine}',flush=True)
                reply = workers[engine].call({'documents':[str(p) for p in pdfs],'output_dir':str(prediction)})
                hashes = validate_run(reply,pdfs,prediction)
                reply.update(index=index,warmup=index==0,engine_order=order,prediction_sha256=hashes)
                runs[engine].append(reply)
                write_json(out/'runs.json',runs)
                print(f'Round {index} {engine}: {reply["elapsed_ns"]/1e9:.6f}s, {sum(d["ok"] for d in reply["documents"])}/200 returns',flush=True)
    finally:
        codes = {name:worker.close() for name,worker in workers.items()}
        write_json(out/'worker-exits.json',codes)
    assert all(code == 0 for code in codes.values())
    assert all(len(runs[e]) == 6 for e in ENGINES)
    for name,digest in sources.items():
        assert sha256(ROOT/name) == digest and sha256(out/'source-snapshot'/name) == digest
    for name,digest in model_files.items():
        assert sha256(MODELS/name) == digest
    for name,digest in installed.items():
        assert sha256(package_root/name) == digest
    sys.path.insert(0,str(upstream/'src'))
    comparison = []
    for engine in ENGINES:
        rs = runs[engine]
        deterministic = all(r['prediction_sha256'] == rs[0]['prediction_sha256'] for r in rs)
        indexes = [5] if deterministic else list(range(1,6))
        evaluations = []
        median = statistics.median(r['elapsed_ns'] for r in rs[1:])/1e9
        for index in indexes:
            d = out/'prediction'/engine/f'round-{index}'
            summary = {'engine_name':engine,'engine_version':identity['versions'][engine],
                       'processor':identity['processor'],'document_count':200,'total_elapsed':median,
                       'date':datetime.now(timezone.utc).date().isoformat()}
            evaluations.append(evaluate(upstream,d/'markdown',d,engine,summary))
        metrics = {k:statistics.mean(e['metrics']['score'][k] for e in evaluations)
                   for k in evaluations[0]['metrics']['score']}
        comparison.append({'engine':engine,'version':identity['versions'][engine],
            'median_seconds':median,'measured_seconds':[r['elapsed_ns']/1e9 for r in rs[1:]],
            'deterministic':deterministic,'quality_rounds':indexes,'quality_aggregation':'one deterministic output' if deterministic else 'mean of five measured runs',
            'metrics':metrics,'metric_counts':{k:evaluations[0]['metrics'][k] for k in ['nid_count','teds_count','mhs_count']},
            'failed_documents_last_pass':[d['document_id'] for d in rs[-1]['documents'] if not d['ok']],
            'partial_documents_last_pass':[d['document_id'] for d in rs[-1]['documents'] if d['status'] in ['partial','partial_success']],
            'failures_all_runs':sum(not d['ok'] for r in rs for d in r['documents'])})
    write_json(out/'comparison.json',comparison)
    print(json.dumps(comparison,indent=2),flush=True)


if __name__ == '__main__':
    main()
