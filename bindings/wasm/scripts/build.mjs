import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const cwd = fileURLToPath(new URL('..', import.meta.url));
execFileSync('wasm-pack', ['build', '--target', 'web', '--release'], {
  cwd,
  stdio: 'inherit',
  env: {
    ...process.env,
    WASM_PACK_CACHE: process.env.WASM_PACK_CACHE
      ?? fileURLToPath(new URL('../../../target/wasm-pack-cache', import.meta.url)),
  },
});
await import('./prepare-package.mjs');
