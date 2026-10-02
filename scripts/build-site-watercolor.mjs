#!/usr/bin/env node
// Bundles packages/moss-watercolor into the IIFE the landing's own classic
// <script> (site/index.html) loads as `vendor/moss-watercolor/moss-watercolor.js`,
// the same "vendored, checked-in build" pattern site/vendor/d3-force already
// uses -- except this one is our own package, not a third party's, so it
// carries no separate LICENSE file (the repo root's covers it).
//
// A copied build artifact drifting from its source is exactly what
// check-site-watercolor-freshness.mjs exists to catch: this script is also
// what that check reruns before diffing.
//
// Usage: node scripts/build-site-watercolor.mjs
import { build } from 'esbuild';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));

await build({
  entryPoints: [`${ROOT}/packages/moss-watercolor/src/index.ts`],
  outfile: `${ROOT}/site/vendor/moss-watercolor/moss-watercolor.js`,
  bundle: true,
  format: 'iife',
  globalName: 'MossWatercolor',
  target: 'es2020',
  banner: { js: '// Built from packages/moss-watercolor by scripts/build-site-watercolor.mjs — do not hand-edit.' },
});
console.log('built site/vendor/moss-watercolor/moss-watercolor.js');
