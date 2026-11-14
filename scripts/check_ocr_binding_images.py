#!/usr/bin/env python3
"""Real inference smoke check; requires built bindings and provisioned OCR models."""
import argparse
import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'bindings/python/python'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    config_path = args.config.resolve()
    config = json.loads(config_path.read_text())
    from ocr_image_fixture import sample_table
    from bulkparse import OcrParser
    image, _ = sample_table()
    names = [f'table.{extension}' for extension in ['png', 'jpg', 'gif', 'webp', 'tiff']]
    for name in names:
        image.save(output / name)
    image.save(output / 'two-frames.tiff', save_all=True, append_images=[image])
    image.save(output / 'table.pdf', resolution=120)
    names += ['two-frames.tiff', 'table.pdf']
    node_script = '''
const fs=require('node:fs'), path=require('node:path');
const {OcrParser}=require(process.argv[1]);
(async()=>{
 const parser=new OcrParser(JSON.parse(fs.readFileSync(process.argv[2])));
 const results=[];
 try {
  for(const name of JSON.parse(process.argv[4])) {
   const start=performance.now();
   const result=await parser.parseFile(path.join(process.argv[3],name),{}, {max_pages:2000,max_output_bytes:33554432});
   results.push({name,ms:performance.now()-start,result});
  }
 } finally {parser.close();}
 console.log(JSON.stringify(results));
})().catch(error=>{console.error(error);process.exitCode=1;});
'''
    records = json.loads(subprocess.check_output([
        'node', '-e', node_script, str(ROOT / 'bindings/node'), str(config_path), str(output), json.dumps(names),
    ], text=True))
    summary = []
    with OcrParser(config) as engine:
        for record in records:
            name, result = record['name'], record['result']
            start = time.perf_counter()
            python = engine.parse_file(output / name, budget={'max_pages': 2000, 'max_output_bytes': 33554432})
            python_ms = (time.perf_counter() - start) * 1000
            assert result == python, f'Node/Python output mismatch: {name}'
            tables = [block for block in result['document']['blocks'] if block['type'] == 'table']
            expected = 2 if name == 'two-frames.tiff' else 1
            assert len(tables) == expected, f'Missing table: {name}'
            assert all(table['rows'][0][0]['col_span'] == 2 for table in tables), f'Missing merge: {name}'
            assert all(value in result['markdown'] for value in ['Sales report', 'Product', 'Amount', 'Apples', '125']), name
            (output / (name + '.json')).write_text(json.dumps(result, ensure_ascii=False, indent=2))
            summary.append({'file': name, 'tables': len(tables), 'all_expected_values': True,
                            'identical_node_python': True, 'node_ms': record['ms'], 'python_ms': python_ms})
    report = {
        'note': 'Synthetic smoke cases, not a quality or performance benchmark; first call includes initialization.',
        'model_manifest_sha256': hashlib.sha256(Path(config['models']).read_bytes()).hexdigest(),
        'results': summary,
    }
    (output / 'summary.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
