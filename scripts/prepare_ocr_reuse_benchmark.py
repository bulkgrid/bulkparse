#!/usr/bin/env python3
"""Prepare the frozen before/after Linux build and local model configurations.

No dependencies or model assets are downloaded. Use --replace-generated only to
replace this runner's build context/configurations; measured results are untouched.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

ROOT=Path(__file__).resolve().parents[1]
REPORT=ROOT/'reports/ocr-reuse-2026-11-11'

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--models',type=Path,default=ROOT/'target/ocr-alternatives/data/models')
    ap.add_argument('--output',type=Path,default=ROOT/'target/ocr-reuse')
    ap.add_argument('--replace-generated',action='store_true')
    a=ap.parse_args();out=a.output.resolve();out.relative_to(ROOT)
    model_dir=a.models.resolve();model_dir.relative_to(ROOT)
    models=json.loads((REPORT/'configs/models.json').read_text())
    for entry in models['models'].values():
        path=model_dir/Path(entry['path']).name
        assert hashlib.sha256(path.read_bytes()).hexdigest()==entry['sha256'],path
        entry['path']='/repo/'+str(path.relative_to(ROOT))
    for name in ['linux-build','configs']:
        path=out/name
        if path.exists() and not a.replace_generated:raise ValueError(f'{path} exists; use a fresh checkout or --replace-generated')
    build=out/'linux-build';config=out/'configs'
    # Only these two generated directories are replaced, never benchmark outputs.
    for path in [build,config]:
        if path.exists():shutil.rmtree(path)
        path.mkdir(parents=True)
    for name,digest in json.loads((REPORT/'build-source-manifest.json').read_text()).items():
        source=REPORT/'linux-build-source'/name
        assert hashlib.sha256(source.read_bytes()).hexdigest()==digest,source
        destination=build/name;destination.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(source,destination)
    (config/'models.json').write_text(json.dumps(models,indent=2)+'\n')
    for source in (REPORT/'configs').glob('*.json'):
        if source.name=='models.json':continue
        value=json.loads(source.read_text());value['models']='/repo/'+str((config/'models.json').relative_to(ROOT))
        (config/source.name).write_text(json.dumps(value,indent=2)+'\n')
    print(f'Prepared {build} and {config}; no models downloaded.')
    if out!=ROOT/'target/ocr-reuse':
        print('The benchmark runner reads target/ocr-reuse/configs; this alternate output is for build inspection only.')

if __name__=='__main__':main()
