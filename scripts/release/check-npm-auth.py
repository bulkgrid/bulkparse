"""Verify npm's OIDC token exchange without uploading or changing any package."""
import json
import os
from pathlib import Path
import urllib.error
import urllib.parse
import urllib.request

PACKAGES = (
    '@bulkgrid/bulkparse', '@bulkgrid/bulkparse-wasm',
    '@bulkgrid/bulkparse-linux-x64-gnu', '@bulkgrid/bulkparse-linux-arm64-gnu',
    '@bulkgrid/bulkparse-darwin-x64', '@bulkgrid/bulkparse-darwin-arm64',
    '@bulkgrid/bulkparse-win32-x64-msvc',
)


def request_json(request, label):
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        # Do not log response bodies, headers or credentials.
        raise RuntimeError(f'{label}: HTTP {error.code}') from None
    except (urllib.error.URLError, ValueError, TimeoutError):
        raise RuntimeError(f'{label}: request failed') from None


def verify():
    if (os.environ.get('GITHUB_REPOSITORY') != 'bulkgrid/bulkparse'
            or not os.environ.get('GITHUB_WORKFLOW_REF', '').startswith(
                'bulkgrid/bulkparse/.github/workflows/release.yml@')):
        raise RuntimeError('Run this check from bulkgrid/bulkparse release.yml')
    url = os.environ.get('ACTIONS_ID_TOKEN_REQUEST_URL')
    credential = os.environ.get('ACTIONS_ID_TOKEN_REQUEST_TOKEN')
    if not url or not credential:
        raise RuntimeError('GitHub job needs id-token: write')
    separator = '&' if '?' in url else '?'
    url += separator + urllib.parse.urlencode({'audience': 'npm:registry.npmjs.org'})
    identity = request_json(urllib.request.Request(url, headers={
        'Authorization': f'Bearer {credential}', 'Accept': 'application/json',
    }), 'GitHub identity')
    token = identity.get('value')
    if not isinstance(token, str) or not token:
        raise RuntimeError('GitHub returned no identity token')
    failures = []
    for name in PACKAGES:
        endpoint = 'https://registry.npmjs.org/-/npm/v1/oidc/token/exchange/package/'
        endpoint += urllib.parse.quote(name, safe='@')
        try:
            result = request_json(urllib.request.Request(endpoint, method='POST', headers={
                'Authorization': f'Bearer {token}', 'Accept': 'application/json',
            }), name)
            if not isinstance(result.get('token'), str) or not result['token']:
                raise RuntimeError(f'{name}: npm returned no scoped token')
            print(f'{name}: GitHub authentication accepted', flush=True)
            del result
        except RuntimeError as error:
            failures.append(str(error))
            print(str(error), flush=True)
    if failures:
        raise RuntimeError(f'npm authentication failed for {len(failures)} of {len(PACKAGES)} packages')
    summary = ('GitHub-to-npm authentication accepted for all seven packages. '
               'No package was uploaded or modified. This checks authentication; '
               'new trusted publishers still require a successful publish within npm\'s activation window.\n')
    print(summary, end='')
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with Path(os.environ['GITHUB_STEP_SUMMARY']).open('a') as output:
            output.write(summary)


if __name__ == '__main__':
    try:
        verify()
    except RuntimeError as error:
        print(str(error))
        raise SystemExit(1) from None
