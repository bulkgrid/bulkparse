#!/usr/bin/env python3
"""Bundle final table repairs and reproducibility evidence, without corpus PDFs."""
from pathlib import Path
import hashlib,json,zipfile
ROOT=Path(__file__).resolve().parents[1]
REPORT=ROOT/'reports/table-boundaries-2026-11-11-v3'
DEST=ROOT/'reports/table-boundaries-2026-11-11.zip'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 paths=set()
 def add(name):
  p=ROOT/name
  assert p.exists(),name
  for f in (p.rglob('*') if p.is_dir() else [p]):
   if f.is_file() and '__pycache__' not in f.parts and f.suffix not in ['.pyc','.pdf','.png']:
    paths.add(f)
 for p in [
  'reports/table-boundaries-2026-11-11-v3',
  'reports/opendataloader-table-boundaries-2026-11-11-v3',
  'reports/independent-table-boundaries-2026-11-11-v3',
  'reports/icdar-table-boundaries-2026-11-11-v3',
  'reports/opendataloader-generalization-2026-11-11-v3',
  'reports/independent-generalization-2026-11-11',
  'reports/icdar-generalization-2026-11-11',
  'reports/independent-pdf-v2-2026-11-11/quality-summary.json',
  'reports/independent-pdf-v2-2026-11-11/DOCLAYNET-LICENSE.txt',
  'target/inspector-generalization/independent-table-boundaries-2026-11-11-v3',
  'target/table-boundaries/icdar-table-boundaries-2026-11-11-v3',
  'benchmarks/icdar2013-tables', 'benchmarks/independent-pdf-v2',
  'target/independent-pdf-v2/references.json',
  'reports/pdf-tables-corpus-2026-11-10-v2/manifest.jsonl',
  'benchmarks/holdout/manifest.jsonl',
  'scripts/benchmark_opendataloader.py','scripts/check_opendataloader_results.py',
  'scripts/benchmark_inspector_generalization.py','scripts/validate_inspector_generalization.py',
  'scripts/benchmark_icdar_tables.py','scripts/benchmark_icdar_regressions.py',
  'scripts/validate_table_boundaries.py','scripts/validate_icdar_regressions.py',
  'scripts/evaluate_independent_pdf.py','scripts/check_pdf_quality_fix.py',
  'scripts/check_pdf_table_upgrade.py','scripts/validate_pdf_tables.py','scripts/compare_pdf_inspectors.py',
  'scripts/freeze_icdar_tables.py','scripts/audit_doclaynet_overlap.py',
  'scripts/test_icdar_tables.py','scripts/test_independent_pdf.py','scripts/test_benchmark_opendataloader.py',
  'scripts/package_table_boundaries.py']:
  add(p)
 # Authored source fixtures are required by the source manifests. These are
 # project tests, not downloaded benchmark corpus documents.
 for report in ['opendataloader-generalization-2026-11-11-v3','opendataloader-table-boundaries-2026-11-11-v3']:
  paths.update((ROOT/'reports'/report/'source-snapshot').rglob('*.pdf'))
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
