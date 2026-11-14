"""Protocol fixture: no recognition. Real inference is checked separately."""
import argparse
import json
import os
import sys
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--models', required=True)
args, _ = parser.parse_known_args()
with Path(args.models).with_suffix('.pids').open('a') as stream:
    stream.write(str(os.getpid()) + '\n')
for frame in sys.stdin:
    request = json.loads(frame)
    page = request['page']
    reply = {'version': 2, 'page': page, 'page_count': 2, 'image_size': [100, 100],
             'lines': [{'text': f'Image frame {page}', 'quad': [[1, 1], [90, 1], [90, 20], [1, 20]], 'confidence': .9}]}
    print(json.dumps({'id': request['id'], 'response': reply}), flush=True)
