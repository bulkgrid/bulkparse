#!/usr/bin/env python3
"""Verify complete accounting and recompute metrics; poor OCR remains a result."""
import hashlib
import json
from pathlib import Path
import sys
from evaluate_ocr import text
from evaluate_ocr_profiles import aggregate,score

ROOT=Path(__file__).resolve().parents[1]


def check(report):
    run=json.loads((report/'run.json').read_text())
    manifest=ROOT/'benchmarks/ocr-profiles/manifest.json'
    assert hashlib.sha256(manifest.read_bytes()).hexdigest()==run['manifest_sha256']
    cases={c['id']:c for c in json.loads(manifest.read_text())}
    rows=[json.loads(line) for line in (report/'samples.jsonl').read_text().splitlines()]
    ids={r['id'] for r in rows}
    originals={c['id'] for c in cases.values() if c['track']!='external_smoke'}
    assert ids in (originals,set(cases)), 'incomplete or selectively reported corpus'
    expected={(p,c,i) for p in run['profile_settings'] for c in ids for i in range(run['repeats'])}
    actual={(r['profile'],r['id'],r['repeat']) for r in rows}
    assert len(rows)==len(expected) and actual==expected,'missing or duplicate attempts'
    assert all(p['passed'] for p in json.loads((report/'preflight.json').read_text()))
    for row in rows:
        case=cases[row['id']]
        assert row['track']==case['track'] and row['language']==case['language']
        assert row['harness_error'] is None and row['exit_code']==2
        assert row['status']=='partial' and 'pdf_ocr_error' not in row['diagnostic_codes']
        assert row['observed_backend_processes']>=1,'OCR RSS was not observed'
        if case['track']=='external_smoke':
            assert row['quality'] is None
            assert not (report/'outputs'/row['profile']/f"{row['id']}-{row['repeat']}.json").exists()
            continue
        ref=ROOT/case['reference'];source=ROOT/case['path']
        assert hashlib.sha256(ref.read_bytes()).hexdigest()==case['reference_sha256']
        assert hashlib.sha256(source.read_bytes()).hexdigest()==case['sha256']
        payload=(report/'outputs'/row['profile']/f"{row['id']}-{row['repeat']}.json").read_bytes()
        assert hashlib.sha256(payload).hexdigest()==row['output_sha256']
        value=text(json.loads(payload))
        assert hashlib.sha256(value.encode()).hexdigest()==row['text_sha256']
        assert score(value,ref.read_text())==row['quality']
    assert aggregate(rows)==json.loads((report/'summary.json').read_text())
    return len(rows)


if __name__=='__main__':
    print(f'Accounting and metrics verified for {check(Path(sys.argv[1]))} attempts; this is not a quality pass rate.')
