#!/usr/bin/env node
// Vendors one exact release of pele, the Mermaid renderer the build runs in its
// own JavaScript engine, into crates/moss-build/data/pele/.
//
//   node scripts/pele/update.mjs <version>
//
// It downloads the npm tarball, refuses it unless it matches the registry's
// sha512 integrity, and writes:
//   pele.global.js.br  the global bundle, brotli-compressed (embedded in the binary)
//   NOTICE             pele's MIT licence, embedded and loaded with the bundle
//   pin.txt            "<version> <sha256 of the uncompressed bundle>"; a test checks the
//                      embedded bytes against it
// Then run `cargo test --workspace` and regenerate any snapshot whose diagrams changed.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { brotliCompressSync, constants } from 'node:zlib';

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? '')) {
  console.error('usage: node scripts/pele/update.mjs <exact version, e.g. 1.13.0>');
  process.exit(2);
}
const out = new URL('../../crates/moss-build/data/pele/', import.meta.url).pathname;

const meta = await (await fetch(`https://registry.npmjs.org/pele/${version}`)).json();
const tgz = Buffer.from(await (await fetch(meta.dist.tarball)).arrayBuffer());
const integrity = `sha512-${createHash('sha512').update(tgz).digest('base64')}`;
if (integrity !== meta.dist.integrity) throw new Error(`tarball integrity ${integrity} != registry ${meta.dist.integrity}`);

const dir = mkdtempSync(join(tmpdir(), 'pele-'));
try {
  writeFileSync(join(dir, 'pele.tgz'), tgz);
  execFileSync('tar', ['-xzf', 'pele.tgz', 'package/dist/pele.global.js', 'package/LICENSE'], { cwd: dir });
  const bundle = readFileSync(join(dir, 'package/dist/pele.global.js'));
  const sha256 = createHash('sha256').update(bundle).digest('hex');
  const br = brotliCompressSync(bundle, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: bundle.length } });
  writeFileSync(join(out, 'pele.global.js.br'), br);
  writeFileSync(join(out, 'NOTICE'), `pele ${version}, https://github.com/obsidianmd/pele\n\n${readFileSync(join(dir, 'package/LICENSE'), 'utf8')}`);
  writeFileSync(join(out, 'pin.txt'), `${version} ${sha256}\n`);
  console.log(`pele ${version}: ${bundle.length} bytes, ${br.length} brotli, sha256 ${sha256}`);
} finally {
  rmSync(dir, { recursive: true, force: true });
}
