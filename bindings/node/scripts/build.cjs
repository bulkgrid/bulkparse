'use strict';
const { execFileSync } = require('node:child_process');
const { copyFileSync } = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '../../..');
execFileSync('cargo', ['build', '--release', '-p', 'bulkparse-node'], {cwd: root, stdio: 'inherit'});
const name = process.platform === 'win32' ? 'bulkparse_node.dll'
  : process.platform === 'darwin' ? 'libbulkparse_node.dylib' : 'libbulkparse_node.so';
const target = path.resolve(root, process.env.CARGO_TARGET_DIR || 'target');
copyFileSync(path.join(target, 'release', name), path.resolve(__dirname, '../bulkparse.node'));
