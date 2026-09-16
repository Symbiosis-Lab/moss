// When does the postinstall fetch a release binary, and when does it stay out
// of the way? Run with `npm test` (node's built-in runner, no dependencies).

'use strict';

const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { test } = require('node:test');

const packageRoot = path.join(__dirname, '..');
const fetchLib = path.join(packageRoot, 'lib', 'fetch.js');

// A real copy of the package under a real `node_modules` path — the shape every
// package manager unpacks a published tarball into. The check under test reads
// the module's own location, so nothing short of relocating it is a fair test.
let installedCopy;
function asInstalled() {
  if (!installedCopy) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'moss-npm-test-'));
    const dest = path.join(dir, 'node_modules', '@symbiosis-lab', 'moss');
    fs.mkdirSync(path.join(dest, 'lib'), { recursive: true });
    fs.copyFileSync(fetchLib, path.join(dest, 'lib', 'fetch.js'));
    fs.copyFileSync(
      path.join(packageRoot, 'package.json'),
      path.join(dest, 'package.json')
    );
    installedCopy = path.join(dest, 'lib', 'fetch.js');
  }
  return installedCopy;
}

// Each case runs in a child process so it gets an environment of exactly the
// variables it names, with no inherited MOSS_* leaking in.
function reasonWith(env, { asDependency = false } = {}) {
  const lib = asDependency ? asInstalled() : fetchLib;
  const out = execFileSync(
    process.execPath,
    ['-e', `console.log(JSON.stringify(require(${JSON.stringify(lib)}).skipDownloadReason()))`],
    { env: { PATH: process.env.PATH, ...env }, encoding: 'utf8' }
  );
  return JSON.parse(out.trim().split('\n').pop());
}

test('a source checkout does not download — this is the moss monorepo case', () => {
  const reason = reasonWith({});
  assert.match(reason, /source checkout/);
});

test('MOSS_FORCE_BINARY_DOWNLOAD overrides the source-checkout skip', () => {
  assert.equal(reasonWith({ MOSS_FORCE_BINARY_DOWNLOAD: '1' }), null);
});

test('an ordinary install downloads', () => {
  assert.equal(reasonWith({}, { asDependency: true }), null);
});

test('MOSS_SKIP_BINARY_DOWNLOAD stops an ordinary install', () => {
  const reason = reasonWith({ MOSS_SKIP_BINARY_DOWNLOAD: '1' }, { asDependency: true });
  assert.match(reason, /MOSS_SKIP_BINARY_DOWNLOAD/);
});

test('a falsy MOSS_SKIP_BINARY_DOWNLOAD is not a skip', () => {
  assert.equal(reasonWith({ MOSS_SKIP_BINARY_DOWNLOAD: '0' }, { asDependency: true }), null);
});

test('an .npmrc key reaches the script as npm_config_*', () => {
  const reason = reasonWith(
    { npm_config_moss_skip_binary_download: 'true' },
    { asDependency: true }
  );
  assert.match(reason, /MOSS_SKIP_BINARY_DOWNLOAD/);
});

test('MOSS_BINARY_PATH means the developer already has a binary', () => {
  const reason = reasonWith({ MOSS_BINARY_PATH: '/tmp/moss' }, { asDependency: true });
  assert.match(reason, /MOSS_BINARY_PATH/);
});

test('resolveBinary prefers MOSS_BINARY_PATH over the vendored copy', () => {
  const { resolveBinary, vendorBinary } = require(fetchLib);
  assert.equal(resolveBinary(), vendorBinary());
  process.env.MOSS_BINARY_PATH = '/tmp/moss-local';
  try {
    assert.equal(resolveBinary(), '/tmp/moss-local');
  } finally {
    delete process.env.MOSS_BINARY_PATH;
  }
});
