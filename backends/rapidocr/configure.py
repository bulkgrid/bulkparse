#!/usr/bin/env python3
"""Record installed local model fingerprints; never install or download assets."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
from profiles import DETECTION, ORIENTATION, PROFILES, profile_settings


def configure(output, profile='multi-small', recognition_model=None, model_directory=None):
    # Validate everything before creating settings, including overwrite checks.
    output = Path(output).absolute()
    manifest = output/'models.json'
    config = output/'config.json'
    if manifest.exists() or config.exists():
        raise ValueError('refusing to overwrite existing OCR settings')
    if profile not in PROFILES:
        raise ValueError('unknown OCR profile')
    if model_directory is None:
        spec = importlib.util.find_spec('rapidocr')
        if spec is None: raise ValueError('install the pinned RapidOCR requirements first')
        model_directory = Path(spec.origin).parent/'models'
    directory = Path(model_directory)
    paths = {'det': directory/DETECTION['filename'], 'cls': directory/ORIENTATION['filename'],
             'rec': Path(recognition_model) if recognition_model else directory/PROFILES[profile]['filename']}
    models = {'version':2, 'profile':profile, 'models':{}}
    for role, path in paths.items():
        if not path.is_file():
            raise ValueError(f'{role} model is not installed locally: {path}')
        digest = hashlib.sha256()
        with path.open('rb') as stream:
            for chunk in iter(lambda: stream.read(1024*1024), b''): digest.update(chunk)
        models['models'][role] = {'path': str(path.absolute()), 'sha256': digest.hexdigest()}
    profile_settings(models)
    output.mkdir(parents=True, exist_ok=True)
    config_data = {'executable':str(Path(sys.executable).absolute()),
        'args':[str(Path(__file__).absolute().with_name('rapidocr_backend.py')),
                '--release-unused-memory'] + (['--adaptive-detector'] if profile == 'multi-small' else []),
        'models':str(manifest),'dpi':200,'max_pixels':8000000,'page_timeout_ms':30000,
        'max_input_bytes':33554432,'reuse_process':True,'max_pages_per_worker':128}
    with manifest.open('x', encoding='utf-8') as stream:
        stream.write(json.dumps(models,indent=2)+'\n')
    try:
        with config.open('x', encoding='utf-8') as stream:
            stream.write(json.dumps(config_data,indent=2)+'\n')
    except OSError:
        manifest.unlink()
        raise
    return config


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--profile',choices=PROFILES,default='multi-small')
    parser.add_argument('--recognition-model',type=Path,
                        help='already-downloaded recognition ONNX model matching the selected profile')
    args=parser.parse_args()
    try: print(configure(args.output,args.profile,args.recognition_model))
    except (ValueError,OSError) as error: parser.exit(1,str(error)+'\n')


if __name__=='__main__': main()
