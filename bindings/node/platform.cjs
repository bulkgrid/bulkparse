'use strict';
const fs = require('node:fs');
const path = require('node:path');

const platforms = {
  'linux-x64-gnu': { os: 'linux', cpu: 'x64', libc: 'glibc' },
  'linux-arm64-gnu': { os: 'linux', cpu: 'arm64', libc: 'glibc' },
  'darwin-x64': { os: 'darwin', cpu: 'x64' },
  'darwin-arm64': { os: 'darwin', cpu: 'arm64' },
  'win32-x64-msvc': { os: 'win32', cpu: 'x64' },
};

function platformKey(os = process.platform, cpu = process.arch, glibc = process.report?.getReport().header.glibcVersionRuntime) {
  const key = `${os}-${cpu}${os === 'linux' ? '-gnu' : os === 'win32' ? '-msvc' : ''}`;
  if (!platforms[key] || (os === 'linux' && !glibc)) {
    throw new Error(`No BulkParse prebuilt binary for ${os}/${cpu}${os === 'linux' && !glibc ? ' (musl)' : ''}. Build from source or use the WASM package.`);
  }
  return key;
}

function nativePath(filename) {
  const local = path.join(__dirname, filename);
  if (fs.existsSync(local)) return local;
  const name = `@bulkgrid/bulkparse-${platformKey()}`;
  let manifest;
  try { manifest = require.resolve(`${name}/package.json`); }
  catch (cause) {
    throw new Error(`Missing ${name}. Reinstall @bulkgrid/bulkparse with optional dependencies enabled, or build from source.`, { cause });
  }
  if (require(manifest).version !== require('./package.json').version) {
    throw new Error(`BulkParse binary version mismatch in ${name}. Reinstall matching package versions.`);
  }
  return path.join(path.dirname(manifest), filename);
}

module.exports = { platforms, platformKey, nativePath };
