import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

if importlib.util.find_spec('psutil'):
    from benchmark_workers import run_pool,percentile

FAKE='''import json,os,sys,time
print(json.dumps({'ready':True,'pid':os.getpid()}),flush=True)
for line in sys.stdin:
 r=json.loads(line)
 if 'slow' in r['input']:time.sleep(2)
 ok='bad' not in r['input']
 print(json.dumps({'id':r['id'],'ok':ok,'markdown':'same text' if ok else '', 'service_ms':.1}),flush=True)
'''


@unittest.skipUnless(importlib.util.find_spec('psutil'),'worker benchmark extra: psutil')
class WorkerTests(unittest.TestCase):
    def setup_pool(self,root):
        worker=root/'fake.py';worker.write_text(FAKE)
        return [sys.executable,str(worker)]

    def test_persistent_reuse_balanced_dispatch_and_failure_denominator(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);argv=self.setup_pool(root)
            cases=[{'id':'good','source_path':str(root/'good')},{'id':'bad','source_path':str(root/'bad')}]
            result,outputs=run_pool(argv,cases,2,root/'run',duration=.05,max_jobs=100,
                min_available_bytes=0,max_rss_bytes=1024**3)
            self.assertEqual(result['status'],'completed')
            self.assertEqual(len(set(result['pids'])),2)
            self.assertGreater(result['successful_documents'],2)
            self.assertEqual(result['successful_documents'],result['failed_documents'])
            self.assertAlmostEqual(result['attempts_per_second'],2*result['documents_per_second'])
            self.assertEqual(result['nondeterministic_cases'],[])
            self.assertTrue(result['workers_reaped'])
            self.assertFalse(outputs['bad']['ok'])
            samples=[json.loads(s) for s in (root/'run/samples.jsonl').read_text().splitlines()]
            self.assertEqual({s['worker'] for s in samples},{0,1})

    def test_memory_guard_aborts_and_reaps(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            result,_=run_pool(self.setup_pool(root),[{'id':'good','source_path':str(root/'good')}],1,
                root/'run',duration=.01,min_available_bytes=0,max_rss_bytes=1)
            self.assertEqual(result['status'],'aborted')
            self.assertEqual(result['abort_reason'],'pool_rss_budget')
            self.assertNotIn('documents_per_second',result)
            self.assertTrue(result['workers_reaped'])

    def test_job_timeout_reaps_worker_and_does_not_report_fast_throughput(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            result,_=run_pool(self.setup_pool(root),[{'id':'good','source_path':str(root/'good')},
                {'id':'slow','source_path':str(root/'slow')}],1,root/'run',duration=.01,
                job_timeout=.05,min_available_bytes=0,max_rss_bytes=1024**3)
            self.assertEqual(result['status'],'aborted')
            self.assertEqual(result['abort_reason'],'response_timeout')
            self.assertTrue(result['workers_reaped'])

    def test_nearest_rank_p95(self):
        self.assertEqual(percentile(list(range(1,101)),.95),95)


if __name__=='__main__':unittest.main()
