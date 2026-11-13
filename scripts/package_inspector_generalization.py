#!/usr/bin/env python3
"""Package reproducibility artifacts without downloaded source PDFs or binaries."""
from pathlib import Path
import hashlib,json,zipfile
ROOT=Path(__file__).resolve().parents[1]
REPORT=ROOT/'reports/inspector-generalization-2026-11-11'
DEST=ROOT/'reports/inspector-generalization-2026-11-11.zip'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 paths=set()
 def add(p):
  p=ROOT/p
  if p.is_dir():
   for f in p.rglob('*'):
    if f.is_file() and '__pycache__' not in f.parts and f.suffix not in ['.pyc','.pdf']:paths.add(f)
  elif p.exists():paths.add(p)
  else:raise FileNotFoundError(p)
 for p in [
  'reports/inspector-generalization-2026-11-11',
  'reports/opendataloader-generalization-2026-11-11-v3',
  'reports/independent-generalization-2026-11-11',
  'reports/icdar-generalization-2026-11-11',
  'reports/opendataloader-heading-fields-2026-11-11',
  'reports/independent-heading-fields-2026-11-11',
  'reports/independent-pdf-v2-2026-11-11/quality-summary.json',
  'reports/independent-pdf-v2-2026-11-11/DOCLAYNET-LICENSE.txt',
  'target/inspector-generalization/independent-generalization-2026-11-11',
  'target/inspector-generalization/icdar-generalization-2026-11-11',
  'target/inspector-generalization/icdar-native',
  'benchmarks/icdar2013-tables',
  'benchmarks/independent-pdf-v2',
  'target/independent-pdf-v2/references.json',
  'target/independent-pdf-v2/review_bounds.swift',
  'target/holdout_pdfkit.swift',
  'target/inspector-generalization/render_page.swift',
  'scripts/benchmark_opendataloader.py','scripts/check_opendataloader_results.py',
  'scripts/benchmark_inspector_generalization.py','scripts/validate_inspector_generalization.py',
  'scripts/evaluate_independent_pdf.py','scripts/check_pdf_quality_fix.py',
  'scripts/compare_pdf_inspectors.py','scripts/validate_pdf_tables.py',
  'scripts/check_pdf_table_upgrade.py','scripts/benchmark_icdar_tables.py',
  'scripts/freeze_icdar_tables.py','scripts/audit_doclaynet_overlap.py',
  'scripts/validate_icdar_tables.py','scripts/audit_icdar_characters.py',
  'scripts/test_icdar_tables.py','scripts/test_independent_pdf.py','scripts/test_benchmark_opendataloader.py',
  'scripts/package_inspector_generalization.py']:
  # Renderer sources vary between existing workspace setups; omit optional unavailable helper.
  if p=='target/holdout_pdfkit.swift' and not (ROOT/p).exists():continue
  add(p)
 for v in json.loads((REPORT/'vendor-review.json').read_text()).values():
  for p in v['sha256']:add(p)
 # Authored fixtures belong to the project; preserve them for source-manifest
 # verification. Downloaded benchmark PDFs are deliberately not bundled.
 for root in ['reports/opendataloader-heading-fields-2026-11-11/source-snapshot','reports/opendataloader-generalization-2026-11-11-v3/source-snapshot']:
  for f in (ROOT/root).rglob('*.pdf'):paths.add(f)
 paths.discard(REPORT/'bundle-manifest.json')
 manifest={str(p.relative_to(ROOT)):sha(p) for p in sorted(paths)}
 (REPORT/'bundle-manifest.json').write_text(json.dumps({'files':manifest,'downloaded_corpus_pdfs_included':False,'compiled_binaries_included':False},indent=2)+'\n')
 paths.add(REPORT/'bundle-manifest.json')
 with zipfile.ZipFile(DEST,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as z:
  for p in sorted(paths):z.write(p,str(p.relative_to(ROOT)))
 with zipfile.ZipFile(DEST) as z:
  assert z.testzip() is None
  for name,digest in manifest.items():assert hashlib.sha256(z.read(name)).hexdigest()==digest
  print(json.dumps({'archive':str(DEST),'bytes':DEST.stat().st_size,'files':len(z.namelist()),'sha256':sha(DEST),'all_archived_file_hashes_verified':True},indent=2))
 DEST.with_suffix('.zip.sha256').write_text(sha(DEST)+'  '+DEST.name+'\n')
if __name__=='__main__':main()
