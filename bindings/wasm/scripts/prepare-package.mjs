import {access, writeFile} from 'node:fs/promises';

const pkg = new URL('../pkg/', import.meta.url);
for (const name of ['bulkparse_wasm.js', 'bulkparse_wasm.d.ts', 'bulkparse_wasm_bg.wasm', 'bulkparse_wasm_bg.wasm.d.ts']) {
  try { await access(new URL(name, pkg)); }
  catch { throw new Error(`Missing pkg/${name}. Run npm run build before packaging.`); }
}
// wasm-pack writes pkg/.gitignore with '*'. npm otherwise treats it as an
// ignore rule even though the wrapper's files list includes these assets.
await writeFile(new URL('.npmignore', pkg), '');
