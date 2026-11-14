#!/usr/bin/env python3
"""POSIX subprocess benchmark with pinned argv, files, settings and binary hashes.

Each engine writes Markdown to stdout. No shell expansion, package installation,
model downloads, or automatic competitor discovery. Include all engine startup
costs. Use separate runs for native-text and OCR pipelines.
"""
from bulkparse_cli import parse_command
import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import resource
import shutil
import signal
import statistics
import subprocess
import time
from evaluate_fixtures import compare, reference, source_digest

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CASES = ['docling/pdf/sources/' + name + '.pdf' for name in
                 ['newspaper-00', 'elsevier-00', 'right_to_left_01', 'normal_4pages']]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sequence(text):
    # Order-sensitive triage signal; not table/layout accuracy or a gold score.
    text = re.sub(r'!?(\[[^\]]*\])\([^)]*\)', r'\1', text)
    text = re.sub(r'<[^>]*>', ' ', text).replace('\\', '')
    return re.findall(r'\w+', text.lower(), flags=re.UNICODE)


def bigram_recall(actual, expected):
    a, b = sequence(actual), sequence(expected)
    a, b = Counter(zip(a, a[1:])), Counter(zip(b, b[1:]))
    return sum((a & b).values()) / sum(b.values()) if b else None


def run(argv, output, timeout=30):
    def limits():
        resource.setrlimit(resource.RLIMIT_CPU, (math.ceil(timeout), math.ceil(timeout) + 1))
        resource.setrlimit(resource.RLIMIT_FSIZE, (32 * 1024**2,) * 2)
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    started = time.perf_counter()
    timed_out = False
    with output.open('wb') as stdout, output.with_suffix('.stderr').open('wb') as stderr:
        process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                                   start_new_session=True, preexec_fn=limits)
        def timeout_handler(_signum, _frame):
            nonlocal timed_out
            timed_out = True
            try: os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError: pass
        old_handler = signal.signal(signal.SIGALRM, timeout_handler)
        signal.setitimer(signal.ITIMER_REAL, timeout)
        try:
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, old_handler)
            if process.returncode is None:
                process.kill(); process.wait()
    return {'exit_code': process.returncode, 'timeout': timed_out,
            'wall_ms': (time.perf_counter() - started) * 1000,
            'cpu_ms': (usage.ru_utime + usage.ru_stime) * 1000,
            'peak_rss_kib': usage.ru_maxrss / (1024 if platform.system() == 'Darwin' else 1),
            'output_sha256': sha(output), 'output_bytes': output.stat().st_size}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--engines', type=Path, help='JSON list: name, argv with {input}, version, configuration, optional pin_files')
    parser.add_argument('--baseline', type=Path, default=ROOT / 'target/bulkparse-before-positioned-pdf')
    parser.add_argument('--repeats', type=int, default=7)
    parser.add_argument('--timeout', type=float, default=30)
    parser.add_argument('--case', action='append', dest='cases')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if args.repeats < 3 or args.timeout <= 0: parser.error('use at least three repeats and a positive timeout')
    engines = json.loads(args.engines.read_text()) if args.engines else [
        {'name': 'bulkparse_before', 'argv': [str(args.baseline.resolve()), parse_command(args.baseline.resolve()), '{input}'], 'version': '0.1.0', 'configuration': {'pipeline': 'native stream text; no OCR'}},
        {'name': 'bulkparse_positioned', 'argv': [str(ROOT / 'target/release/bulkparse'), parse_command(ROOT / 'target/release/bulkparse'), '{input}'], 'version': '0.1.0', 'configuration': {'pipeline': 'native positioned text; no OCR'}},
    ]
    names = set()
    for engine in engines:
        if not re.fullmatch(r'[a-zA-Z0-9_-]+', engine['name']) or engine['name'] in names: parser.error('unique safe engine names required')
        names.add(engine['name'])
        if not engine.get('version') or not engine.get('configuration'): parser.error('each engine needs an explicit version and configuration')
        executable = shutil.which(engine['argv'][0])
        if not executable: parser.error(f"executable missing: {engine['argv'][0]}")
        # Preserve virtual-environment discovery next to the symlink.
        engine['argv'][0] = str(Path(executable).absolute())
        engine['executable_sha256'] = sha(executable)
        engine['executable_bytes'] = Path(executable).stat().st_size
        engine['pin_sha256'] = {p: sha(p) for p in engine.get('pin_files', [])}
        engine['directory_sha256'] = {}
        for directory in engine.get('pin_dirs', []):
            base = Path(directory)
            if not base.is_dir(): parser.error(f'pin directory missing: {directory}')
            digest = hashlib.sha256()
            for path in sorted(base.rglob('*')):
                if path.is_file(): digest.update(path.relative_to(base).as_posix().encode() + b'\0' + sha(path).encode() + b'\0')
            engine['directory_sha256'][directory] = digest.hexdigest()
    manifest = {r['id']: r for r in map(json.loads, args.manifest.read_text().splitlines())}
    cases = [manifest[key] for key in (args.cases or DEFAULT_CASES)]
    for row in cases:
        if sha(row['source_path']) != row['source_sha256']: parser.error('source hash changed')
        if row['reference_path'] and sha(row['reference_path']) != row['reference_sha256']: parser.error('reference hash changed')
    args.out.mkdir(parents=True, exist_ok=False)
    artifacts = ROOT / 'target/benchmarks' / args.out.name
    artifacts.mkdir(parents=True, exist_ok=False)
    metadata = {'created_utc': datetime.now(timezone.utc).isoformat(), 'platform': platform.platform(),
                'python': platform.python_version(), 'repeats': args.repeats, 'warmups': 1,
                'engine_order': 'rotated each repetition', 'engines': engines,
                'manifest_sha256': sha(args.manifest), 'harness_sha256': sha(__file__),
                'rust_source_sha256': source_digest(),
                'scope': 'Sequential full CLI process runs, including startup and Markdown serialization. Repeated OS caches, not cold disk. No throughput-under-load claim. RSS is per child; no hard memory cap on macOS.',
                'cases': [{k:r[k] for k in ['id','source_sha256','reference_sha256']} for r in cases]}
    rows=[]
    for case_index,case in enumerate(cases):
        for repetition in range(-1,args.repeats):
            shift=(repetition+1)%len(engines)
            for engine in engines[shift:]+engines[:shift]:
                output=artifacts/f'{case_index}-{engine["name"]}-{repetition}.md'
                argv=[arg.replace('{input}',case['source_path']) for arg in engine['argv']]
                result=run(argv,output,args.timeout)
                if result['exit_code'] not in engine.get('success_exit_codes',[0,2]) or result['timeout']:
                    raise RuntimeError(f"Engine failed: {engine['name']} {case['id']}; see {output.with_suffix('.stderr')}")
                if repetition >= 0:
                    rows.append({'id':case['id'],'engine':engine['name'],'repetition':repetition,**result})
        print(f"Benchmarked {case['id']}",flush=True)
    summaries=[]
    for case_index,case in enumerate(cases):
        expected=reference(case)
        for engine in engines:
            selected=[r for r in rows if r['id']==case['id'] and r['engine']==engine['name']]
            if len({r['output_sha256'] for r in selected}) != 1: raise RuntimeError('nondeterministic conversion output')
            actual=(artifacts/f'{case_index}-{engine["name"]}-0.md').read_text(errors='replace')
            stats={f'median_{key}':statistics.median(r[key] for r in selected) for key in ['wall_ms','cpu_ms','peak_rss_kib']}
            stats['documents_per_second_from_median_cli_wall']=1000/stats['median_wall_ms']
            stats['max_peak_rss_kib']=max(r['peak_rss_kib'] for r in selected)
            summaries.append({'id':case['id'],'engine':engine['name'],**stats,
                              'quality':({**compare(actual,expected),'bigram_recall_proxy':bigram_recall(actual,expected)} if expected is not None else None)})
    (args.out/'run.json').write_text(json.dumps(metadata,indent=2)+'\n')
    (args.out/'samples.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
    (args.out/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
    print(json.dumps(summaries,indent=2))


if __name__ == '__main__': main()
