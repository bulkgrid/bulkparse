#!/usr/bin/env python3
"""Verify sources, native preservation, score replay, and timing for this study."""
from pathlib import Path
from collections import Counter
import hashlib,json,statistics
import evaluate_independent_pdf as evaluator
from check_opendataloader_results import check
from check_pdf_quality_fix import page_characters
ROOT=Path(__file__).resolve().parents[1]
OPEN=ROOT/'reports/opendataloader-generalization-2026-11-11-v3'
OLD=ROOT/'reports/opendataloader-heading-fields-2026-11-11'
STUDY=ROOT/'reports/independent-generalization-2026-11-11'
OUT=ROOT/'reports/inspector-generalization-2026-11-11'
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,v):p.write_text(json.dumps(v,indent=2)+'\n')
def timing(p,n):
    t=read(p);assert t['documents']==n
    for v,runs in t['runs'].items():
        assert len(runs)==6 and [r['warmup'] for r in runs]==[True]+[False]*5
        assert statistics.median(r['elapsed_ns'] for r in runs[1:])/1e9==t['summary'][v]['median_seconds']
        for i,r in enumerate(runs):
            assert len(r['documents'])==n
            assert r['version_order']==(['before','current'] if i%2==0 else ['current','before'])
            assert all(d['ok'] for d in r['documents'])
            assert r['prediction_sha256']==runs[0]['prediction_sha256']
    return t

def main():
    checked=check(OPEN)
    assert read(OPEN/'prediction/bulkparse/evaluation.json')['documents']==read(OLD/'prediction/bulkparse/evaluation.json')['documents']
    assert read(OPEN/'runs.json')[-1]['prediction_sha256']==read(OLD/'runs.json')[-1]['prediction_sha256']
    identity=read(STUDY/'identity.json');q=read(STUDY/'quality.json');refs=read(ROOT/'target/independent-pdf-v2/references.json')
    assert sha(ROOT/'scripts/benchmark_inspector_generalization.py')==identity['harness_sha256']
    assert sha(ROOT/'scripts/evaluate_independent_pdf.py')==identity['evaluator_sha256']
    assert sha(ROOT/'target/independent-pdf-v2/references.json')==identity['reference_sha256']
    assert sha(ROOT/'benchmarks/independent-pdf-v2/manifest.json')==identity['sample_manifest_sha256']
    inputs=0
    for entry in read(ROOT/'benchmarks/independent-pdf-v2/manifest.json')['rows']:
        for kind,digest in entry['sha256'].items():assert sha(ROOT/entry['paths'][kind])==digest;inputs+=1
    for v,manifest in identity['source_manifests'].items():
        for name,digest in manifest.items():
            assert sha(ROOT/identity['source_roots'][v]/name)==digest
            if v=='current':assert sha(ROOT/name)==digest
    counts=Counter();inspection_changes=[]
    assert len(q['documents'])==57
    for row in q['documents']:
        raw={}
        for v,score in row['versions'].items():
            base=ROOT/identity['scratch']/'json'/v
            p=base/(row['id']+'.json');rp=base/(row['id']+'.raw.json')
            assert sha(p)==score['prediction_json_sha256'] and sha(rp)==score['raw_json_sha256']
            raw[v]=read(rp);computed=evaluator.evaluate(read(p),refs[row['id']])
            tables=computed.pop('table_structures');assert hashlib.sha256(json.dumps(tables,sort_keys=True).encode()).hexdigest()==score['table_structure_sha256']
            for name,value in computed.items():assert score[name]==value,(row['id'],v,name)
        assert page_characters(raw['before']['document'])==page_characters(raw['current']['document'])
        for field in ['native_characters_preserved','status_unchanged','table_structures_unchanged']:assert row['comparison'][field]
        if raw['before']['pdf_inspection']!=raw['current']['pdf_inspection']:
            b=raw['before']['pdf_inspection'];c=raw['current']['pdf_inspection']
            # Reviewed test fixture identity; this exception is never in parser code.
            assert row['id']=='99c991140b4dd010bdbd4bf482669f00b6a39b12ef7521ab7e7980bfbad444f5'
            assert sha(ROOT/'target/independent-pdf-v2/pdfs'/(row['id']+'.pdf'))=='de5a7b39fc09f8e5d3f28ad2a385a63f4fb6d2d37c0d518e430eb05b6ed44b99'
            assert b['pages'][0]['route']=='skip' and c['pages'][0]['route']=='ocr' and c['pages'][0]['image_count']==1
            inspection_changes.append({'id':row['id'],'before':b,'current':c})
        counts['changed_json_pages']+=not row['comparison']['json_byte_identical']
        counts['text_order_changed_pages']+=not row['comparison']['normalized_text_order_unchanged']
        b,c=(row['versions'][v] for v in ['before','current'])
        def f1(s):s=s['heading_fuzzy'];return 2*s['tp']/(2*s['tp']+s['fp']+s['fn']) if 2*s['tp']+s['fp']+s['fn'] else 1
        counts['heading_improved_pages']+=f1(c)>f1(b);counts['heading_regressed_pages']+=f1(c)<f1(b)
        assert c['text_token_matches']>=b['text_token_matches'] and c['order_correct_pairs']>=b['order_correct_pairs']
    assert len(inspection_changes)==1 and counts['heading_regressed_pages']==0
    evaluator.VERSIONS=['before','current'];assert evaluator.aggregate(q['documents'])==q['summary']
    timing(STUDY/'timing.json',57);t=timing(STUDY/'opendataloader-timing.json',200)
    for v,p in [('before',OLD),('current',OPEN)]:assert t['runs'][v][-1]['prediction_sha256']==read(p/'runs.json')[-1]['prediction_sha256']
    regression=read(OUT/'regression.json');assert regression['passed']
    assert regression['before_binary_sha256']==identity['cli_sha256']['before'] and regression['after_binary_sha256']==identity['cli_sha256']['current']
    result={'passed':True,'original':checked,'all_original_200_scores_and_markdown_unchanged':True,'input_files_hash_checked':inputs,'native_characters_preserved_on_57_pages':True,'tables_preserved_on_57_pages':True,'heading_text_and_order_page_scores_do_not_regress':True,'inspection_changes':inspection_changes,'regression_pdf_cases':len(regression['pdf_cases']),'authored_table_cases':len(regression['authored_table_cases']),**counts}
    write(OUT/'validation.json',result);print(json.dumps(result,indent=2))
if __name__=='__main__':main()
