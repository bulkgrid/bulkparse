import {execFileSync} from 'node:child_process';
import {mkdtemp, readFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';
import assert from 'node:assert/strict';

const cwd = fileURLToPath(new URL('..', import.meta.url));
const temporary = await mkdtemp(join(tmpdir(), 'bulkparse-wasm-package-'));
const npm = args => execFileSync(process.execPath, [process.env.npm_execpath, ...args], {
  cwd, encoding: 'utf8', env: {...process.env, npm_config_cache: join(temporary, 'cache')},
});
assert.ok(process.env.npm_execpath, 'Run this test with npm run test:package');
try {
  const [packed] = JSON.parse(npm(['pack', '--silent', '--json', '--pack-destination', temporary]));
  const names = new Set(packed.files.map(file => file.path));
  for (const name of ['index.js', 'index.d.ts', 'LICENSE', 'pkg/bulkparse_wasm.js', 'pkg/bulkparse_wasm.d.ts', 'pkg/bulkparse_wasm_bg.wasm']) {
    assert.ok(names.has(name), `Package is missing ${name}`);
  }
  npm(['install', '--prefix', temporary, '--offline', '--ignore-scripts', '--no-audit', '--no-fund', join(temporary, packed.filename)]);
  const installed = join(temporary, 'node_modules/@bulkgrid/bulkparse-wasm');
  const {init, parse} = await import(pathToFileURL(join(installed, 'index.js')));
  await init({module_or_path: await readFile(join(installed, 'pkg/bulkparse_wasm_bg.wasm'))});
  const result = parse(await readFile(new URL('../../../fixtures/hello.docx', import.meta.url)), {filename: 'hello.docx'});
  assert.equal(result.document.format, 'docx');
  assert.equal(result.status, 'complete');
  assert.match(result.markdown, /# Hello BulkParse/);
  console.log('Packed WASM package installs offline and converts DOCX');
} finally {
  await rm(temporary, {recursive: true, force: true});
}
