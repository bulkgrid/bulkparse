import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { installRuntime } from './provision_native_ocr.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
function fixtures(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'bulkparse-ocr-setup-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const archives = new Map();
  const specs = ['primary', 'fallback'].map(name => {
    const dir = path.join(root, name);
    const library = `${name}/lib/runtime.bin`;
    mkdirSync(path.join(dir, name, 'lib'), { recursive: true });
    const bytes = Buffer.from(`${name} native library fixture`);
    writeFileSync(path.join(dir, library), bytes);
    writeFileSync(path.join(dir, name, 'install.js'), 'throw new Error("must not execute");');
    const archive = path.join(root, `${name}.tgz`);
    execFileSync('tar', ['czf', archive, '-C', dir, name]);
    const body = readFileSync(archive);
    const url = `https://example.test/${name}.tgz`;
    archives.set(url, body);
    return { name: `${name}.tgz`, url, sha256: sha(body), library, librarySha256: sha(bytes) };
  });
  return { root: path.join(root, 'output'), specs, archives };
}

for (const useFallback of [false, true]) test(`installs verified ${useFallback ? 'fallback after 404' : 'primary'} archive`, async t => {
  const { root, specs: [primary, fallback], archives } = fixtures(t);
  const requested = [];
  t.mock.method(globalThis, 'fetch', async url => {
    requested.push(url);
    return useFallback && url === primary.url ? new Response('', { status: 404 }) : new Response(archives.get(url));
  });
  const result = await installRuntime(root, primary, fallback);
  const expected = useFallback ? fallback : primary;
  assert.equal(readFileSync(result.library, 'utf8'), `${useFallback ? 'fallback' : 'primary'} native library fixture`);
  assert.deepEqual(result.source, { url: expected.url, sha256: expected.sha256 });
  assert.deepEqual(requested, useFallback ? [primary.url, fallback.url] : [primary.url]);
  if (useFallback) assert.equal(existsSync(path.join(root, 'runtime/ort/fallback/install.js')), false);
});

test('verified cached primary does not use the network', async t => {
  const { root, specs: [primary, fallback], archives } = fixtures(t);
  mkdirSync(path.join(root, 'downloads'), { recursive: true });
  writeFileSync(path.join(root, 'downloads', primary.name), archives.get(primary.url));
  t.mock.method(globalThis, 'fetch', () => { throw new Error('unexpected network call'); });
  const result = await installRuntime(root, primary, fallback);
  assert.equal(result.source.url, primary.url);
});

for (const corrupt of ['primary', 'fallback']) test(`rejects corrupt ${corrupt} without extracting it`, async t => {
  const { root, specs: [primary, fallback] } = fixtures(t);
  const requested = [];
  t.mock.method(globalThis, 'fetch', async url => {
    requested.push(url);
    if (corrupt === 'fallback' && url === primary.url) return new Response('', { status: 404 });
    return new Response('incorrect bytes');
  });
  await assert.rejects(installRuntime(root, primary, fallback), /fingerprint mismatch/);
  assert.equal(existsSync(path.join(root, 'runtime/ort')), false);
  if (corrupt === 'primary') assert.deepEqual(requested, [primary.url]);
});

test('other HTTP errors do not silently select another source', async t => {
  const { root, specs: [primary, fallback] } = fixtures(t);
  const requested = [];
  t.mock.method(globalThis, 'fetch', async url => { requested.push(url); return new Response('', { status: 403 }); });
  await assert.rejects(installRuntime(root, primary, fallback), /Download failed: 403/);
  assert.deepEqual(requested, [primary.url]);
});

test('standalone and symlinked setup entry points still validate arguments', t => {
  const root = mkdtempSync(path.join(tmpdir(), 'bulkparse-ocr-entry-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const script = fileURLToPath(new URL('./provision_native_ocr.mjs', import.meta.url));
  const link = path.join(root, 'setup-ocr.mjs');
  symlinkSync(script, link);
  for (const entry of [script, link]) {
    const result = spawnSync(process.execPath, [entry], { encoding: 'utf8' });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Usage: provision_native_ocr/);
  }
});
