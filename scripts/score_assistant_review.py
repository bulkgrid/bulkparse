#!/usr/bin/env python3
"""Validate and freeze same-chat assessments before revealing converter identities."""
import argparse
import collections
import datetime
import hashlib
import json
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / 'reports/realworld-2026-11-12'
PACKETS = ROOT / 'target/realworld/assistant-review'
DIMS = ['completeness', 'structure', 'formatting', 'cleanliness']


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read(path):
    return json.loads(path.read_text())


def write(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def validate(root, index, assessment_dir):
    """This phase deliberately has no access to the identity mapping."""
    reviewed = {}
    files = {}
    for entry in index:
        doc = entry['id']
        path = assessment_dir / (doc + '.json')
        if not path.exists():
            raise ValueError('Missing assessment: ' + doc)
        assessment = read(path)
        packet_path = root / entry['packet']
        packet = read(packet_path)
        if assessment['document_id'] != doc or assessment['format'] != entry['format']:
            raise ValueError('Document identity mismatch: ' + doc)
        if assessment['packet_sha256'] != sha(packet_path):
            raise ValueError('Packet changed: ' + doc)
        if assessment['reference'] != packet['reference']:
            raise ValueError('Reference metadata changed: ' + doc)
        for image in packet['reference']['images']:
            if sha(root / image['path']) != image['sha256']:
                raise ValueError('Reference image changed: ' + doc)
        outputs = {o['label']: o for o in packet['outputs']}
        if set(outputs) != set(assessment['assessments']):
            raise ValueError('Missing or extra candidate: ' + doc)
        for label, a in assessment['assessments'].items():
            o = outputs[label]
            actual = sha(root / o['path'])
            if actual != o['sha256'] or actual != a['output_sha256']:
                raise ValueError('Output changed: ' + doc + '/' + label)
            scores = a['scores']
            if scores is None:
                if a['confidence'] != 'unreviewable':
                    raise ValueError('Null score needs unreviewable designation')
            else:
                if len(scores) != 4 or any(type(v) is not int or not 0 <= v <= 100 for v in scores):
                    raise ValueError('Invalid dimensions: ' + doc)
                if (root / o['path']).stat().st_size == 0 and any(scores):
                    raise ValueError('Empty output received credit: ' + doc)
                if a['dimensions'] != dict(zip(DIMS, scores)):
                    raise ValueError('Dimension mismatch: ' + doc)
            if len(a.get('evidence', '')) < 20:
                raise ValueError('Missing concrete evidence: ' + doc)
        reviewed[doc] = assessment
        files[doc] = sha(path)
    return reviewed, files


def aggregate(reviewed, mapping):
    expected = {(doc, label) for doc, r in reviewed.items() for label in r['assessments']}
    selected = [m for m in mapping if m['document_id'] in reviewed]
    keys = [(m['document_id'], m['label']) for m in selected]
    if len(keys) != len(set(keys)) or set(keys) != expected:
        raise ValueError('Identity mapping does not exactly cover reviewed candidates')
    rows = []
    for m in selected:
        r = reviewed[m['document_id']]
        a = r['assessments'][m['label']]
        if m['output_sha256'] != a['output_sha256']:
            raise ValueError('Identity mapping hash mismatch')
        if not m['ok'] and a['scores'] is not None and any(a['scores']):
            raise ValueError('Failed conversion received credit')
        rows.append({'document_id': m['document_id'], 'format': r['format'],
                     'engine': m['engine'], 'anonymous_label': m['label'],
                     'conversion_returned': m['ok'], 'scope': r['scope'],
                     **a, 'score': statistics.mean(a['scores']) if a['scores'] is not None else None})
    engines = {}
    for engine in sorted({r['engine'] for r in rows}):
        all_rows = [r for r in rows if r['engine'] == engine]
        valid = [r for r in all_rows if r['score'] is not None]
        formats = {}
        for fmt in sorted({r['format'] for r in valid}):
            subset = [r for r in valid if r['format'] == fmt]
            formats[fmt] = {'documents': len(subset), 'score': statistics.mean(r['score'] for r in subset),
                            **{d: statistics.mean(r['dimensions'][d] for r in subset) for d in DIMS}}
        engines[engine] = {'documents_reviewed': len(all_rows), 'documents_scored': len(valid),
                           'unreviewable': len(all_rows) - len(valid),
                           'failed_conversions': sum(not r['conversion_returned'] for r in all_rows),
                           'formats': formats,
                           **{d: statistics.mean(f[d] for f in formats.values()) if formats else None
                              for d in ['score', *DIMS]}}
    return {'engines': engines, 'documents': rows}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--selection', type=Path, help='Optional frozen sample JSON with document_ids; otherwise all inputs')
    ap.add_argument('--validate-only', action='store_true', help='Never read identities or generate scores')
    args = ap.parse_args()
    index = read(PACKETS / 'index.json')
    selection = None
    if args.selection:
        selection = read(args.selection)
        ids = selection['document_ids']
        if len(ids) != len(set(ids)) or not set(ids).issubset({r['id'] for r in index}):
            raise ValueError('Invalid selected document IDs')
        index = [r for r in index if r['id'] in ids]
    out = REPORT / 'assistant-review'
    reviewed, hashes = validate(ROOT, index, out / 'assessments')
    if args.validate_only:
        print(f'Validated {len(reviewed)} documents; converter identities not read')
        return
    # Freeze the exact assessment set BEFORE reading the identity mapping.
    freeze_path = out / 'assessment-freeze.json'
    frozen = {'method': 'Same-chat assistant assessment; not independent blind judging',
              'assessment_sha256': hashes, 'selection': selection,
              'documents': len(reviewed), 'candidates': sum(len(r['assessments']) for r in reviewed.values())}
    if freeze_path.exists():
        previous = read(freeze_path)
        if any(previous.get(k) != v for k, v in frozen.items()):
            raise ValueError('Frozen assessment set changed; do not silently replace published scores')
    else:
        write(freeze_path, {**frozen, 'frozen_utc': datetime.datetime.now(datetime.timezone.utc).isoformat()})
    mapping_path = PACKETS / 'private-label-map.json'
    quality = aggregate(reviewed, read(mapping_path)['mapping'])
    quality.update({'method': frozen['method'], 'selection': selection,
                    'assessment_freeze_sha256': sha(freeze_path), 'mapping_sha256': sha(mapping_path),
                    'documents_reviewed': len(reviewed), 'candidate_assessments': frozen['candidates'],
                    'format_counts': dict(collections.Counter(r['format'] for r in reviewed.values())),
                    'aggregation': 'Mean four dimensions per document, mean documents per format, mean supported formats. Failures zero; unsupported formats excluded.'})
    write(out / 'quality.json', quality)
    print(f'Frozen and aggregated {len(reviewed)} documents; identities now revealed in quality.json')


if __name__ == '__main__':
    main()
