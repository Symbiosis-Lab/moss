// Downloads the moss CLI binary for this platform from moss.
//
// No dependencies: uses the global fetch that ships with Node >= 18.
// The version defaults to this package's own version (the wrapper is
// released in lockstep with moss), overridable with MOSS_VERSION_OVERRIDE
// for ops and testing.

'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

const RELEASES = 'https://github.com/Symbiosis-Lab/moss/releases/download';

const packageRoot = path.join(__dirname, '..');

// Compute the vendored binary path at use time, appending .exe on win32.
function vendorBinary() {
  const name = process.platform === 'win32' ? 'moss.exe' : 'moss';
  return path.join(packageRoot, 'vendor', name);
}

// npm and pnpm expose every `.npmrc` key to lifecycle scripts as `npm_config_<key>`,
// so one lookup covers both the environment variable and the config file. This is
// the mechanism Cypress documents for `CYPRESS_INSTALL_BINARY` and puppeteer for
// `PUPPETEER_SKIP_DOWNLOAD`.
function flag(name) {
  const value =
    process.env[name] ?? process.env[`npm_config_${name.toLowerCase()}`];
  if (value === undefined || value === '') return false;
  return !/^(0|false|no)$/i.test(value);
}

// True when this copy of the package was installed as somebody's dependency.
// Every package manager unpacks a published tarball into a directory with a
// `node_modules` segment in its path — npm and pnpm's store both, and Yarn PnP's
// unplugged cache. A checkout of the moss repo has no such segment, so the
// absence of one means we are running from source, not from an install.
function isInstalledAsDependency() {
  return packageRoot.split(path.sep).includes('node_modules');
}

/**
 * Why the postinstall download should not run, or null to go ahead.
 *
 * Returns a reason suitable for printing — the caller decides whether that is
 * a log line or an error.
 */
function skipDownloadReason() {
  if (flag('MOSS_FORCE_BINARY_DOWNLOAD')) return null;
  if (flag('MOSS_SKIP_BINARY_DOWNLOAD')) return 'MOSS_SKIP_BINARY_DOWNLOAD is set';
  if (process.env.MOSS_BINARY_PATH) {
    return `MOSS_BINARY_PATH points at ${process.env.MOSS_BINARY_PATH}`;
  }
  if (!isInstalledAsDependency()) {
    return 'running from a source checkout, not an install — build moss yourself, ' +
      'or set MOSS_FORCE_BINARY_DOWNLOAD=1 to fetch the release binary anyway';
  }
  return null;
}

// Which binary `moss` should execute. `MOSS_BINARY_PATH` wins so a developer can
// point the wrapper at a locally built moss (esbuild's `ESBUILD_BINARY_PATH`).
function resolveBinary() {
  return process.env.MOSS_BINARY_PATH || vendorBinary();
}

function resolveVersion() {
  const override = process.env.MOSS_VERSION_OVERRIDE;
  if (override) return override.replace(/^v/, '');
  return require(path.join(packageRoot, 'package.json')).version;
}

// Platform → release asset. The darwin binary is a universal (x64 + arm64)
// build, so both Mac architectures map to the same asset.
function resolveAsset() {
  const { platform, arch } = process;
  if (platform === 'darwin' && (arch === 'x64' || arch === 'arm64')) {
    return 'moss-darwin-universal';
  }
  if (platform === 'linux' && arch === 'x64') {
    return 'moss-linux-x86_64';
  }
  if (platform === 'win32' && arch === 'x64') {
    return 'moss-windows-x86_64.exe';
  }
  throw new Error(
    `moss-npm: no moss binary is published for ${platform}-${arch}.\n` +
      'Supported: macOS (x64/arm64), Linux (x64), and Windows (x64). ' +
      'See https://github.com/Symbiosis-Lab/moss for available builds.'
  );
}

async function download(url) {
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) {
    throw new Error(`moss-npm: download failed (${res.status} ${res.statusText}): ${url}`);
  }
  return Buffer.from(await res.arrayBuffer());
}

// The first release that published a SHA256SUMS file. Before it there is
// nothing to verify against; from it on, every release has one.
const CHECKSUMS_SINCE = [0, 11, 2];

// Must a missing checksum be fatal for this version? Anything unparseable
// answers yes: an unrecognised version string is not a licence to skip the
// only check on bytes we are about to mark executable.
function checksumsRequired(version) {
  const parts = String(version).split('-')[0].split('.').map(Number);
  if (parts.length !== 3 || parts.some((n) => !Number.isInteger(n))) return true;
  for (let i = 0; i < 3; i += 1) {
    if (parts[i] !== CHECKSUMS_SINCE[i]) return parts[i] > CHECKSUMS_SINCE[i];
  }
  return true;
}

// Verifies bytes against the release's SHA256SUMS.
//
// Every reason we might not reach a checksum — a non-200, a network error, a
// SHA256SUMS with no line for this asset — is reached over the same connection
// that just served the binary. So on a version that publishes checksums, taking
// any of them as permission to skip verification hands the skip to whoever
// controls that connection: suppress one small request and the binary installs
// unverified. Fail closed there, and keep the warning only for the pre-v0.11.2
// releases where the file genuinely does not exist.
async function verifyChecksum(bytes, asset, version) {
  const unverified = (reason) => {
    if (checksumsRequired(version)) {
      throw new Error(
        `moss-npm: cannot verify moss v${version} (${asset}): ${reason}.\n` +
          '  Every release from v0.11.2 on publishes SHA256SUMS, so this is either a\n' +
          '  transient network fault or a tampered download. Not installing.'
      );
    }
    console.warn(`moss-npm: ${reason}; skipping checksum verification (v${version} predates SHA256SUMS)`);
  };

  let sums;
  try {
    const res = await fetch(`${RELEASES}/v${version}/SHA256SUMS`, { redirect: 'follow' });
    if (!res.ok) {
      return unverified(`no SHA256SUMS published for v${version} (${res.status})`);
    }
    sums = await res.text();
  } catch (err) {
    return unverified(`could not fetch SHA256SUMS (${err.message})`);
  }

  const line = sums.split('\n').find((l) => l.trim().endsWith(asset));
  if (!line) {
    return unverified(`SHA256SUMS has no entry for ${asset}`);
  }

  const expected = line.trim().split(/\s+/)[0];
  const actual = crypto.createHash('sha256').update(bytes).digest('hex');
  if (actual !== expected) {
    throw new Error(
      `moss-npm: checksum mismatch for ${asset} v${version}\n` +
        `  expected ${expected}\n  got      ${actual}\n` +
        'The download may be corrupted or tampered with. Not installing.'
    );
  }
}

async function fetchBinary() {
  const version = resolveVersion();
  const asset = resolveAsset();
  const url = `${RELEASES}/v${version}/${asset}`;

  console.log(`moss-npm: downloading moss v${version} (${asset})...`);
  const bytes = await download(url);
  await verifyChecksum(bytes, asset, version);

  const binaryPath = vendorBinary();
  fs.mkdirSync(path.dirname(binaryPath), { recursive: true });
  fs.writeFileSync(binaryPath, bytes, { mode: 0o755 });
  console.log(`moss-npm: installed ${binaryPath}`);
  return binaryPath;
}

module.exports = {
  checksumsRequired,
  fetchBinary,
  vendorBinary,
  resolveAsset,
  resolveBinary,
  resolveVersion,
  skipDownloadReason,
  verifyChecksum,
};
