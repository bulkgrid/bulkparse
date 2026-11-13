#!/usr/bin/env python3
"""Replay independent table scoring from saved predictions and check raw text."""
from bulkparse_cli import parse_command
from collections import Counter,defaultdict
from pathlib import Path
import json,subprocess
from benchmark_icdar_tables import ROOT,SPEC,read,write,sha,score_page,prediction_tables,aggregate
from validate_inspector_generalization import timing
from check_pdf_quality_fix import page_characters
REPORT=ROOT/'reports/icdar-generalization-2026-11-11'
def main():
    identity=read(REPORT/'identity.json');q=read(REPORT/'quality.json');manifest=read(SPEC/'manifest.json');refs=read(SPEC/'references.json');freeze=read(SPEC/'freeze.json')
    assert q['complete'] and len(q['documents'])==freeze['eligible']==67
    assert identity['protocol_freeze_sha256']==sha(SPEC/'freeze.json')
    for name,digest in freeze['files'].items():assert sha(SPEC/name)==digest
    assert identity['harness_sha256']==sha(ROOT/'scripts/benchmark_icdar_tables.py')
    assert identity['evaluator_sha256']==sha(ROOT/'target/opendataloader-bench/src/evaluator_table.py')
    previous=read(ROOT/'reports/independent-generalization-2026-11-11/identity.json')
    assert identity['cli_sha256']==previous['cli_sha256'] and identity['worker_sha256']==previous['worker_sha256'] and identity['source_manifests']==previous['source_manifests']
    for name,digest in identity['source_manifests']['current'].items():assert sha(ROOT/name)==digest
    inputs={r['id']:r for r in manifest['rows'] if not r['excluded']};pages=defaultdict(list);changes=[];regressions=[];rawchecks=[];counts=Counter()
    binaries={'before':ROOT/'target/heading-field-fix/after-cli/release/bulkparse','current':ROOT/'target/inspector-generalization/after-cli/release/bulkparse'}
    for v,p in binaries.items():assert sha(p)==identity['cli_sha256'][v]
    for row in q['documents']:
        source=inputs[row['id']];assert sha(ROOT/source['pdf'])==source['pdf_sha256'];assert sha(ROOT/source['xml'])==source['xml_sha256'];raw={}
        gold=refs[row['id']]
        for v,score in row['versions'].items():
            base=ROOT/identity['scratch']/'json'/v;p=base/(row['id']+'.json');assert sha(p)==score['prediction_sha256']
            tables=prediction_tables(read(p));assert tables==score['tables']
            recomputed=[]
            for page in sorted({t['page'] for t in gold}):
                s=score_page([t for t in gold if t['page']==page],[t for t in tables if t['page']==page]);s.update(page=page,id=row['id']);recomputed.append(s)
            assert recomputed==score['pages'];pages[v].extend(recomputed)
            rp=base/(row['id']+'.raw.json')
            run=subprocess.run([str(binaries[v]), parse_command(binaries[v]),str(ROOT/source['pdf']),'--to','json','--pdf-normalize-unicode=false'],capture_output=True,timeout=120)
            rp.write_bytes(run.stdout);rp.with_suffix('.stderr').write_bytes(run.stderr);raw[v]=json.loads(run.stdout)
        b,c=(raw[v] for v in ['before','current']);bc,cc=page_characters(b['document']),page_characters(c['document'])
        char_changes=[]
        for p in set(bc)|set(cc):
            if bc.get(p,Counter())!=cc.get(p,Counter()):char_changes.append({'page':p,'added':dict(cc.get(p,Counter())-bc.get(p,Counter())),'removed':dict(bc.get(p,Counter())-cc.get(p,Counter()))})
        rawchecks.append({'id':row['id'],'raw_sha256':{v:sha(ROOT/identity['scratch']/'json'/v/(row['id']+'.raw.json')) for v in raw},'native_character_changes':char_changes,'inspection_equal':b['pdf_inspection']==c['pdf_inspection'],'status_equal':b['status']==c['status']})
        for b,c in zip(row['versions']['before']['pages'],row['versions']['current']['pages']):
            d=c['teds']-b['teds'];ds=c['teds_s']-b['teds_s']
            counts['teds_improved_pages' if d>1e-12 else 'teds_regressed_pages' if d< -1e-12 else 'teds_unchanged_pages']+=1
            counts['structure_improved_pages' if ds>1e-12 else 'structure_regressed_pages' if ds< -1e-12 else 'structure_unchanged_pages']+=1
            if d or ds:changes.append({'id':row['id'],'page':b['page'],'before_teds':b['teds'],'current_teds':c['teds'],'before_structure':b['teds_s'],'current_structure':c['teds_s']})
            if d< -1e-12 or ds< -1e-12:regressions.append(changes[-1])
        print(row['id'],'verified',flush=True)
    for v in pages:assert aggregate(pages[v])==q['summary'][v]
    timing(REPORT/'timing.json',67)
    result={'artifact_and_scoring_validation_passed':True,'all_page_scores_recomputed':True,'prediction_tuning_after_holdout':False,'documents':67,'annotated_pages':len(pages['current']),'native_character_changes':[r for r in rawchecks if r['native_character_changes']],'inspection_changes':[r['id'] for r in rawchecks if not r['inspection_equal']],'raw_checks':rawchecks,'regressions':regressions,'score_changes':changes,**counts}
    write(REPORT/'validation.json',result);print(json.dumps({k:v for k,v in result.items() if k not in ['raw_checks','score_changes']},indent=2))
if __name__=='__main__':main()
