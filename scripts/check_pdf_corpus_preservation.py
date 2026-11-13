#!/usr/bin/env python3
"""Check per-page native characters and inspection on a supplied PDF directory.

This allows new structure and reading order. It is a preservation check, not a
semantic accuracy score. Unicode normalization is disabled for this check only.
"""
import argparse
import hashlib
import json
from pathlib import Path

from check_pdf_quality_fix import convert, page_characters


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--pdf-dir', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if args.out.exists():
        raise ValueError('Use a fresh output path')
    files = sorted(args.pdf_dir.glob('*.pdf'))
    if not files:
        raise ValueError('No PDF inputs')
    records = []
    for path in files:
        old_code, old = convert(args.before, path, True)
        new_code, new = convert(args.after, path, True)
        row = {'id': path.stem, 'sha256': sha(path),
               'before_exit': old_code, 'after_exit': new_code}
        if old_code in (0, 2):
            row['characters_preserved'] = new is not None and page_characters(old['document']) == page_characters(new['document'])
            row['inspection_unchanged'] = new is not None and old['pdf_inspection'] == new['pdf_inspection']
            row['passed'] = new_code == old_code and row['characters_preserved'] and row['inspection_unchanged']
        else:
            row['expected_failure'] = True
            row['passed'] = new_code == old_code
        records.append(row)
    passed = all(row['passed'] for row in records)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps({'passed': passed, 'scope': __doc__,
        'before_binary_sha256': sha(args.before), 'after_binary_sha256': sha(args.after),
        'harness_sha256': sha(Path(__file__)), 'pdf_cases': records}, indent=2) + '\n')
    print(json.dumps({'passed': passed, 'pdf_count': len(records),
                      'failures': [row for row in records if not row['passed']]}, indent=2))
    raise SystemExit(0 if passed else 1)


if __name__ == '__main__':
    main()
