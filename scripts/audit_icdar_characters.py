#!/usr/bin/env python3
"""Post-hoc character audit against independently cached PDFKit text, not gold."""
from pathlib import Path
from collections import Counter
import json,hashlib
from evaluate_independent_pdf import strings
ROOT=Path(__file__).resolve().parents[1]
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 report=ROOT/'reports/icdar-generalization-2026-11-11';validation=read(report/'validation.json');identity=read(report/'identity.json');base=ROOT/identity['scratch']/'json';rows={r['id']:r for r in read(ROOT/'benchmarks/icdar2013-tables/manifest.json')['rows']};result=[]
 for row in validation['native_character_changes']:
  id=row['id'];ref_path=ROOT/'target/inspector-generalization/icdar-native'/(id+'.txt');assert sha(ref_path)==rows[id]['native_text_sha256']
  ref=Counter(c for c in ref_path.read_text() if not c.isspace());vals={}
  for v in ['before','current']:
   p=base/v/(id+'.raw.json');assert sha(p)==row['raw_sha256'][v]
   pred=read(p);c=Counter(c for s in strings(pred['document']['blocks']) for c in s if not c.isspace());vals[v]={'missing_vs_pdfkit':sum((ref-c).values()),'extra_vs_pdfkit':sum((c-ref).values())}
  result.append({'id':id,**vals})
 out={'post_hoc_diagnostic_not_a_frozen_quality_metric':True,'reference':'PDFKit native text cached before predictions for overlap audit; not human ground truth','script_sha256':sha(Path(__file__)),'documents':result}
 (report/'pdfkit-character-audit.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps({'changed_documents':len(result),'all_current_character_multisets_match_pdfkit':all(r['current']=={'missing_vs_pdfkit':0,'extra_vs_pdfkit':0} for r in result)},indent=2))
if __name__=='__main__':main()
