// Platform-specific asset resolution for Windows.
// Run with `npm test` (node's built-in runner, no dependencies).

'use strict';

const assert = require('node:assert');
const { execFileSync } = require('node:child_process');
const { test } = require('node:test');
const path = require('node:path');

const packageRoot = path.join(__dirname, '..');
const fetchLib = path.join(packageRoot, 'lib', 'fetch.js');

// Run code in a child process with mocked platform/arch, isolated from inherited env.
function resolveOnPlatform(platform, arch) {
  const code = `
    Object.defineProperty(process, 'platform', { value: '${platform}' });
    Object.defineProperty(process, 'arch', { value: '${arch}' });
    const fetch = require(${JSON.stringify(fetchLib)});
    console.log(JSON.stringify({
      asset: fetch.resolveAsset(),
      vendorPath: fetch.vendorBinary()
    }));
  `;
  const out = execFileSync(process.execPath, ['-e', code], {
    env: { PATH: process.env.PATH },
    encoding: 'utf8'
  });
  return JSON.parse(out.trim().split('\n').pop());
}

test('resolveAsset returns moss-windows-x86_64.exe on win32-x64', () => {
  const { asset } = resolveOnPlatform('win32', 'x64');
  assert.equal(asset, 'moss-windows-x86_64.exe');
});

test('vendorBinary path ends with moss.exe on win32', () => {
  const { vendorPath } = resolveOnPlatform('win32', 'x64');
  assert(vendorPath.endsWith('moss.exe'), `expected path to end with moss.exe, got ${vendorPath}`);
});
