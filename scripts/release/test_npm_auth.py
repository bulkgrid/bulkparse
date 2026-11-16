import contextlib
import importlib.util
import io
import os
from pathlib import Path
import unittest
from unittest.mock import patch
import urllib.error

spec = importlib.util.spec_from_file_location('npm_auth', Path(__file__).with_name('check-npm-auth.py'))
auth = importlib.util.module_from_spec(spec)
spec.loader.exec_module(auth)

ENV = {
    'GITHUB_REPOSITORY': 'bulkgrid/bulkparse',
    'GITHUB_WORKFLOW_REF': 'bulkgrid/bulkparse/.github/workflows/release.yml@refs/heads/main',
    'ACTIONS_ID_TOKEN_REQUEST_URL': 'https://example.invalid/token?job=1',
    'ACTIONS_ID_TOKEN_REQUEST_TOKEN': 'secret-request-token',
}


class NpmAuthTests(unittest.TestCase):
    def test_wrong_workflow_or_missing_permission_never_contacts_registry(self):
        for changes in ({'GITHUB_REPOSITORY': 'other/repo'},
                        {'GITHUB_WORKFLOW_REF': 'bulkgrid/bulkparse/.github/workflows/other.yml@main'},
                        {'ACTIONS_ID_TOKEN_REQUEST_TOKEN': ''}):
            with self.subTest(changes=changes), patch.dict(os.environ, ENV | changes, clear=True), \
                    patch.object(auth, 'request_json') as request:
                with self.assertRaises(RuntimeError):
                    auth.verify()
                request.assert_not_called()

    def test_exchanges_for_all_packages_without_upload_or_token_logging(self):
        calls = []

        def respond(request, label):
            calls.append(request)
            return {'value': 'secret-identity'} if label == 'GitHub identity' else {'token': 'secret-npm'}

        output = io.StringIO()
        with patch.dict(os.environ, ENV, clear=True), patch.object(auth, 'request_json', side_effect=respond), \
                contextlib.redirect_stdout(output):
            auth.verify()
        self.assertEqual(len(calls), 8)
        self.assertIn('audience=npm%3Aregistry.npmjs.org', calls[0].full_url)
        for request in calls[1:]:
            self.assertEqual(request.method, 'POST')
            self.assertIn('/-/npm/v1/oidc/token/exchange/package/', request.full_url)
            self.assertIsNone(request.data)
        self.assertNotIn('secret-', output.getvalue())

    def test_one_missing_publisher_fails_but_checks_the_other_packages(self):
        def respond(request, label):
            if label == 'GitHub identity':
                return {'value': 'secret-identity'}
            if label == '@bulkgrid/bulkparse-wasm':
                raise RuntimeError(f'{label}: HTTP 404')
            return {'token': 'secret-npm'}

        with patch.dict(os.environ, ENV, clear=True), patch.object(auth, 'request_json', side_effect=respond) as request, \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, '1 of 7'):
                auth.verify()
        self.assertEqual(request.call_count, 8)

    def test_registry_errors_do_not_disclose_response_body(self):
        error = urllib.error.HTTPError('https://example.invalid', 403, 'secret-body', {}, None)
        with patch.object(auth.urllib.request, 'urlopen', side_effect=error):
            with self.assertRaisesRegex(RuntimeError, '^npm: HTTP 403$'):
                auth.request_json(urllib.request.Request('https://example.invalid'), 'npm')


if __name__ == '__main__':
    unittest.main()
