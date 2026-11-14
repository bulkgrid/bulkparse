#!/usr/bin/env node
// Export the native binding's build inputs for a private monorepo consumer.
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = process.argv[2];
if (!output) throw new Error('Usage: node scripts/export_node_source.mjs OUTPUT_DIRECTORY');
const destination = path.resolve(output);
const stage = await mkdtemp(path.join(tmpdir(), 'bulkparse-source-'));
try {
  for (const relative of ['Cargo.toml', 'Cargo.lock', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'crates/bulkparse', 'crates/bulkparse-ocr-native', 'bindings/node', 'scripts/provision_native_ocr.mjs']) {
    await mkdir(path.dirname(path.join(stage, relative)), { recursive: true });
    await cp(path.join(root, relative), path.join(stage, relative), { recursive: true, filter: file => !file.split(path.sep).some(part => ['target','node_modules','.venv','__pycache__'].includes(part)) && !file.endsWith('.node') });
  }
  const cargoPath = path.join(stage, 'Cargo.toml');
  let cargo = await readFile(cargoPath, 'utf8');
  cargo = cargo.replace(/^members = .*$/m, 'members = ["crates/bulkparse", "crates/bulkparse-ocr-native", "bindings/node"]')
    .replace(/^default-members = .*$/m, 'default-members = ["crates/bulkparse"]')
    .replace(/# wasm-bindgen[^]*$/, '');
  await writeFile(cargoPath, cargo);
  execFileSync('cargo', ['generate-lockfile', '--offline'], { cwd: stage, stdio: 'inherit' });
  await mkdir(destination, { recursive: true });
  const archive = path.join(destination, 'bulkparse-source.tar.gz');
  execFileSync('tar', ['czf', archive, '-C', stage, '.'], { stdio: 'inherit' });
  const commit = execFileSync('git', ['rev-parse','HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  const dirty = execFileSync('git', ['status','--porcelain'], { cwd: root, encoding: 'utf8' }).trim().length > 0;
  await writeFile(path.join(destination, 'source.json'), JSON.stringify({ repository: 'https://github.com/bulkgrid/bulkparse', base_commit: commit, includes_uncommitted_changes: dirty, sha256: createHash('sha256').update(await readFile(archive)).digest('hex') }, null, 2) + '\n');
} finally { await rm(stage, { recursive: true, force: true }); }
