import json
import os
import sys
import tempfile
from pathlib import Path
import time
import unittest
import psutil
from evaluate_ocr_profiles import aggregate,measure,score


class Scores(unittest.TestCase):
    def test_accents_are_not_repaired_and_whitespace_is_normalized(self):
        self.assertEqual(score('Åsa  går\nhem.','Åsa går hem.')['character_errors'],0)
        self.assertEqual(score('Asa går hem.','Åsa går hem.')['character_errors'],1)
        self.assertFalse(score('asa går hem.','Åsa går hem.')['exact_after_whitespace'])
        self.assertEqual(score('','expected')['character_error_rate'],1)

    def test_failure_cost_and_reference_tiers_are_retained(self):
        base={'profile':'small','language':'en','id':'a','track':'authored_stress',
              'exit_code':2,'harness_error':None,'diagnostic_codes':[],
              'quality':score('cat','cat'),'wall_seconds':1.,'sampled_peak_rss_bytes':1024**2}
        rows=[base,{**base,'id':'b','exit_code':1,'quality':score('','dog'),'wall_seconds':3.},
              {**base,'track':'external_smoke','quality':None}]
        groups=aggregate(rows)
        authored=next(g for g in groups if g['track']=='authored_stress')
        self.assertEqual(authored['conversion_failures'],1)
        self.assertEqual(authored['weighted_cer'],.5)
        self.assertEqual(authored['median_wall_seconds'],2.)
        self.assertIsNone(next(g for g in groups if g['track']=='external_smoke')['weighted_cer'])


@unittest.skipUnless(os.name=='posix','POSIX process groups')
class ProcessMeasurements(unittest.TestCase):
    def test_normal_result_and_failure_are_distinguished(self):
        for code in [0,2,7]:
            result,out,_=measure([sys.executable,'-c',f'import sys;print("result");sys.exit({code})'])
            self.assertEqual(result['exit_code'],code)
            self.assertEqual(out,b'result\n')
            self.assertIsNone(result['harness_error'])

    def test_timeout_cleans_up_child_and_counts_tree_memory(self):
        with tempfile.TemporaryDirectory() as temp:
            pid_file=Path(temp)/'pids.jsonl'
            source='import subprocess,sys,time,os,json; p=subprocess.Popen([sys.executable,"-c","import time;time.sleep(60)"]);open(sys.argv[1],"w").write(json.dumps({"pid":p.pid,"parent":os.getpid()})+"\\n");print(p.pid,flush=True);time.sleep(60)'
            result,out,_=measure([sys.executable,'-c',source,str(pid_file)],pid_file=pid_file,timeout=1)
        self.assertEqual(result['harness_error'],'wall_timeout')
        self.assertGreater(result['observed_backend_processes'],0)
        self.assertGreater(result['sampled_peak_rss_bytes'],0)
        pid=int(out);deadline=time.monotonic()+2
        while psutil.pid_exists(pid) and psutil.Process(pid).status()!=psutil.STATUS_ZOMBIE:
            self.assertLess(time.monotonic(),deadline);time.sleep(.01)

    def test_output_and_memory_guards_stop_work(self):
        result,out,_=measure([sys.executable,'-c','import sys,time;print("x"*20000,flush=True);time.sleep(60)'],max_output=1000)
        self.assertEqual(result['harness_error'],'output_budget');self.assertEqual(out,b'')
        result,_,_=measure([sys.executable,'-c','import time;time.sleep(60)'],max_rss=1)
        self.assertEqual(result['harness_error'],'sampled_rss_budget')


if __name__=='__main__':unittest.main()
