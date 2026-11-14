#!/usr/bin/env node
'use strict';
const { spawnSync } = require('node:child_process');
const { nativePath } = require('../platform.cjs');
try {
  const binary = nativePath(process.platform === 'win32' ? 'bulkparse.exe' : 'bulkparse');
  const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.signal) process.kill(process.pid, result.signal);
  else process.exitCode = result.status ?? 1;
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
