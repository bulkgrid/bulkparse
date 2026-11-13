"""Failure accounting and prediction-integrity tests for the comparison harness."""
from pathlib import Path
import tempfile
import unittest
from benchmark_same_machine import validate_run


class Accounting(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name);self.paths=[Path('a.pdf'),Path('b.pdf')]
        (self.root/'a.md').write_text('native text');(self.root/'b.md').write_text('')
        self.run={'elapsed_ns':100,'documents':[
            {'document_id':'a','elapsed_ns':40,'ok':True,'error':None},
            {'document_id':'b','elapsed_ns':30,'ok':False,'error':'needs OCR'}]}
    def test_failure_retained_with_empty_prediction(self):
        self.assertEqual(len(validate_run(self.run,self.paths,self.root)),2)
    def test_stale_failure_output_rejected(self):
        (self.root/'b.md').write_text('stale successful output')
        with self.assertRaises(AssertionError):validate_run(self.run,self.paths,self.root)
    def test_missing_outcome_rejected(self):
        self.run['documents'].pop()
        with self.assertRaises(AssertionError):validate_run(self.run,self.paths,self.root)
    def test_duplicate_outcome_rejected(self):
        self.run['documents'][1]['document_id']='a'
        with self.assertRaises(AssertionError):validate_run(self.run,self.paths,self.root)
    def test_missing_prediction_rejected(self):
        (self.root/'b.md').unlink()
        with self.assertRaises(AssertionError):validate_run(self.run,self.paths,self.root)
    def test_extra_prediction_rejected(self):
        (self.root/'extra.md').write_text('unexpected')
        with self.assertRaises(AssertionError):validate_run(self.run,self.paths,self.root)


if __name__=='__main__':unittest.main()
