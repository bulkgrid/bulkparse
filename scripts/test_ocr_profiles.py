"""Manifest/profile tests require no installed OCR engine or model downloads."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'backends/rapidocr'))
from configure import configure
from profiles import DETECTION, ORIENTATION, PROFILES, profile_settings
from rapidocr_backend import model_paths


class ProfileTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name);self.models=self.root/'assets';self.models.mkdir()
        for item in [DETECTION,ORIENTATION,*PROFILES.values()]:
            data=item['filename'].encode();(self.models/item['filename']).write_bytes(data)
            mock=patch.dict(item,{'sha256':hashlib.sha256(data).hexdigest()})
            mock.start();self.addCleanup(mock.stop)

    def settings(self,name='multi-small'):
        return configure(self.root/name,name,model_directory=self.models)

    def test_profiles_pin_model_identity_and_parameters(self):
        for name,profile in PROFILES.items():
            config=self.settings(name)
            manifest=Path(json.loads(config.read_text())['models'])
            paths,params=model_paths(manifest)
            self.assertEqual(params,profile['params'])
            self.assertEqual(Path(paths['rec']).name,profile['filename'])
            self.assertEqual(json.loads(manifest.read_text())['profile'],name)

    def test_missing_or_wrong_model_does_not_create_settings(self):
        for name in ['missing','wrong']:
            file=self.root/(name+'.onnx')
            if name=='wrong':file.write_bytes(b'wrong model')
            with self.assertRaises(ValueError):configure(self.root/name,'latin-mobile',file,self.models)
            self.assertFalse((self.root/name).exists())

    def test_existing_settings_are_not_overwritten(self):
        path=self.settings();before=path.read_bytes()
        with self.assertRaises(ValueError):self.settings()
        self.assertEqual(path.read_bytes(),before)

    def test_bundled_settings_enable_bounded_model_reuse(self):
        config=json.loads(self.settings().read_text())
        self.assertTrue(config['reuse_process'])
        self.assertEqual(config['max_pages_per_worker'],128)
        self.assertIn('--release-unused-memory',config['args'])
        self.assertIn('--adaptive-detector',config['args'])
        self.assertNotIn('--detector-max-side',config['args'])

    def test_unbenchmarked_recognizers_keep_original_detection(self):
        for name in ['multi-medium','latin-mobile']:
            config=json.loads(self.settings(name).read_text())
            self.assertTrue(config['reuse_process'])
            self.assertNotIn('--adaptive-detector',config['args'])

    def test_changed_assets_fail_integrity_checks(self):
        config=self.settings()
        (self.models/DETECTION['filename']).write_bytes(b'tampered')
        with self.assertRaisesRegex(ValueError,'fingerprint'):model_paths(Path(json.loads(config.read_text())['models']))

    def test_legacy_manifest_keeps_legacy_parameters(self):
        config=self.settings();manifest=Path(json.loads(config.read_text())['models'])
        data=json.loads(manifest.read_text());data['version']=1;del data['profile']
        manifest.write_text(json.dumps(data))
        self.assertEqual(model_paths(manifest)[1],{})

    def test_unknown_or_mismatched_profile_and_parameter_injection_fail(self):
        config=self.settings();manifest=Path(json.loads(config.read_text())['models'])
        data=json.loads(manifest.read_text())
        for bad in [{**data,'profile':'unknown'},{**data,'profile':'latin-mobile'},
                    {**data,'params':{'Rec.model_path':'https://untrusted.invalid/model'}},
                    {**data,'version':3}]:
            with self.assertRaises(ValueError):profile_settings(bad)


if __name__=='__main__':unittest.main()
