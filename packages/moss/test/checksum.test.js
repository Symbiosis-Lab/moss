// A release binary is about to be written with mode 0o755. The checksum is the
// only thing standing between "GitHub served these bytes" and "we run them", so
// these tests are about what happens when the checksum cannot be read.

'use strict';

const assert = require('node:assert');
const { test } = require('node:test');

const { checksumsRequired, verifyChecksum } = require('../lib/fetch.js');

// Fetch is global in Node >= 18; each test installs its own and restores it.
async function withFetch(impl, body) {
  const real = global.fetch;
  global.fetch = impl;
  try {
    return await body();
  } finally {
    global.fetch = real;
  }
}

const ASSET = 'moss-linux-x86_64';
const BYTES = Buffer.from('not a real binary');

test('a suppressed SHA256SUMS does not install unverified bytes', async () => {
  // The download and the checksum come over the same connection, so whoever can
  // serve a binary can also 404 this one small request. If that skipped
  // verification, the check would protect exactly nobody.
  await withFetch(
    async () => ({ ok: false, status: 404 }),
    async () => {
      await assert.rejects(
        () => verifyChecksum(BYTES, ASSET, '0.12.1'),
        /cannot verify moss v0\.12\.1/
      );
    }
  );
});

test('a network error reaching SHA256SUMS is fatal, not a warning', async () => {
  await withFetch(
    async () => {
      throw new Error('ECONNRESET');
    },
    async () => {
      await assert.rejects(
        () => verifyChecksum(BYTES, ASSET, '0.12.1'),
        /ECONNRESET/
      );
    }
  );
});

test('a SHA256SUMS with no line for this asset is fatal', async () => {
  // Trimming one line is a subtler edit than removing the file, and before the
  // fail-closed change it had the same effect: no verification, exit 0.
  await withFetch(
    async () => ({ ok: true, text: async () => 'abc123  some-other-asset\n' }),
    async () => {
      await assert.rejects(
        () => verifyChecksum(BYTES, ASSET, '0.12.1'),
        /no entry for moss-linux-x86_64/
      );
    }
  );
});

test('a pre-v0.11.2 release still installs without checksums', async () => {
  // Those releases genuinely published no SHA256SUMS. Failing closed there would
  // break an install that was never verifiable in the first place.
  await withFetch(
    async () => ({ ok: false, status: 404 }),
    async () => {
      await verifyChecksum(BYTES, ASSET, '0.11.1');
    }
  );
});

test('the version floor is the release that first published SHA256SUMS', () => {
  assert.equal(checksumsRequired('0.11.1'), false);
  assert.equal(checksumsRequired('0.11.2'), true);
  assert.equal(checksumsRequired('0.13.0'), true);
  assert.equal(checksumsRequired('1.0.0'), true);
  // A prerelease of the floor is the floor.
  assert.equal(checksumsRequired('0.11.2-rc.1'), true);
  // Unparseable is not a licence to skip: MOSS_VERSION_OVERRIDE takes any string.
  assert.equal(checksumsRequired('nightly'), true);
  assert.equal(checksumsRequired(''), true);
});
