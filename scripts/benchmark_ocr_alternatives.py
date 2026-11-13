#!/usr/bin/env python3
"""Run isolated, persistent Linux CPU OCR workers against frozen image inputs."""
import argparse
import hashlib
import json
import queue
import random
import statistics
import subprocess
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

from evaluate_ocr import distance, normalized

ROOT = Path(__file__).resolve().parents[1]
PROFILES = {
    'rapid-tiny': ('rapid', 'tiny', 'python'),
    'rapid-small': ('rapid', 'small', 'python'),
    'rapid-medium': ('rapid', 'medium', 'python'),
    'rapid-latin': ('rapid', 'latin', 'python'),
    'lite-tiny': ('lite', 'tiny', '/opt/lite/bin/python'),
    'lite-small': ('lite', 'small', '/opt/lite/bin/python'),
    'tesseract-fast': ('tesseract', 'fast', '/opt/tesseract/bin/python'),
    'tesseract-best': ('tesseract', 'best', '/opt/tesseract/bin/python'),
    'paddle-tiny': ('paddle', 'tiny', '/opt/paddle/bin/python'),
    'paddle-small': ('paddle', 'small', '/opt/paddle/bin/python'),
}


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def score(actual, reference):
    a, r = normalized(actual), normalized(reference)
    return {'exact': a == r, 'character_errors': distance(r, a),
            'reference_characters': len(r), 'word_errors': distance(r.split(), a.split()),
            'reference_words': len(r.split()), 'blank_false_positive': not r and bool(a)}


def aggregate(scores):
    fields = ['character_errors', 'reference_characters', 'word_errors', 'reference_words']
    result = {k: sum(s[k] for s in scores) for k in fields}
    result.update(cases=len(scores), exact=sum(s['exact'] for s in scores),
                  blank_false_positives=sum(s['blank_false_positive'] for s in scores))
    for unit in ['character', 'word']:
        denominator = result['reference_' + unit + 's']
        result[unit + '_error_rate'] = result[unit + '_errors'] / denominator if denominator else None
    return result


def run_profile(name, cases, data, output, passes):
    engine, size, python = PROFILES[name]
    image = f"bulkparse-ocr-{'paddle' if engine == 'paddle' else 'alternatives'}:2026-11-11"
    container_name = 'bulkparse-ocr-bench-' + name
    command = ['docker', 'run', '--rm', '-i', '--name', container_name,
               '--network', 'none', '--cpus', '1', '--memory', '4g', '--pids-limit', '256',
               '-e', 'PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK=True',
               '-e', 'PPOCR_BENCH_THREADS=1',
               '-v', f'{data}:/data:ro',
               '-v', f'{ROOT}/target/ocr-alternatives/paddle-cache:/root/.paddlex:ro',
               '-v', f'{ROOT}/scripts/ocr_alternatives_worker.py:/bench/worker.py:ro',
               image, python, 'worker.py', '--engine', engine, '--size', size]
    folder = output / name
    folder.mkdir()
    (folder / 'command.json').write_text(json.dumps(command, indent=2) + '\n')
    q = queue.Queue()
    results = []
    with (folder / 'stderr.log').open('w') as err, (folder / 'stdout.jsonl').open('w') as raw:
        launch = time.perf_counter()
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=err, text=True, bufsize=1)
        def read():
            for line in process.stdout:
                raw.write(line)
                raw.flush()
                q.put(line)
            q.put(None)
        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        def receive():
            while True:
                line = q.get(timeout=240)
                if line is None:
                    raise RuntimeError('Worker exited; see stderr.log')
                try:
                    value = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if isinstance(value, dict) and ('ready' in value or 'id' in value):
                    return value
        try:
            ready = receive()
            assert ready.get('ready'), ready
            ready['container_launch_to_ready_seconds'] = time.perf_counter() - launch
            (folder / 'startup.json').write_text(json.dumps(ready, indent=2) + '\n')
            with (folder / 'records.jsonl').open('w') as records:
                for run in range(passes + 1):
                    rows = []
                    for case in cases:
                        request = {'id': case['id'], 'images': [im['path'] for im in case['images']]}
                        process.stdin.write(json.dumps(request) + '\n')
                        process.stdin.flush()
                        result = receive()
                        assert result.get('id') == case['id'], result
                        assert len(result['pages']) == len(case['images']), result
                        actual = '\n'.join(page['text'] for page in result['pages'])
                        result.update(run=run, warmup=run == 0, quality=score(actual, case['reference_text']),
                                      text_sha256=hashlib.sha256(actual.encode()).hexdigest())
                        records.write(json.dumps(result, ensure_ascii=False) + '\n')
                        records.flush()
                        rows.append(result)
                    results.append(rows)
                    print(f"{name} {'warmup' if run == 0 else 'pass '+str(run)}: "
                          f"{sum(r['wall_seconds'] for r in rows):.3f}s; "
                          f"{sum(bool(r['errors']) for r in rows)} failed documents", flush=True)
            process.stdin.close()
            code = process.wait(timeout=30)
            reader.join(timeout=5)
            assert code == 0, code
        except BaseException:
            subprocess.run(['docker', 'stop', '-t', '1', container_name], capture_output=True, timeout=30)
            process.wait(timeout=30)
            reader.join(timeout=5)
            raise
    measured = results[1:]
    first = measured[0]
    summary = {
        'profile': name, 'startup': ready,
        'full_corpus_wall_seconds': [sum(r['wall_seconds'] for r in rows) for rows in measured],
        'full_corpus_cpu_seconds': [sum(r['cpu_seconds'] for r in rows) for rows in measured],
        'peak_rss_bytes': max(r['peak_rss_bytes'] for rows in results for r in rows),
        'quality': aggregate([r['quality'] for r in first]),
        'by_track': {track: aggregate([r['quality'] for c, r in zip(cases, first) if c['track'] == track])
                     for track in sorted({c['track'] for c in cases})},
        'by_language': {lang: aggregate([r['quality'] for c, r in zip(cases, first) if c['language'] == lang])
                        for lang in sorted({c['language'] for c in cases})},
        'error_documents_first_pass': sum(bool(r['errors']) for r in first),
        'changed_text_across_runs': [c['id'] for i, c in enumerate(cases)
                                    if len({rows[i]['text_sha256'] for rows in results}) > 1],
    }
    summary['median_wall_seconds'] = statistics.median(summary['full_corpus_wall_seconds'])
    summary['median_cpu_seconds'] = statistics.median(summary['full_corpus_cpu_seconds'])
    (folder / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    return summary


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--data', type=Path, default=ROOT / 'target/ocr-alternatives/data')
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--profiles', nargs='+', choices=list(PROFILES), default=list(PROFILES))
    ap.add_argument('--passes', type=int, default=5)
    ap.add_argument('--smoke', action='store_true')
    a = ap.parse_args()
    assert a.passes > 0
    a.out.mkdir(parents=True, exist_ok=False)
    cases = json.loads((a.data / 'cases.json').read_text())
    for asset in json.loads((ROOT / 'benchmarks/ocr-alternatives/assets.json').read_text()):
        assert sha(a.data / asset['path']) == asset['sha256'], asset['path']
    for model in json.loads((ROOT / 'benchmarks/ocr-alternatives/paddle-assets.json').read_text()):
        for path, expected in model['files'].items():
            assert sha(ROOT / 'target/ocr-alternatives/paddle-cache' / path) == expected, path
    for c in cases:
        assert hashlib.sha256(c['reference_text'].encode()).hexdigest() == c['reference_sha256']
        for im in c['images']:
            assert sha(a.data / im['path']) == im['sha256']
    if a.smoke:
        cases = [c for c in cases if c['id'] in ['ocr-blank', 'english-clean', 'chinese-clean', 'swedish-a-clean']]
        assert len(cases) >= 3
    order = list(a.profiles)
    random.Random(20261111).shuffle(order)
    manifest = {'created_utc': datetime.now(timezone.utc).isoformat(), 'profiles_order': order,
                'passes': a.passes, 'smoke': a.smoke, 'cases': cases,
                'source_sha256': {str(p.relative_to(ROOT)): sha(p) for p in [Path(__file__),
                                 ROOT / 'scripts/ocr_alternatives_worker.py', ROOT / 'scripts/evaluate_ocr.py',
                                 ROOT / 'benchmarks/ocr-alternatives/protocol.json',
                                 ROOT / 'benchmarks/ocr-alternatives/assets.json',
                                 ROOT / 'benchmarks/ocr-alternatives/paddle-assets.json']},
                'assets': {str(p.relative_to(a.data)): sha(p) for p in sorted(a.data.rglob('*')) if p.is_file()},
                'paddle_assets': {str(p.relative_to(ROOT / 'target/ocr-alternatives/paddle-cache')): sha(p)
                                  for p in sorted((ROOT / 'target/ocr-alternatives/paddle-cache').rglob('*'))
                                  if p.is_file() and p.name != '.lock'},
                'docker_images': json.loads(subprocess.check_output(['docker', 'image', 'inspect',
                   'bulkparse-ocr-alternatives:2026-11-11', 'bulkparse-ocr-paddle:2026-11-11'], text=True))}
    (a.out / 'run.json').write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + '\n')
    summaries = []
    for name in order:
        try:
            summaries.append(run_profile(name, cases, a.data.resolve(), a.out, a.passes))
        except Exception as error:
            failure = {'profile': name, 'failed': True, 'error': str(error)}
            (a.out / name / 'failed.json').write_text(json.dumps(failure, indent=2) + '\n')
            summaries.append(failure)
            print(f'{name}: FAILED: {error}', flush=True)
        (a.out / 'summary.json').write_text(json.dumps(summaries, indent=2) + '\n')


if __name__ == '__main__':
    main()
