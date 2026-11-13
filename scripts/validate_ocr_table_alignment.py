#!/usr/bin/env python3
"""Replay the OCR/table-alignment study; retain and explain intentional routing changes."""
from bulkparse_cli import parse_command
from collections import Counter
from pathlib import Path
import hashlib,json,sys,subprocess
import evaluate_independent_pdf as evaluator
from check_opendataloader_results import check
from check_pdf_quality_fix import page_characters
from validate_inspector_generalization import timing
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'reports/ocr-table-alignment-2026-11-11'
OPEN=ROOT/'reports/opendataloader-ocr-table-alignment-2026-11-11'
OLD=ROOT/'reports/opendataloader-table-boundaries-2026-11-11-v3'
STUDY=ROOT/'reports/independent-ocr-table-alignment-2026-11-11'
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,v):p.write_text(json.dumps(v,indent=2)+'\n')
def main():
    original=check(OPEN)
    sys.path.insert(0,str(ROOT/'target/opendataloader-bench/src'))
    from evaluator import _evaluate_single_document,_aggregate_document_scores
    documents=[];changes=[]
    old={r['document_id']:r['scores'] for r in read(OLD/'prediction/bulkparse/evaluation.json')['documents']}
    for p in sorted((ROOT/'target/opendataloader-bench/ground-truth/markdown').glob('*.md')):
        score=_evaluate_single_document(p.stem,p,OPEN/'prediction/bulkparse/markdown'/p.name)
        documents.append(score);v=score.to_json()['scores'];b=old[p.stem]
        assert all(v[k] is None or v[k]>=b[k]-1e-12 for k in v),(p.stem,b,v)
        if v!=b:changes.append({'id':p.stem,'before':b,'current':v})
    saved=read(OPEN/'prediction/bulkparse/evaluation.json')
    assert [d.to_json() for d in documents]==saved['documents']
    assert _aggregate_document_scores(documents)==saved['metrics']
    identity=read(STUDY/'identity.json');q=read(STUDY/'quality.json');refs=read(ROOT/'target/independent-pdf-v2/references.json')
    assert len(q['documents'])==57 and q['fresh_before_reproduces_saved_quality_and_json']
    assert sha(ROOT/'scripts/benchmark_inspector_generalization.py')==identity['harness_sha256']
    assert sha(ROOT/'scripts/evaluate_independent_pdf.py')==identity['evaluator_sha256']
    assert sha(ROOT/'target/independent-pdf-v2/references.json')==identity['reference_sha256']
    for v,manifest in identity['source_manifests'].items():
        for name,digest in manifest.items():assert sha(ROOT/identity['source_roots'][v]/name)==digest
    for row in q['documents']:
        for v,score in row['versions'].items():
            base=ROOT/identity['scratch']/'json'/v
            p=base/(row['id']+'.json');rp=base/(row['id']+'.raw.json')
            assert sha(p)==score['prediction_json_sha256'] and sha(rp)==score['raw_json_sha256']
            computed=evaluator.evaluate(read(p),refs[row['id']]);tables=computed.pop('table_structures')
            assert hashlib.sha256(json.dumps(tables,sort_keys=True).encode()).hexdigest()==score['table_structure_sha256']
            assert all(score[k]==value for k,value in computed.items())
        assert all(row['comparison'][k] for k in ['native_characters_preserved','inspection_unchanged','status_unchanged','json_byte_identical'])
    evaluator.VERSIONS=['before','current'];assert evaluator.aggregate(q['documents'])==q['summary']
    timing(STUDY/'timing.json',57);timing(STUDY/'opendataloader-timing.json',200)
    preservation=read(OUT/'preservation-strict.json')
    assert len(preservation['pdf_cases'])==304 and len(preservation['authored_table_cases'])==9
    assert all(r['passed'] for r in preservation['authored_table_cases'])
    changes_ocr=[]
    binaries={'before':ROOT/'target/table-boundaries/after-cli/release/bulkparse','current':ROOT/'target/ocr-table-alignment/after-cli/release/bulkparse'}
    assert sha(binaries['before'])==preservation['before_binary_sha256']
    assert sha(binaries['current'])==preservation['after_binary_sha256']
    assert sha(ROOT/'scripts/check_pdf_table_upgrade.py')==preservation['harness_sha256']
    for row in preservation['pdf_cases']:
        assert sha(Path(row['source']))==row['sha256']
        if row.get('expected_failure'):assert row['passed'];continue
        assert row['characters_preserved'] and row['before_exit']==row['after_exit']
        if row['inspection_unchanged']:assert row['passed'];continue
        # This is an audit of the investigated input, never a parser exception.
        assert row['sha256']=='827d21de372a2c26237ee1db526460851ae71c1867761776583535f532432e32'
        values={}
        for version,binary in binaries.items():
            r=subprocess.run([str(binary), parse_command(binary),row['source'],'--to','json'],capture_output=True,check=False)
            assert r.returncode in (0,2);values[version]=json.loads(r.stdout)
            (OUT/(version+'-infographic.json')).write_bytes(r.stdout)
        b,c=(values[v] for v in ['before','current'])
        assert page_characters(b['document'])==page_characters(c['document'])
        assert b['pdf_inspection']['pages_needing_ocr']==[] and c['pdf_inspection']['pages_needing_ocr']==[1]
        bp=b['pdf_inspection']['pages'][0];cp=c['pdf_inspection']['pages'][0]
        assert bp['route']=='native' and cp['route']=='ocr' and cp['native_characters']==7
        assert cp['ocr_status']=='not_requested' and 'outlined_text_with_sparse_native_text' in cp['reasons']
        assert {k:v for k,v in bp.items() if k not in ['route','reasons','ocr_status']}=={k:v for k,v in cp.items() if k not in ['route','reasons','ocr_status']}
        changes_ocr.append(row['source'])
    assert len(changes_ocr)==1
    icdar=read(ROOT/'reports/icdar-ocr-table-alignment-2026-11-11/validation.json')
    assert icdar['artifact_and_scoring_validation_passed'] and not icdar['regressions']
    result={'passed':True,'original':original,'all_200_document_scores_replayed':True,'score_changes':changes,
            'original_score_regressions':0,'doclaynet_pages_identical':57,'preservation_pdf_cases':304,
            'native_characters_preserved':True,'authored_table_checks_passed':9,'intentional_ocr_changes':changes_ocr,
            'icdar_annotated_pages':icdar['annotated_pages'],'icdar_score_regressions':0,
            'icdar_teds_improved_pages':icdar['teds_improved_pages']}
    write(OUT/'validation.json',result);print(json.dumps(result,indent=2))
if __name__=='__main__':main()
