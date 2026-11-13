#!/usr/bin/env python3
"""Validate source hashes, saved images and unchanged Markdown on the measured corpus."""
import argparse, hashlib, json, re, tempfile, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--benchmark', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    from bulkparse import parse_file_to_markdown
    rows = json.loads((args.benchmark/'bulkparse-round-1.json').read_text())['documents']
    corpus = {r['id']: r for r in json.loads((args.benchmark/'manifest.json').read_text())}
    checked = []
    for row in rows:
        if not row.get('image_count'): continue
        source = ROOT / corpus[row['id']]['path']
        expected = {a['id']: a for a in row['image_assets']}
        source_checks = 0
        if row["format"] in {"docx", "xlsx", "xlsm", "pptx", "odt", "ods", "odp", "epub", "xlsb"}:
            with zipfile.ZipFile(source) as package:
                for asset in expected.values():
                    original = package.read(asset['source']['part'])
                    assert hashlib.sha256(original).hexdigest() == asset['id'].split('.')[0]
                    assert len(original) == asset['bytes']
                    source_checks += 1
        with tempfile.TemporaryDirectory(prefix='bulkparse-export-audit-') as tmp:
            output = Path(tmp)/'document.md'
            summary = parse_file_to_markdown(source, output, export_images=True)
            assert {a['id'] for a in summary['images']} == set(expected)
            linked = set()
            tail = ''
            with output.open() as file:
                while chunk := file.read(65536):
                    text = tail + chunk
                    linked.update(re.findall(r'bulkparse-images-[A-Za-z0-9]+/([a-f0-9]{64}\.[a-z0-9]+)', text))
                    tail = text[-256:]
            assert not linked, (row['id'], linked)
            with output.open('rb') as stream:
                assert hashlib.file_digest(stream, 'sha256').hexdigest() == row.get('output_sha256',row.get('markdown_sha256'))
            for image in summary['images']:
                file = Path(tmp)/image['path']
                assert file.resolve().is_relative_to(Path(tmp).resolve())
                with file.open('rb') as stream:
                    assert hashlib.file_digest(stream, 'sha256').hexdigest() == image['id'].split('.')[0]
                assert file.stat().st_size == expected[image['id']]['bytes']
            checked.append(dict(id=row['id'], images=len(expected), source_hashes_checked=source_checks, markdown_unchanged=True, no_generated_image_links=True))
    report = dict(documents=len(checked), exported_images=sum(r['images'] for r in checked), source_package_images_verified=sum(r['source_hashes_checked'] for r in checked), checks=checked,
        limitations='Package hashes independently checked against source bytes. Legacy assets verified across structured output and file export, not an exhaustive visual source audit. No image understanding quality score.')
    args.out.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k!='checks'}, indent=2))

if __name__ == '__main__': main()
