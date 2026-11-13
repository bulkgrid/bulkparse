#!/usr/bin/env python3
"""Package final source and evidence without upstream corpora or binaries."""
from pathlib import Path
import hashlib,json,shutil,zipfile
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'reports/multiformat-tables-layout-2026-11-12'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    files=set()
    for group in ['crates','bindings','backends']:
        for p in (ROOT/group).rglob('*'):
            if p.is_file() and p.suffix in {'.rs','.toml','.py','.js','.mjs','.cjs','.ts','.md','.json'} and not any(x in p.parts for x in ['node_modules','__pycache__','pkg','target']):files.add(p)
    files.update(ROOT/p for p in ['Cargo.toml','Cargo.lock','LICENSE','README.md','THIRD_PARTY_NOTICES.md','docs/office-formats.md','docs/validation.md','docs/roadmap.md'])
    files.update((ROOT/'benchmarks/multiformat').rglob('*'))
    files={p for p in files if p.is_file()}
    for name in ['benchmark_office_formats.py','generate_office_benchmark.py','worker_docling_office.py','report_office_formats.py','validate_office_benchmark.py','package_office_formats.py','package_office_quality.py','evaluate_office_quality.py','report_office_quality.py','report_legacy_office_quality.py','package_legacy_office_quality.py','benchmark_same_machine.py','benchmark_opendataloader.py','check_binding_parity.mjs','check_office_pdf_regression.py','define_table_layout_checks.py','audit_table_layout_outputs.py','report_table_layout_quality.py','package_table_layout_quality.py']:files.add(ROOT/'scripts'/name)
    files.update(p for p in (ROOT/'fixtures/office-authored').iterdir() if p.is_file())
    for name in ['native-and-empty.pdf','hello.docx','hello.md','notebook.ipynb','table.csv','holdout/scan-only.pdf','holdout/native-and-scan.pdf','inspection/header-over-scan.pdf']:files.add(ROOT/'fixtures'/name)
    snapshot=OUT/'source-snapshot';snapshot.mkdir(exist_ok=True)
    for p in sorted(files):dest=snapshot/p.relative_to(ROOT);dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
    (OUT/'package-source-manifest.json').write_text(json.dumps({str(p.relative_to(ROOT)):sha(p) for p in sorted(files)},indent=2)+'\n')
    evidence=[p for p in OUT.iterdir() if p.is_file() and not p.name.startswith('development')]
    baseline=ROOT/'reports/multiformat-2026-11-11'
    evidence.extend(baseline/name for name in ['README.md','corpus-manifest.json','upstream-source-hashes.json','benchmark-final/bulkparse-pass-1.json','benchmark-final/anydoc-pass-1.json','benchmark-final/summary.json','benchmark-final/manifest.json'])
    previous=ROOT/'reports/multiformat-legacy-quality-2026-11-12'
    evidence.extend(previous/name for name in ['README.md','quality-checks.json','quality-results.json','benchmark-final/bulkparse-pass-1.json','benchmark-final/anydoc-pass-1.json','benchmark-final/summary.json','benchmark-final/manifest.json','benchmark-final/environment.json'])
    for folder in ['benchmark-final','validation','before-source']:evidence.extend(p for p in (OUT/folder).rglob('*') if p.is_file())
    pdf=ROOT/'reports/same-machine-ocr-table-alignment-2026-11-11'
    evidence.append(pdf/'corpus-manifest.json')
    evidence.extend(p for p in (pdf/'prediction/bulkparse/round-0/markdown').glob('*.md'))
    archive=ROOT/'reports/multiformat-tables-layout-2026-11-12.zip'
    with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED,compresslevel=9) as z:
        for p in sorted(files):z.write(p,'bulkparse-office/'+str(p.relative_to(ROOT)))
        for p in sorted(evidence):z.write(p,'bulkparse-office/'+str(p.relative_to(ROOT)))
    with zipfile.ZipFile(archive) as z:
        assert z.testzip() is None
        for p in files:assert hashlib.sha256(z.read('bulkparse-office/'+str(p.relative_to(ROOT)))).hexdigest()==sha(p)
    archive.with_suffix('.zip.sha256').write_text(sha(archive)+'  '+archive.name+'\n')
    print(json.dumps({'archive':str(archive),'bytes':archive.stat().st_size,'source_files':len(files),'evidence_files':len(evidence),'zip_verified':True}))
if __name__=='__main__':main()
