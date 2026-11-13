#!/usr/bin/env python3
"""Bundle verified OCR/table work without downloaded PDFs, models or binaries."""
from pathlib import Path
import hashlib,json,zipfile
ROOT=Path(__file__).resolve().parents[1]
REPORT=ROOT/'reports/ocr-table-alignment-2026-11-11'
DEST=REPORT.with_suffix('.zip')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 assert json.loads((REPORT/'validation.json').read_text())['passed']
 paths=set()
 def add(p):
  p=ROOT/p;assert p.exists(),p
  for f in p.rglob('*') if p.is_dir() else [p]:
   if f.is_file() and '__pycache__' not in f.parts and f.suffix not in ['.pyc','.pdf','.png','.zip']:paths.add(f)
 for p in ['reports/ocr-table-alignment-2026-11-11','reports/opendataloader-ocr-table-alignment-2026-11-11','reports/independent-ocr-table-alignment-2026-11-11','reports/icdar-ocr-table-alignment-2026-11-11','reports/same-machine-ocr-table-alignment-2026-11-11','reports/opendataloader-table-boundaries-2026-11-11-v3','reports/independent-table-boundaries-2026-11-11-v3','reports/icdar-table-boundaries-2026-11-11-v3','reports/independent-pdf-v2-2026-11-11/quality-summary.json','reports/independent-pdf-v2-2026-11-11/DOCLAYNET-LICENSE.txt','target/inspector-generalization/independent-ocr-table-alignment-2026-11-11','target/table-boundaries/icdar-ocr-table-alignment-2026-11-11','benchmarks/icdar2013-tables','benchmarks/independent-pdf-v2','benchmarks/same-machine','target/independent-pdf-v2/references.json','docs/pdf-inspection.md','docs/pdf-ocr.md']:
  add(p)
 for name in ['benchmark_opendataloader','check_opendataloader_results','benchmark_inspector_generalization','benchmark_icdar_tables','benchmark_icdar_regressions','validate_icdar_regressions','validate_inspector_generalization','validate_ocr_table_alignment','evaluate_independent_pdf','check_pdf_quality_fix','check_pdf_table_upgrade','validate_pdf_tables','report_ocr_table_alignment','package_ocr_table_alignment','check_same_machine_results','benchmark_same_machine','worker_docling_corpus','test_same_machine_benchmark']:
  add('scripts/'+name+'.py')
 for name in ['opendataloader-ocr-table-alignment-2026-11-11','opendataloader-table-boundaries-2026-11-11-v3','same-machine-ocr-table-alignment-2026-11-11']:
  paths.update((ROOT/'reports'/name/'source-snapshot').rglob('*.pdf'))
 paths.discard(REPORT/'bundle-manifest.json')
 manifest={str(p.relative_to(ROOT)):sha(p) for p in sorted(paths)}
 (REPORT/'bundle-manifest.json').write_text(json.dumps({'files':manifest,'downloaded_pdfs_models_binaries_included':False},indent=2)+'\n')
 paths.add(REPORT/'bundle-manifest.json')
 with zipfile.ZipFile(DEST,'w',zipfile.ZIP_DEFLATED,compresslevel=9) as z:
  for p in sorted(paths):z.write(p,str(p.relative_to(ROOT)))
 with zipfile.ZipFile(DEST) as z:
  assert z.testzip() is None
  for name,digest in manifest.items():assert hashlib.sha256(z.read(name)).hexdigest()==digest
 digest=sha(DEST);DEST.with_suffix('.zip.sha256').write_text(digest+'  '+DEST.name+'\n')
 print(json.dumps({'path':str(DEST),'bytes':DEST.stat().st_size,'files':len(paths),'sha256':digest,'verified':True},indent=2))
if __name__=='__main__':main()
