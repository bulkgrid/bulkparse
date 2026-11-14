// Run through npm so npm_execpath also works on Windows without a command shell.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync, chmodSync, cpSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const require = createRequire(import.meta.url);
const { platforms, platformKey } = require('../../bindings/node/platform.cjs');
const root = fileURLToPath(new URL('../../', import.meta.url));
const pkg = JSON.parse(readFileSync(path.join(root, 'bindings/node/package.json')));
const output = path.join(root, 'dist/release');
const staging = path.join(root, 'target/release-packages');
const mode = process.argv[2];
if (!process.env.npm_execpath) throw new Error('Run through npm run release:pack');
mkdirSync(output, { recursive: true });
mkdirSync(staging, { recursive: true });

function pack(directory) {
  execFileSync(process.execPath, [process.env.npm_execpath, 'pack', '--ignore-scripts', '--pack-destination', output], {
    cwd: directory, stdio: 'inherit',
  });
}
function json(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`); }
function legal(directory) {
  for (const name of ['LICENSE', 'THIRD_PARTY_NOTICES.md']) copyFileSync(path.join(root, name), path.join(directory, name));
  cpSync(path.join(root, 'crates/bulkparse-ocr-native/licenses'), path.join(directory, 'licenses'), { recursive: true });
}

if (mode === 'native') {
  const key = platformKey();
  const platform = platforms[key];
  const directory = path.join(staging, key);
  mkdirSync(directory, { recursive: true });
  const executable = platform.os === 'win32' ? 'bulkparse.exe' : 'bulkparse';
  copyFileSync(path.join(root, 'bindings/node/bulkparse.node'), path.join(directory, 'bulkparse.node'));
  copyFileSync(path.join(root, 'target/release', executable), path.join(directory, executable));
  chmodSync(path.join(directory, executable), 0o755);
  legal(directory);
  copyFileSync(path.join(root, 'scripts/provision_native_ocr.mjs'), path.join(directory, 'setup-ocr.mjs'));
  json(path.join(directory, 'package.json'), {
    name: `@bulkgrid/bulkparse-${key}`, version: pkg.version,
    description: `BulkParse native addon and CLI for ${key}`, license: 'MIT',
    repository: pkg.repository, os: [platform.os], cpu: [platform.cpu],
    ...(platform.libc ? { libc: [platform.libc] } : {}),
    files: ['bulkparse.node', executable, 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'licenses'],
    publishConfig: { access: 'public' },
  });
  pack(directory);
  // Standalone CLI archive also carries the optional OCR setup utility.
  execFileSync('tar', ['czf', path.join(output, `bulkparse-${pkg.version}-${key}.tar.gz`), '-C', directory,
    executable, 'setup-ocr.mjs', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'licenses'], { stdio: 'inherit' });
} else if (mode === 'shared') {
  const directory = path.join(staging, 'node');
  mkdirSync(path.join(directory, 'bin'), { recursive: true });
  for (const name of ['index.cjs', 'index.d.ts', 'platform.cjs', 'README.md']) {
    copyFileSync(path.join(root, 'bindings/node', name), path.join(directory, name));
  }
  copyFileSync(path.join(root, 'bindings/node/bin/bulkparse.cjs'), path.join(directory, 'bin/bulkparse.cjs'));
  copyFileSync(path.join(root, 'scripts/provision_native_ocr.mjs'), path.join(directory, 'bin/setup-ocr.mjs'));
  legal(directory);
  const manifest = { ...pkg, scripts: undefined, optionalDependencies: Object.fromEntries(
    Object.keys(platforms).map(key => [`@bulkgrid/bulkparse-${key}`, pkg.version]),
  ) };
  json(path.join(directory, 'package.json'), manifest);
  pack(directory);
  // The build step has already checked and prepared the generated WASM files.
  pack(path.join(root, 'bindings/wasm'));
} else {
  throw new Error('Expected native or shared');
}
