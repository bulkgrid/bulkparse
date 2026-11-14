'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { platformKey, platforms } = require('../platform.cjs');

test('selects only supported native packages and rejects musl', () => {
  for (const [key, { os, cpu }] of Object.entries(platforms)) {
    assert.equal(platformKey(os, cpu, '2.35'), key);
  }
  assert.throws(() => platformKey('linux', 'x64', ''), /musl/);
  assert.throws(() => platformKey('win32', 'arm64', ''), /No BulkParse prebuilt/);
});
