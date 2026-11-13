"""Exercise the real worker framing and lifecycle without loading OCR models."""
import argparse
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'backends/rapidocr'))
import rapidocr_backend as backend


class WorkerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.pdf = Path(self.tmp.name)/'input.pdf'
        self.pdf.write_bytes(b'%PDF-test')
        self.args = argparse.Namespace(models=Path(self.tmp.name)/'models.json',
            dpi=200, max_pixels=8_000_000, max_input_bytes=32*1024**2,
            enable_cpu_mem_arena=False, detector_max_side=0, adaptive_detector=False, release_unused_memory=True)

    def request(self, ident, page=1, limit=1024):
        return {'version':1, 'id':ident, 'input':str(self.pdf),
                'page':page, 'max_output_bytes':limit}

    def serve(self, requests, recognize):
        data=''.join(json.dumps(r)+'\n' for r in requests).encode()
        output=io.StringIO()
        with patch.object(sys, 'stdin', io.TextIOWrapper(io.BytesIO(data))), \
             patch.object(backend, 'Runtime') as runtime, \
             patch.object(backend, 'recognize', side_effect=recognize), \
             contextlib.redirect_stdout(output):
            backend.serve(self.args)
        return [json.loads(line) for line in output.getvalue().splitlines()], runtime

    def test_one_runtime_handles_distinct_requests_and_trims_after_each(self):
        pages=[]
        def recognize(args, runtime):
            pages.append(args.page)
            return {'image_size':[100,100], 'lines':[]}
        replies,runtime=self.serve([self.request(1),self.request(2,2)],recognize)
        self.assertEqual(pages,[1,2])
        runtime.assert_called_once_with(self.args.models,False,0,False)
        self.assertEqual(runtime.return_value.release_unused_memory.call_count,2)
        self.assertEqual([r['id'] for r in replies],[1,2])
        self.assertEqual([r['response']['page'] for r in replies],[1,2])

    def test_budget_failure_never_initializes_models_and_stops_worker(self):
        replies,runtime=self.serve([self.request(1,limit=0),self.request(2)],lambda *_:self.fail())
        runtime.assert_not_called()
        self.assertEqual(len(replies),1)
        self.assertIn('error',replies[0]['response'])

    def test_inference_failure_returns_bounded_error_then_stops(self):
        def fail(*_):raise RuntimeError('x'*1000)
        replies,runtime=self.serve([self.request(1),self.request(2)],fail)
        self.assertEqual(len(replies),1)
        self.assertEqual(len(replies[0]['response']['error']['message']),512)
        runtime.return_value.release_unused_memory.assert_called_once()

    def test_invalid_frame_is_rejected_before_models_load(self):
        for bad in [dict(self.request(1),id=True),dict(self.request(1),unexpected=1)]:
            with self.assertRaises(ValueError):self.serve([bad],lambda *_:self.fail())


class AdaptiveDetectorTests(unittest.TestCase):
    def run_detector(self, boxes, shape=(2000,1500), fail_retry=False):
        class Detector:
            limit_type='min'
            limit_side_len=736
            def __init__(self):self.calls=[]
            def __call__(self, pixels):
                self.calls.append((self.limit_type,self.limit_side_len))
                if len(self.calls)==2:
                    if fail_retry:raise RuntimeError('inference failed')
                    return 'original-resolution-result'
                return SimpleNamespace(boxes=boxes)
        detector=Detector();wrapper=backend.AdaptiveDetector(detector)
        try:result=wrapper(SimpleNamespace(shape=shape))
        finally:
            self.assertEqual((detector.limit_type,detector.limit_side_len),('min',736))
        return result,detector.calls

    def test_large_text_uses_one_pass(self):
        _,calls=self.run_detector([[[0,0],[300,0],[300,50],[0,50]]])
        self.assertEqual(calls,[('max',960)])

    def test_sparse_small_rotated_and_empty_detections_retry(self):
        for boxes in [None,[],[[[0,0],[300,0],[300,20],[0,20]]],
                      [[[0,0],[20,0],[20,300],[0,300]]]]:
            result,calls=self.run_detector(boxes)
            self.assertEqual(result,'original-resolution-result')
            self.assertEqual(calls,[('max',960),('min',736)])

    def test_small_images_do_not_retry_identical_resolution(self):
        _,calls=self.run_detector(None,shape=(800,600))
        self.assertEqual(len(calls),1)

    def test_retry_failure_restores_detector_configuration(self):
        with self.assertRaises(RuntimeError):self.run_detector(None,fail_retry=True)


if __name__=='__main__':unittest.main()
