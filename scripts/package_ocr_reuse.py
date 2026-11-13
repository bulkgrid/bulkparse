#!/usr/bin/env python3
"""Package verified OCR integration results, source and authored fixtures."""
import hashlib
import json
from pathlib import Path
import zipfile

ROOT=Path(__file__).resolve().parents[1]
REPORT=ROOT/'reports/ocr-reuse-2026-11-11'
DEST=REPORT.with_suffix('.zip')

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    assert json.loads((REPORT/'validation.json').read_text())['passed']
    paths=set()
    def add(path):
        path=ROOT/path
        assert path.exists(),path
        for p in path.rglob('*') if path.is_dir() else [path]:
            if p.is_file() and '__pycache__' not in p.parts and p.suffix not in ['.pyc','.zip','.onnx']:
                paths.add(p)
    for name in ['reports/ocr-reuse-2026-11-11','reports/opendataloader-ocr-reuse-2026-11-11',
                 'reports/ocr-alternatives-2026-11-11/build','backends/rapidocr','crates/bulkparse-ocr',
                 'benchmarks/ocr-profiles','fixtures/ocr-fine-print','docs/pdf-ocr.md','docs/ocr-profiles.md',
                 'docs/ocr-layout.md','README.md','Cargo.toml','Cargo.lock','LICENSE','THIRD_PARTY_NOTICES.md']:
        add(name)
    for name in ['benchmark_ocr_reuse','report_ocr_reuse','prepare_ocr_reuse_benchmark','package_ocr_reuse',
                 'test_ocr_worker','test_ocr_profiles','benchmark_ocr_alternatives','ocr_alternatives_worker',
                 'evaluate_ocr','benchmark_opendataloader','check_opendataloader_results','test_benchmark_opendataloader',
                 'provision_ocr_alternatives']:
        add('scripts/'+name+'.py')
    add('scripts/holdout_pdfkit.swift')
    # Include the complete native workspace inputs at their working-tree paths.
    native=ROOT/'reports/opendataloader-ocr-reuse-2026-11-11'
    for name in json.loads((native/'source-manifest.json').read_text()):add(name)
    for c in json.loads((REPORT/'linux/run.json').read_text())['cases']:
        add(c['path']);add(c['reference'])
    manifest_path=REPORT/'bundle-manifest.json';paths.discard(manifest_path)
    manifest={str(p.relative_to(ROOT)):sha(p) for p in sorted(paths)}
    manifest_path.write_text(json.dumps({'files':manifest,'model_weights_included':False,
        'third_party_corpus_pdfs_included':False,'authored_ocr_fixtures_included':True},indent=2)+'\n')
    paths.add(manifest_path)
    with zipfile.ZipFile(DEST,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as archive:
        for p in sorted(paths):archive.write(p,str(p.relative_to(ROOT)))
    with zipfile.ZipFile(DEST) as archive:
        assert archive.testzip() is None
        for name,digest in manifest.items():assert hashlib.sha256(archive.read(name)).hexdigest()==digest,name
    DEST.with_suffix('.zip.sha256').write_text(sha(DEST)+'  '+DEST.name+'\n')
    print(json.dumps({'path':str(DEST),'bytes':DEST.stat().st_size,'files':len(paths),'sha256':sha(DEST),'verified':True},indent=2))

if __name__=='__main__':main()
