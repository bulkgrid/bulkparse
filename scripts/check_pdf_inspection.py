#!/usr/bin/env python3
"""Verify shared inspection routes and preserve existing PDF native outputs.

Uses the existing hash-pinned upstream corpus, not new OCR accuracy gold.
No external document text is copied into the report.
"""
from bulkparse_cli import parse_command
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]

def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def run(binary, command, path):
    result = subprocess.run([str(binary), command, str(path), *(['--to','json'] if command==parse_command(binary) else [])], capture_output=True, timeout=45)
    return result.returncode, json.loads(result.stdout) if result.stdout else None

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT/'target/debug/bulkparse')
    parser.add_argument('--baseline', type=Path, default=ROOT/'reports/pdf-tables-corpus-2026-11-10-v2')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    baseline = {r['id']:r for r in map(json.loads,(args.baseline/'results.jsonl').read_text().splitlines())}
    manifest = list(map(json.loads,(args.baseline/'manifest.jsonl').read_text().splitlines()))
    rows = []
    for entry in manifest:
        if entry['format'] != 'pdf': continue
        source = Path(entry['source_path'])
        assert sha(source) == entry['source_sha256'], entry['id']
        code, output = run(args.binary, parse_command(args.binary), source)
        inspect_code, inspected = run(args.binary, 'inspect', source)
        old = baseline[entry['id']]
        record = {'id':entry['id'], 'sha256':entry['source_sha256'], 'convert_exit':code,'inspect_exit':inspect_code}
        if old['outcome'] == 'failed':
            record['expected_failure'] = code == old['json_run']['exit_code'] and inspect_code == 1
        else:
            previous = json.loads(Path(old['json_run']['stdout_path']).read_text())
            record['native_blocks_unchanged'] = output is not None and output['document']['blocks'] == previous['document']['blocks']
            record['inspection_matches_conversion'] = output is not None and inspected == output['pdf_inspection']
            record['pages'] = inspected['page_count'] if inspected else None
            record['pages_needing_ocr'] = inspected['pages_needing_ocr'] if inspected else None
        rows.append(record)
    passed = all(r.get('expected_failure', r.get('native_blocks_unchanged',False) and r.get('inspection_matches_conversion',False)) for r in rows)
    report = {'passed':passed,'cases':len(rows), 'binary_sha256':sha(args.binary),
              'source_sha256':{str(p.relative_to(ROOT)):sha(p) for p in sorted((ROOT/'crates/bulkparse/src').rglob('*.rs'))},
              'scope':'Native block regression and shared-route parity, not OCR recognition accuracy or classifier precision/recall.', 'results':rows}
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'passed':passed,'cases':len(rows),'failures':[r for r in rows if not r.get('expected_failure',r.get('native_blocks_unchanged',False) and r.get('inspection_matches_conversion',False))]},indent=2))
    raise SystemExit(0 if passed else 1)

if __name__ == '__main__': main()
