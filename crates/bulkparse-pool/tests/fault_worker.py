"""Test-only protocol peer. Fault markers are never read by production code."""
import json
import os
from pathlib import Path
import struct
import sys
import time


def send(value):
    body = json.dumps(value).encode()
    sys.stdout.buffer.write(struct.pack('<I', len(body)) + body)
    sys.stdout.buffer.flush()


if '--no-ready' in sys.argv:
    time.sleep(60)
if '--bad-ready' in sys.argv:
    send({'type': 'ready', 'version': 999})
else:
    send({'type': 'ready', 'version': 1})
while True:
    header = sys.stdin.buffer.read(4)
    if not header:
        break
    job = json.loads(sys.stdin.buffer.read(struct.unpack('<I', header)[0]))
    path = Path(job['path'])
    if path.name == 'hang':
        path.with_suffix('.started').touch()
        time.sleep(60)
    if path.name == 'crash':
        os._exit(7)
    if path.name == 'oversize':
        sys.stdout.buffer.write(struct.pack('<I', 0xffffffff))
        sys.stdout.buffer.flush()
        time.sleep(60)
    if path.name == 'bad-json':
        sys.stdout.buffer.write(struct.pack('<I', 1) + b'!')
        sys.stdout.buffer.flush()
        continue
    send({'type': 'failed', 'id': job['id'] + (path.name == 'wrong-id'),
          'code': 'fixture_ok', 'message': 'test peer processed request'})
