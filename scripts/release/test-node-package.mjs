import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const require = createRequire(import.meta.url);
const { platformKey } = require('../../bindings/node/platform.cjs');
const version = require('../../bindings/node/package.json').version;
const artifacts = path.resolve(process.argv[2] ?? path.join(root, 'dist/release'));
const shared = path.resolve(process.argv[3] ?? artifacts);
const temporary = mkdtempSync(path.join(tmpdir(), 'bulkparse-install-'));
assert.ok(process.env.npm_execpath, 'Run through npm run release:test');
try {
  const packageFile = name => {
    const file = `${name}-${version}.tgz`;
    const directory = [artifacts, shared].find(folder => readdirSync(folder).includes(file));
    assert.ok(directory, `Missing ${file}`);
    return path.join(directory, file);
  };
  execFileSync(process.execPath, [process.env.npm_execpath, 'install', '--offline', '--ignore-scripts',
    '--omit=optional', '--no-audit', '--no-fund', '--prefix', temporary,
    packageFile(`bulkgrid-bulkparse-${platformKey()}`), packageFile('bulkgrid-bulkparse')], {
    stdio: 'inherit', env: { ...process.env, npm_config_cache: path.join(temporary, 'cache') },
  });
  const installed = path.join(temporary, 'node_modules/@bulkgrid/bulkparse');
  // Exit the addon process before cleanup: Windows locks loaded native libraries.
  execFileSync(process.execPath, ['--input-type=module', '-e', `
    import assert from 'node:assert/strict';
    import { readFileSync } from 'node:fs';
    import { createRequire } from 'node:module';
    const api = createRequire(import.meta.url)(process.argv[1]);
    const result = await api.parse(Buffer.from('# Release check\\n\\nHello.'), { filename: 'check.md' });
    assert.match(result.markdown, /Release check/);
    const docx = await api.parse(readFileSync(process.argv[2]), { filename: 'hello.docx' });
    assert.match(docx.markdown, /Hello BulkParse/);
  `, installed, path.join(root, 'fixtures/hello.docx')], { stdio: 'inherit' });
  const cli = path.join(installed, 'bin/bulkparse.cjs');
  assert.match(execFileSync(process.execPath, [cli, '--version'], { encoding: 'utf8' }), new RegExp(version.replaceAll('.', '\\.')));
  const stdin = spawnSync(process.execPath, [cli, 'parse', '-', '--format', 'md'], { input: '# From stdin', encoding: 'utf8' });
  assert.equal(stdin.status, 0, stdin.stderr);
  assert.match(stdin.stdout, /From stdin/);
  const bad = spawnSync(process.execPath, [cli, 'not-a-command'], { encoding: 'utf8' });
  assert.notEqual(bad.status, 0, 'CLI must propagate errors');
  const missing = spawnSync(process.execPath, [path.join(installed, 'bin/setup-ocr.mjs')], { encoding: 'utf8' });
  assert.match(missing.stderr, /--output/, 'OCR setup utility is installed');
  const platformManifest = path.join(temporary, `node_modules/@bulkgrid/bulkparse-${platformKey()}/package.json`);
  const manifest = JSON.parse(readFileSync(platformManifest));
  writeFileSync(platformManifest, JSON.stringify({ ...manifest, version: '0.0.0' }));
  const mismatch = spawnSync(process.execPath, [cli, '--version'], { encoding: 'utf8' });
  assert.notEqual(mismatch.status, 0);
  assert.match(mismatch.stderr, /version mismatch/);
  console.log('Packed Node addon, DOCX parsing, CLI, OCR setup and version guard passed without Rust or install scripts.');
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
