#!/usr/bin/env python3
"""Restore and rerun the frozen study in a fresh extracted evidence bundle."""
from pathlib import Path
import hashlib
import json
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]

def main():
    out = ROOT/'reports/independent-pdf-v2-2026-11-11'
    if (out/'quality.json').exists() or (out/'timing.json').exists():
        raise SystemExit('Use a fresh extracted bundle: this command will not replace existing results.')
    freeze = json.loads((ROOT/'benchmarks/independent-pdf-v2/freeze.json').read_text())
    for version in freeze['versions'].values():
        source = ROOT/'reports'/version['report']/'source-snapshot'
        for name, digest in version['source_manifest'].items():
            assert hashlib.sha256((source/name).read_bytes()).hexdigest() == digest
    def run(*args):
        subprocess.run(list(map(str,args)),cwd=ROOT,check=True)
    run(sys.executable,ROOT/'scripts/doclaynet_sample.py','--out',ROOT/'target/independent-pdf-v2')
    run(sys.executable,ROOT/'scripts/select_doclaynet_holdout.py')
    for label, version in freeze['versions'].items():
        source = ROOT/'reports'/version['report']/'source-snapshot'
        target = ROOT/'target/independent-pdf-v2'/('build-'+label)
        run('cargo','build','--locked','--release','--manifest-path',source/'Cargo.toml','--target-dir',target,'-p','bulkparse-cli')
        run('cargo','build','--locked','--release','--manifest-path',source/'benchmarks/opendataloader/Cargo.toml','--target-dir',target)
    run(sys.executable,ROOT/'scripts/evaluate_independent_pdf.py','--out',out)
    run(sys.executable,ROOT/'scripts/time_independent_pdf.py')
    print('Rerun quality and timing saved to',out)

if __name__ == '__main__':
    main()
