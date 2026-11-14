"""Shared benchmark transports and provenance helpers; no benchmark runs on import."""
import hashlib
import json
import os
import selectors
import signal
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / 'reports/mixed-200-2026-11-13'
ENGINES = ['bulkparse', 'anydoc', 'docling', 'markitdown', 'pandoc', 'mammoth', 'libreoffice', 'unstructured']

def digest(p):
 with p.open('rb') as stream:
  return hashlib.file_digest(stream, 'sha256').hexdigest()
def write(p,obj):p.write_text(json.dumps(obj,indent=2,ensure_ascii=False)+'\n')
def commands():
 py=ROOT/'target/competitors/broad-venv/bin/python'
 c={e:[str(py),str(ROOT/'scripts/worker_mixed200.py'),e] for e in ENGINES}
 c['bulkparse']=[str(ROOT/'target/release/examples/office_bench')]
 c['anydoc']=[str(ROOT/'target/multiformat-anydoc/release/anydoc-bench-worker')]
 c['docling'][0]=str(ROOT/'target/competitors/docling-venv/bin/python')
 c['unstructured'][0]=str(ROOT/'target/competitors/realworld-venv/bin/python')
 return c
class DocumentWorker:
 def __init__(self,cmd,log,env):
  self.log=log.open('w');self.p=subprocess.Popen(['/usr/bin/time','-l',*cmd],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log,env=env,start_new_session=True,cwd=ROOT)
  self.pending=b'';self.sel=selectors.DefaultSelector();self.sel.register(self.p.stdout,selectors.EVENT_READ)
 def receive(self,timeout):
  end=time.monotonic()+timeout;chunks=[self.pending] if self.pending else []
  while not chunks or b'\n' not in chunks[-1]:
   if time.monotonic()>=end:raise TimeoutError('90-second conversion deadline exceeded')
   if not self.sel.select(min(1,max(0,end-time.monotonic()))):continue
   b=os.read(self.p.stdout.fileno(),1<<20)
   if not b:raise RuntimeError('Worker exited before returning output')
   chunks.append(b)
  b,self.pending=b''.join(chunks).split(b'\n',1);return json.loads(b)
 def call(self,p):
  self.p.stdin.write((json.dumps({'documents':[str(p)]})+'\n').encode());self.p.stdin.flush()
  return self.receive(90)['documents'][0]
 def close(self,kill=False):
  if kill:
   try:os.killpg(self.p.pid,signal.SIGKILL)
   except ProcessLookupError:pass
  else:
   try:self.p.stdin.close();self.p.wait(timeout=15)
   except (BrokenPipeError,subprocess.TimeoutExpired):
    try:os.killpg(self.p.pid,signal.SIGKILL)
    except ProcessLookupError:pass
  self.p.wait();self.sel.close();self.p.stdout.close();self.log.close();return self.p.returncode

def validate_run(run, paths, directory):
    ids = [p.stem for p in paths]
    assert [d['document_id'] for d in run['documents']] == ids, 'Missing, duplicate or reordered outcomes'
    expected = {x+'.md' for x in ids}
    assert {p.name for p in directory.glob('*.md')} == expected, 'Missing or extra predictions'
    assert run['elapsed_ns'] > 0
    assert sum(d['elapsed_ns'] for d in run['documents']) <= run['elapsed_ns']
    for d in run['documents']:
        assert d['ok'] == (d['error'] is None)
        if not d['ok']:
            assert (directory/(d['document_id']+'.md')).read_bytes() == b'', 'Stale output on failure'
    return {p.name: digest(p) for p in sorted(directory.glob('*.md'))}


class CorpusWorker:
    def __init__(self, argv, stderr, env):
        self.log = stderr.open('w')
        self.p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=self.log, cwd=ROOT, env=env)
        self.pending = b''
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.p.stdout, selectors.EVENT_READ)

    def receive(self, timeout=3600):
        deadline = time.monotonic()+timeout
        while b'\n' not in self.pending:
            remaining = deadline-time.monotonic()
            if remaining <= 0:
                raise TimeoutError('Worker response deadline exceeded; run is incomplete')
            if not self.selector.select(min(remaining, 1.0)):
                continue
            chunk = os.read(self.p.stdout.fileno(), 1 << 20)
            if not chunk:
                raise RuntimeError(f'Worker exited early ({self.p.poll()})')
            self.pending += chunk
        line, self.pending = self.pending.split(b'\n', 1)
        return json.loads(line)

    def call(self, request):
        self.p.stdin.write((json.dumps(request)+'\n').encode())
        self.p.stdin.flush()
        return self.receive()

    def close(self):
        self.p.stdin.close()
        try:
            code = self.p.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.p.kill()
            self.p.wait()
            code = -9
        self.selector.close()
        self.p.stdout.close()
        self.log.close()
        return code
