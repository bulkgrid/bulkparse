#!/usr/bin/env python3
"""Reproducible local host exercise with larger originals and malformed inputs.

Requires psutil. Builds are explicit; no downloads or deployment actions occur.
Each run emits per-second pool counters and sampled host+worker RSS/CPU. This is
not a quality benchmark, load-capacity promise, or an OS-enforced memory sandbox.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import time

import psutil

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def generate(directory):
    directory.mkdir(parents=True, exist_ok=False)
    paths = [ROOT / 'fixtures' / name for name in
             ['hello.docx', 'hello.md', 'table.csv', 'notebook.ipynb', 'native-and-empty.pdf']]
    originals = {
        'one-mib.txt': ('Harbor staff inspected the equipment. ' * 30000)[:1024**2],
        'ten-thousand-rows.csv': 'item,units,cost\n' + ''.join(f'item{i},{i},42\n' for i in range(10000)),
        'small.tsv': 'item\tunits\nCopper\t12\n',
        'broken.pdf': '%PDF-1.7\ntruncated document',
        'broken.docx': 'PK\x03\x04truncated archive',
        'broken.ipynb': '{"nbformat":4,"cells":[',
    }
    for name, content in originals.items():
        path = directory / name
        path.write_text(content)
        paths.append(path)
    # Original deterministic 100-page PDF with 40 text lines per page.
    count = 100
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>',
               ('<< /Type /Pages /Count %d /Kids [%s] >>' %
                (count, ' '.join(f'{4+2*i} 0 R' for i in range(count)))).encode(),
               b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>']
    for page in range(count):
        stream = (f'BT /F1 10 Tf 50 750 Td (Page {page+1} inspection record) Tj\n' +
                  ''.join(f'0 -16 Td (Equipment item {i+1} passed inspection.) Tj\n' for i in range(40)) + 'ET\n').encode()
        objects.append((f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] '
                        f'/Resources << /Font << /F1 3 0 R >> >> /Contents {5+2*page} 0 R >>').encode())
        objects.append(f'<< /Length {len(stream)} >>\nstream\n'.encode() + stream + b'endstream')
    pdf = bytearray(b'%PDF-1.4\n'); offsets = [0]
    for number, obj in enumerate(objects, 1):
        offsets.append(len(pdf)); pdf.extend(f'{number} 0 obj\n'.encode() + obj + b'\nendobj\n')
    xref = len(pdf)
    pdf.extend(f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode())
    for offset in offsets[1:]: pdf.extend(f'{offset:010d} 00000 n \n'.encode())
    pdf.extend(f'trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode())
    path = directory / 'hundred-pages.pdf'; path.write_bytes(pdf); paths.append(path)
    return paths


def exercise(binary, worker, manifest, seconds, workers, out, max_rss, min_available):
    events = out / f'workers-{workers}.jsonl'
    samples = []
    known = {}
    reason = None
    start = time.monotonic()
    with events.open('w') as log:
        process = subprocess.Popen([str(binary), str(worker), str(manifest), str(seconds), str(workers)],
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        host = psutil.Process(process.pid)
        known[host.pid] = host
        reader = events.open()
        try:
            while process.poll() is None:
                now = time.monotonic()
                # Use only PIDs emitted by our host; no system-wide process scan.
                while True:
                    position = reader.tell()
                    line = reader.readline()
                    if not line.endswith('\n'):
                        reader.seek(position); break
                    try: record = json.loads(line)
                    except ValueError: continue
                    for pid in record.get('metrics', {}).get('worker_pids', []):
                        try:
                            child = psutil.Process(pid)
                            if child.ppid() == host.pid: known[pid] = child
                        except psutil.NoSuchProcess:
                            pass
                rss = 0; cpu = 0
                for child in list(known.values()):
                    try:
                        known[child.pid] = child
                        rss += child.memory_info().rss
                        times = child.cpu_times(); cpu += times.user + times.system
                    except psutil.NoSuchProcess:
                        pass
                available = psutil.virtual_memory().available
                samples.append({'seconds':now-start, 'host_and_workers_rss_bytes':rss,
                                'live_process_cpu_seconds':cpu, 'host_available_bytes':available})
                if rss > max_rss: reason = 'rss_guard'
                elif available < min_available: reason = 'host_memory_floor'
                elif now-start > seconds+45: reason = 'exercise_timeout'
                if reason: break
                time.sleep(.02)
        except Exception as error:
            reason = f'monitor_error: {error}'
        finally:
            reader.close()
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try: process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL); process.wait()
                # Also stop a child still running after its host exited.
                try: os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError: pass
            else:
                process.wait()
    records = []
    for line in events.read_text().splitlines():
        try: records.append(json.loads(line))
        except ValueError: pass
    summaries = [r for r in records if r.get('event') == 'summary']
    surviving = []
    for pid, proc in known.items():
        try:
            if proc.is_running() and proc.status() != psutil.STATUS_ZOMBIE: surviving.append(pid)
        except psutil.NoSuchProcess:
            pass
    (out / f'memory-{workers}.jsonl').write_text(''.join(json.dumps(s)+'\n' for s in samples))
    return {'workers':workers, 'exit_code':process.returncode, 'abort_reason':reason,
            'peak_sampled_host_and_workers_rss_bytes':max((s['host_and_workers_rss_bytes'] for s in samples), default=0),
            'observed_surviving_processes':surviving, 'summary':summaries[-1] if summaries else None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--seconds', type=int, default=60)
    parser.add_argument('--workers', type=int, nargs='+', default=[1, 4])
    parser.add_argument('--max-rss-mib', type=int, default=1024)
    parser.add_argument('--min-available-mib', type=int, default=2048)
    args = parser.parse_args()
    if (not 1 <= args.seconds <= 1800 or any(n < 1 or n > 32 for n in args.workers)
            or args.max_rss_mib <= 0 or args.min_available_mib < 0):
        parser.error('invalid duration or worker count')
    args.out.mkdir(parents=True, exist_ok=False)
    raw = ROOT / 'target/pool-exercises' / args.out.name
    paths = generate(raw)
    manifest = raw / 'manifest.json'; manifest.write_text(json.dumps([str(p) for p in paths]))
    worker = ROOT / 'target/release/bulkparse-worker'
    binary = ROOT / 'target/release/examples/load'
    metadata = {'created_utc':datetime.now(timezone.utc).isoformat(), 'platform':platform.platform(),
                'logical_cpus':psutil.cpu_count(), 'memory_bytes':psutil.virtual_memory().total,
                'seconds':args.seconds, 'workers':args.workers, 'max_rss_mib':args.max_rss_mib,
                'min_available_mib':args.min_available_mib, 'sample_interval_ms':20,
                'files':[{'path':str(p.relative_to(ROOT)), 'bytes':p.stat().st_size, 'sha256':sha(p)} for p in paths],
                'fingerprints':{str(p.relative_to(ROOT)):sha(p) for p in
                    [worker, binary, Path(__file__).resolve(), ROOT/'Cargo.lock',
                     *sorted((ROOT/'crates/bulkparse-pool').rglob('*.rs'))]},
                'scope':'Local sustained host exercise, one in-flight job per configured worker. Includes startup, IPC, full JSON and Markdown. Not a competitor or quality benchmark. PIDs come from host metrics after replies; startup before first PID report may be missed. CPU samples cover currently live processes, reset on recycling; do not interpret as cumulative CPU.'}
    (args.out / 'run.json').write_text(json.dumps(metadata, indent=2)+'\n')
    results = []
    for workers in args.workers:
        print(f'Exercising {workers} workers for {args.seconds}s', flush=True)
        result = exercise(binary, worker, manifest, args.seconds, workers, args.out,
                          args.max_rss_mib*1024**2, args.min_available_mib*1024**2)
        results.append(result)
        (args.out / 'summary.json').write_text(json.dumps(results, indent=2)+'\n')
        print(json.dumps(result), flush=True)
        if result['exit_code'] or result['abort_reason'] or result['observed_surviving_processes'] or not result['summary']:
            raise RuntimeError('exercise did not finish cleanly')


if __name__ == '__main__':
    main()
