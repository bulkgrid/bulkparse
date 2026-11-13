#!/usr/bin/env python3
"""Diagnostic: interleave before/after per document, reversing order on alternating inputs."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
from benchmark_realworld import ROOT, REPORT, Worker, digest, write


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--before', type=Path, required=True)
    ap.add_argument('--after', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--allow-content-changes', action='store_true', help='Retain differences for a separate source-backed quality review instead of asserting identical extraction')
    a = ap.parse_args()
    a.out.mkdir(parents=True, exist_ok=False)
    corpus = json.loads((REPORT/'corpus-manifest.json').read_text())
    assert len(corpus) == 112
    assert all(digest(ROOT/d['path']) == d['sha256'] for d in corpus)
    commands = {k:[str(p.resolve())] for k,p in [('before',a.before),('after',a.after)]}
    hashes = {k:digest(Path(v[0])) for k,v in commands.items()}
    env = dict(os.environ, RAYON_NUM_THREADS='1', OMP_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1', MKL_NUM_THREADS='1')
    workers = {k:Worker(v,a.out/(k+'.log'),env) for k,v in commands.items()}
    rows = {k:[] for k in workers}
    try:
        for index,d in enumerate(corpus):
            order = ['before','after'] if index % 2 == 0 else ['after','before']
            for k in order:
                r = workers[k].call(ROOT/d['path'])
                r['output_sha256'] = hashlib.sha256(r.pop('markdown').encode()).hexdigest()
                r.update(id=d['id'],format=d['format'],order=order)
                rows[k].append(r)
    finally:
        codes = {k:w.close() for k,w in workers.items()}
    fields = ['ok','status','diagnostics','render_diagnostics','error','output_sha256']
    differences = [{"id":b['id'],"fields":[k for k in fields if b.get(k)!=a.get(k)]} for b,a in zip(rows['before'],rows['after'])]
    differences = [d for d in differences if d['fields']]
    profiles = {}
    for k,rs in rows.items():
        write(a.out/(k+'.json'),dict(documents=rs))
        rss = re.findall(r'(\d+)\s+maximum resident set size',(a.out/(k+'.log')).read_text())
        profiles[k] = dict(returned=sum(r['ok'] for r in rs),seconds=sum(r['elapsed_ns'] for r in rs)/1e9,
            cpu_seconds=sum(r['cpu_seconds'] for r in rs),peak_rss_bytes=int(rss[-1]),exit_code=codes[k])
    assert all(digest(Path(commands[k][0]))==h for k,h in hashes.items())
    write(a.out/'summary.json',dict(method=__doc__,commands=commands,binary_sha256=hashes,
        scope='One pass per persistent worker; no warm-up; interleaved inputs for drift control. This is not the sequential whole-corpus competitor timing protocol.',
        profiles=profiles,differences=differences,allow_content_changes=a.allow_content_changes))
    assert (a.allow_content_changes or not differences) and all(p['returned']==112 and p['exit_code']==0 for p in profiles.values())
    print(json.dumps(profiles,indent=2))


if __name__ == '__main__':
    main()
