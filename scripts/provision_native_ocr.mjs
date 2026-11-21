#!/usr/bin/env node
// Explicit build/setup operation. The parser itself never downloads artifacts.
import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream, realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { access, mkdir, readdir, rename, rm, writeFile } from 'node:fs/promises';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';

const release = 'https://github.com/GreatV/oar-ocr/releases/download/v0.3.0';
const rapid = 'https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.10.0/onnx/PP-OCRv6';
const models = {
  det: ['PP-OCRv6_det_small.onnx', `${rapid}/det/PP-OCRv6_det_small.onnx`, '090f04abcd9d9a7498bc4ebf677e4cb9bdce1fe4197ddb7e529f1ef44e1ff94f'],
  rec: ['PP-OCRv6_rec_small.onnx', `${rapid}/rec/PP-OCRv6_rec_small.onnx`, '6f327246b50388f3c176ae304bd95767ea6dc0c9ae92153ef8cbe210b3c14884'],
  cls: ['pp-lcnet_x0_25_textline_ori.onnx', `${release}/pp-lcnet_x0_25_textline_ori.onnx`, 'fb402220f39b183d64a68cd48d4bd53267a21354b6fc39370c2a83fbdad85b10'],
  layout: ['pp-doclayout_plus-l.onnx', `${release}/pp-doclayout_plus-l.onnx`, 'b06cedc7ab3cca7da4ed66cf16024732149d2c29e6adcbfc69b9bb6ef94b4a48'],
  table: ['slanet_plus.onnx', `${release}/slanet_plus.onnx`, '3a96a71719247c5d94992fca31266b598c54740388de371f0c75077e2a9e0b55'],
  table_dictionary: ['table_structure_dict_ch.txt', `${release}/table_structure_dict_ch.txt`, '68d344a84b726e043f390122240ff2b2ced2949b2a80ce9b61ae955054d190ef'],
};
const runtimes = {
  'linux-x64': {
    ort: ['onnxruntime-linux-x64-1.31.1.tgz', 'c849e36a1ef3e22d4ff217b9d1ce4a3cf5b9467b975610913ae959ad5b16bdc9'],
    pdfium: ['pdfium-linux-x64.tgz', '384ab5c51882408a143a21f9c14dcd2caf1f75280ae41c554d0489b46c3dba9f'],
  },
  'linux-arm64': {
    ort: ['onnxruntime-linux-aarch64-1.31.1.tgz', '5317505e2da99be65f8e2775ba3c2afd6d2961fd78aba1bd8f56159f48820c78'],
    pdfium: ['pdfium-linux-arm64.tgz', 'c6ed1a91234aede0cfafdc09c401b83bd44ac69deafc4599fda75d511c36403e'],
  },
  'darwin-arm64': {
    ort: ['onnxruntime-osx-arm64-1.31.1.tgz', '2e1344a1926ee50eb25b38153cb6992c3567515aef0e677a1a520a1798896d6f'],
    pdfium: ['pdfium-mac-arm64.tgz', '30b0c3f55448840f0947b54bedbe35185bbaccbadd35b8cd960270d6fa6caf80'],
  },
};

async function fingerprint(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}
export async function download(file, url, expected) {
  try { if (await fingerprint(file) === expected) return; }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  await mkdir(path.dirname(file), { recursive: true });
  const temporary = `${file}.download`;
  const response = await fetch(url, { signal: AbortSignal.timeout(300_000) });
  if (!response.ok || !response.body) {
    await response.body?.cancel();
    throw Object.assign(new Error(`Download failed: ${response.status} ${url}`), { status: response.status });
  }
  let bytes = 0;
  const limit = new Transform({ transform(chunk, _, callback) {
    bytes += chunk.length;
    callback(bytes > 512 * 1024 ** 2 ? new Error('Artifact exceeds download limit') : null, chunk);
  } });
  try {
    await pipeline(Readable.fromWeb(response.body), limit, createWriteStream(temporary));
    if (await fingerprint(temporary) !== expected) throw new Error(`Artifact fingerprint mismatch: ${url}`);
    await rename(temporary, file);
  } finally { await rm(temporary, { force: true }); }
}
async function findFile(directory, name) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.name === name) return file;
    if (entry.isDirectory()) { const found = await findFile(file, name); if (found) return found; }
  }
}

// Microsoft's npm archive contains the same pinned runtime version. Verify the
// whole archive and extract only the selected native library; never run npm or
// package lifecycle scripts. Keep the original GitHub pins for verified caches.
const npmRuntime = {
  name: 'onnxruntime-node-1.31.1.tgz',
  url: 'https://registry.npmjs.org/onnxruntime-node/-/onnxruntime-node-1.31.1.tgz',
  sha256: 'd7a3de2b1bba92614abc74af909cc0ba4f4dd5347735418794edeb58cfef2d22',
};
const npmLibraries = {
  'linux-x64': ['package/bin/napi-v6/linux/x64/libonnxruntime.so.1', 'f19a4da48c4c65e0cdccff82354482f657fa91a3455b66174195b94ca55b0e6f'],
  'linux-arm64': ['package/bin/napi-v6/linux/arm64/libonnxruntime.so.1', '152fb7d2f4f06acc5a38627fbca3e56ba2a9b984cb060d2ac500ac272afbb9ba'],
  'darwin-arm64': ['package/bin/napi-v6/darwin/arm64/libonnxruntime.1.31.1.dylib', '9a362365243b56da98a2ad65128a32d6f8066d1f39ce697e0fce997adf0586dc'],
};

export async function installRuntime(root, primary, fallback) {
  const directory = path.join(root, 'runtime', 'ort');
  let source = primary;
  try {
    await download(path.join(root, 'downloads', primary.name), primary.url, primary.sha256);
  } catch (error) {
    // A bad checksum must fail closed; it must never trigger a source switch.
    if (error.status !== 404) throw error;
    console.log('GitHub runtime archive unavailable; using the verified Microsoft npm archive for the same version.');
    await download(path.join(root, 'downloads', fallback.name), fallback.url, fallback.sha256);
    source = fallback;
  }
  await mkdir(directory, { recursive: true });
  execFileSync('tar', ['xzf', path.join(root, 'downloads', source.name), '-C', directory,
    ...(source === fallback ? [source.library] : [])], { stdio: 'inherit' });
  const library = path.join(directory, source.library);
  await access(library);
  if (source.librarySha256 && await fingerprint(library) !== source.librarySha256) {
    throw new Error('Native library fingerprint mismatch');
  }
  return { library, source: { url: source.url, sha256: source.sha256 } };
}

async function main() {
  const args = process.argv.slice(2);
  const option = name => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
  if (!option('--output')) throw new Error('Usage: provision_native_ocr.mjs --output DIRECTORY [--platform linux-x64|linux-arm64|darwin-arm64]');
  const root = path.resolve(option('--output'));
  const platform = option('--platform') ?? `${process.platform}-${process.arch}`;
  const runtime = runtimes[platform];
  if (!runtime) throw new Error(`No pinned runtime for ${platform}; provision compatible native libraries explicitly`);

  const artifacts = {};
  for (const [key, [name, url, sha256]] of Object.entries(models)) {
    const file = path.join(root, 'models', name);
    console.log(`Provisioning ${name}`);
    await download(file, url, sha256);
    artifacts[key] = { path: file, sha256 };
  }
  const [ortName, ortSha256] = runtime.ort;
  const ortLibrary = platform.startsWith('darwin') ? 'libonnxruntime.1.31.1.dylib' : 'libonnxruntime.so.1.31.1';
  const [npmLibrary, npmLibrarySha256] = npmLibraries[platform];
  const { library: onnxruntime_library, source: onnxruntime_source } = await installRuntime(root, {
    name: ortName, sha256: ortSha256,
    url: `https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/${ortName}`,
    library: `${ortName.replace(/\.tgz$/, '')}/lib/${ortLibrary}`,
  }, { ...npmRuntime, library: npmLibrary, librarySha256: npmLibrarySha256 });
  for (const [key, [name, sha256]] of Object.entries(runtime)) {
    if (key === 'ort') continue;
    const url = `https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8152/${name}`;
    const file = path.join(root, 'downloads', name);
    await download(file, url, sha256);
    const directory = path.join(root, 'runtime', key);
    await mkdir(directory, { recursive: true });
    execFileSync('tar', ['xzf', file, '-C', directory], { stdio: 'inherit' });
  }
  const manifest = {
    models: { det: artifacts.det, rec: artifacts.rec, cls: artifacts.cls },
    layout: { detector: artifacts.layout, table: artifacts.table, dictionary: artifacts.table_dictionary },
  };
  const pdfium_library = await findFile(path.join(root, 'runtime/pdfium'), platform.startsWith('darwin') ? 'libpdfium.dylib' : 'libpdfium.so');
  if (!onnxruntime_library || !pdfium_library) throw new Error('Native library missing from verified runtime archive');
  await writeFile(path.join(root, 'models.json'), JSON.stringify(manifest, null, 2) + '\n');
  await writeFile(path.join(root, 'config.json'), JSON.stringify({
    models: path.join(root, 'models.json'), onnxruntime_library, pdfium_library,
    threads: 1, page_timeout_ms: 120_000, document_timeout_ms: 120_000,
  }, null, 2) + '\n');
  await writeFile(path.join(root, 'sources.json'), JSON.stringify({ models, runtime, platform, onnxruntime_source }, null, 2) + '\n');
  console.log(`Ready: ${path.join(root, 'config.json')}`);
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
