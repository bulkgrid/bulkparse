#!/usr/bin/env python3
"""Restore pinned holdout bytes without replacing the corpus or expected text.

Downloads public third-party inputs, copies distributed original fixtures, and
recreates PDFKit differential references on macOS. Every byte is hash-checked.
An OS/reference mismatch requires a new corpus version, not relaxed checks.
"""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from benchmark_parsers import sha

ROOT=Path(__file__).resolve().parents[1]


def local(path):
    result=(ROOT/path).resolve()
    result.relative_to(ROOT/'target/independent-inputs')
    return result


def ensure(path, expected, writer):
    if path.exists():
        if sha(path)!=expected: raise ValueError(f'existing file differs: {path}')
        return
    path.parent.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(dir=path.parent) as temp:
        staged=Path(temp)/path.name
        writer(staged)
        if sha(staged)!=expected: raise ValueError(f'restored bytes differ: {path}')
        staged.replace(path)


def main():
    rows=[json.loads(s) for s in (ROOT/'benchmarks/holdout/manifest.jsonl').read_text().splitlines()]
    downloads=json.loads((ROOT/'benchmarks/holdout/downloads.json').read_text())
    for item in downloads:
        dest=local('target/independent-inputs/external/'+item['path'])
        def download(staged,item=item):
            subprocess.run(['curl','-fsSL','--connect-timeout','10','--max-time','90',
                            item['url'],'-o',str(staged)],check=True)
        ensure(dest,item['sha256'],download)
    extractor=ROOT/'target/holdout-pdfkit'
    for row in rows:
        src=local(row['source_path']);ref=local(row['reference_path'])
        if row['reference_kind']=='authored_gold':
            for path,key in [(src,'source_sha256'),(ref,'reference_sha256')]:
                ensure(path,row[key],lambda staged,path=path:shutil.copyfile(ROOT/'fixtures/holdout'/path.name,staged))
        elif row['reference_kind']=='silver_pdfkit' and not ref.exists():
            if not extractor.exists():
                subprocess.run(['swiftc','-module-cache-path','/tmp/bulkparse-swift-cache','-Xcc',
                    '-fmodules-cache-path=/tmp/bulkparse-clang-cache',str(ROOT/'scripts/holdout_pdfkit.swift'),
                    '-o',str(extractor)],check=True)
            ensure(ref,row['reference_sha256'],lambda staged:subprocess.run(
                [str(extractor),'extract',str(src),str(staged)],check=True))
        assert sha(src)==row['source_sha256']
        assert sha(ref)==row['reference_sha256']
    print(f'Verified {len(rows)} restored input/reference pairs')


if __name__=='__main__':main()
