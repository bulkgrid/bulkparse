#!/usr/bin/env python3
"""Compare explicitly provisioned OCR profiles on a frozen, tiered PDF corpus.

Fresh CLI/backend processes per document. Sample CLI+registered-backend RSS, not just
Rust's small supervisor. No model downloads, output repairs or result caching.
"""
from bulkparse_cli import parse_command
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import tempfile
import time
import psutil
from evaluate_ocr import distance, normalized, quality, text

ROOT=Path(__file__).resolve().parents[1]


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def measure(command, *, pid_file=None, timeout=90, max_rss=2*1024**3, max_output=8*1024**2):
    """Sample only the root and explicitly registered direct child processes.

    No system-wide process enumeration. The trusted OCR backend uses threads,
    not further descendants; unregistered grandchildren would not be counted.
    """
    if os.name!='posix':raise ValueError('this measurement harness requires POSIX process groups')
    with tempfile.TemporaryDirectory(prefix='bulkparse-ocr-measure-') as directory:
        paths=[Path(directory)/name for name in ['stdout','stderr']]
        with paths[0].open('wb') as stdout,paths[1].open('wb') as stderr:
            started=time.monotonic()
            process=subprocess.Popen(command,stdout=stdout,stderr=stderr,start_new_session=True)
            try:root=psutil.Process(process.pid)
            except psutil.NoSuchProcess:root=None
            observed={};peak=0;reason=None
            try:
                while process.poll() is None:
                    members=[root] if root else []
                    if pid_file and Path(pid_file).is_file():
                        if Path(pid_file).stat().st_size>16384:
                            reason='pid_registration_budget';break
                        for line in Path(pid_file).read_text().splitlines(keepends=True):
                            if not line.endswith('\n'):continue
                            registration=json.loads(line)
                            if registration['parent']!=process.pid:continue
                            try:
                                child=psutil.Process(registration['pid'])
                                if child.ppid()==process.pid:members.append(child)
                            except psutil.NoSuchProcess:pass
                    rss=0
                    for member in members:
                        try:
                            rss+=member.memory_info().rss
                            if member.pid!=process.pid:observed[member.pid]=member.create_time()
                        except psutil.NoSuchProcess:pass
                    peak=max(peak,rss)
                    if rss>max_rss:reason='sampled_rss_budget'
                    if time.monotonic()-started>timeout:reason='wall_timeout'
                    if any(p.stat().st_size>max_output for p in paths):reason='output_budget'
                    if reason:break
                    time.sleep(.01)
            finally:
                # Also kill surviving group members if a faulty parent exits early.
                try:os.killpg(process.pid,signal.SIGKILL)
                except ProcessLookupError:pass
                process.wait()
            elapsed=time.monotonic()-started
        if any(p.stat().st_size>max_output for p in paths):reason='output_budget'
        payload=paths[0].read_bytes() if paths[0].stat().st_size<=max_output else b''
        with paths[1].open('rb') as stream:error=stream.read(4096).decode('utf-8',errors='replace')
        return {'wall_seconds':elapsed,'sampled_peak_rss_bytes':peak,
                'observed_backend_processes':len(observed),'exit_code':process.returncode,
                'harness_error':reason},payload,error


def score(value,reference):
    result=quality(value,reference)
    a,b=normalized(value),normalized(reference)
    result.update({'character_errors':distance(b,a),'reference_characters':len(b),
                   'word_errors':distance(b.split(),a.split()),'reference_words':len(b.split())})
    return result


def aggregate(rows):
    groups={}
    for row in rows:
        key=(row['profile'],row['track'],row['language'])
        groups.setdefault(key,[]).append(row)
    result=[]
    for (profile,track,language),items in sorted(groups.items()):
        scored=[r for r in items if r['quality'] is not None]
        chars=sum(r['quality']['reference_characters'] for r in scored)
        errors=sum(r['quality']['character_errors'] for r in scored)
        result.append({'profile':profile,'track':track,'language':language,'samples':len(items),
            'distinct_cases':len({r['id'] for r in items}),
            'conversion_failures':sum(r['exit_code'] not in (0,2) or r['harness_error'] is not None for r in items),
            'ocr_error_results':sum('pdf_ocr_error' in r['diagnostic_codes'] for r in items),
            'exact_samples':sum(r['quality']['exact_after_whitespace'] for r in scored),
            'character_errors':errors if scored else None,
            'reference_characters':chars if scored else None,
            'weighted_cer':errors/chars if chars else None,
            'median_wall_seconds':statistics.median(r['wall_seconds'] for r in items),
            'max_sampled_rss_mib':max(r['sampled_peak_rss_bytes'] for r in items)/1024**2})
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--profiles',type=Path,required=True,help='JSON map of names to local config paths')
    parser.add_argument('--manifest',type=Path,default=ROOT/'benchmarks/ocr-profiles/manifest.json')
    parser.add_argument('--include-external',action='store_true')
    parser.add_argument('--repeats',type=int,default=2)
    parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    if not 1<=args.repeats<=10:parser.error('repeats must be 1..10')
    configs=json.loads(args.profiles.read_text())
    if not configs or any(not name.replace('-','').isalnum() for name in configs):parser.error('invalid profile names')
    all_cases=json.loads(args.manifest.read_text())
    cases=[c for c in all_cases if args.include_external or c['track']!='external_smoke']
    for case in cases:
        if not case['id'].replace('-','').isalnum():raise ValueError('invalid case id')
        assert sha(ROOT/case['path'])==case['sha256'],case['id']
        if case.get('reference'):assert sha(ROOT/case['reference'])==case['reference_sha256'],case['id']
    settings={}
    for name,path in configs.items():
        config=json.loads(Path(path).read_text());models=json.loads(Path(config['models']).read_text())
        for model in models['models'].values():assert sha(model['path'])==model['sha256']
        settings[name]={'config':config,'manifest':models,
            'total_model_bytes':sum(Path(m['path']).stat().st_size for m in models['models'].values())}
    args.out.mkdir(parents=True,exist_ok=False)
    files=[Path(__file__),ROOT/'scripts/evaluate_ocr.py',ROOT/'backends/rapidocr/rapidocr_backend.py',
        ROOT/'backends/rapidocr/profiles.py',ROOT/'backends/rapidocr/configure.py',
        ROOT/'crates/bulkparse-ocr/src/lib.rs',ROOT/'crates/bulkparse/src/ocr.rs',
        ROOT/'crates/bulkparse/src/ocr/layout.rs',ROOT/'scripts/ocr_measure_backend.py',ROOT/'Cargo.lock']
    metadata={'created_utc':datetime.now(timezone.utc).isoformat(),'platform':platform.platform(),
        'python':platform.python_version(),'psutil':psutil.__version__, 'binary_sha256':sha(args.binary),
        'manifest_sha256':sha(args.manifest),'profile_settings':settings,'repeats':args.repeats,
        'source_sha256':{str(p.resolve().relative_to(ROOT)):sha(p) for p in files},
        'method':'Fresh CLI and backend per conversion, rotating profile order. One untimed English functional preflight per profile; warmed filesystem/model-file caches. Model/startup/render/IPC plus benchmark-only Python PID-wrapper startup included. CLI+explicitly registered direct OCR child RSS sampled every 10ms; no global process enumeration, unregistered grandchildren excluded, shared pages can be counted twice, spikes may be missed. Controlled original scans are not real-world accuracy evidence. External scans have no accuracy reference. Errors retained.'}
    (args.out/'run.json').write_text(json.dumps(metadata,indent=2)+'\n')
    preflight=[]
    for name,path in configs.items():
        check=subprocess.run([str(args.binary), parse_command(args.binary),str(ROOT/'fixtures/ocr/ocr-prose.pdf'),
            '--to','json','--ocr-config',str(Path(path).absolute())],capture_output=True,timeout=45)
        parsed=json.loads(check.stdout) if check.stdout else {}
        codes=[d['code'] for d in parsed.get('diagnostics',[])]
        valid=check.returncode==2 and 'pdf_ocr_applied' in codes and 'pdf_ocr_error' not in codes
        preflight.append({'profile':name,'passed':valid,'exit_code':check.returncode,'diagnostics':parsed.get('diagnostics',[])})
        (args.out/'preflight.json').write_text(json.dumps(preflight,indent=2)+'\n')
        if not valid:raise ValueError(f'{name} OCR preflight failed; inspect preflight.json')
    records=[];names=list(configs)
    with (args.out/'samples.jsonl').open('w') as stream:
        for repeat in range(args.repeats):
            for index,case in enumerate(cases):
                offset=(repeat+index)%len(names)
                for name in names[offset:]+names[:offset]:
                    with tempfile.TemporaryDirectory(prefix='bulkparse-ocr-profile-') as temp:
                        pid_file=Path(temp)/'backend-pids.jsonl'
                        config=settings[name]['config']
                        wrapped={**config,'args':[str(ROOT/'scripts/ocr_measure_backend.py'),
                            '--pid-file',str(pid_file),'--',config['executable'],*config['args']]}
                        config_path=Path(temp)/'config.json'
                        config_path.write_text(json.dumps(wrapped))
                        command=[str(args.binary), parse_command(args.binary),str(ROOT/case['path']),'--to','json',
                                 '--ocr-config',str(config_path)]
                        measured,payload,error=measure(command,pid_file=pid_file)
                    parsed=None
                    if measured['exit_code'] in (0,2) and not measured['harness_error']:
                        try:parsed=json.loads(payload)
                        except (ValueError,UnicodeError):measured['harness_error']='invalid_json'
                    value=text(parsed) if parsed else ''
                    row={**measured,'id':case['id'],'base_id':case.get('base_id',case['id']),
                        'profile':name,'repeat':repeat,'track':case['track'],'language':case['language'],
                        'output_sha256':hashlib.sha256(payload).hexdigest(),
                        'text_sha256':hashlib.sha256(value.encode()).hexdigest(),
                        'characters_returned':len(value),'status':parsed.get('status') if parsed else None,
                        'diagnostic_codes':sorted({d['code'] for d in parsed['diagnostics']}) if parsed else [],
                        'ocr_pages':sorted({d['source']['page'] for d in parsed['diagnostics'] if d['code']=='pdf_ocr_applied'}) if parsed else [],
                        'quality':score(value,(ROOT/case['reference']).read_text()) if case.get('reference') else None}
                    if row['quality'] and (not parsed or 'pdf_ocr_error' in row['diagnostic_codes']):
                        row['quality']['exact_after_whitespace']=False
                    if case['track']!='external_smoke':
                        dest=args.out/'outputs'/name;dest.mkdir(parents=True,exist_ok=True)
                        (dest/f"{case['id']}-{repeat}.json").write_bytes(payload)
                        if error:(dest/f"{case['id']}-{repeat}.stderr").write_text(error)
                    records.append(row);stream.write(json.dumps(row)+'\n');stream.flush()
                print(f"round {repeat+1}: {case['id']}",flush=True)
    (args.out/'summary.json').write_text(json.dumps(aggregate(records),indent=2)+'\n')


if __name__=='__main__':main()
