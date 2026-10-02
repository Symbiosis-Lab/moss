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
// The landing gets the engine, the model and the default paper, not the whole
// package: the physical paper generator is for offline sheets and the lab,
// and would add a third to the bundle for nothing the page calls.
//
// Usage: node scripts/build-site-watercolor.mjs
import { build } from 'esbuild';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));

await build({
  stdin: {
    contents: [
      "export * from './engine/index.js';",
      "export * from './model/index.js';",
      "export { createPaper } from './paper/default.js';",
    ].join('\n'),
    resolveDir: `${ROOT}/packages/moss-watercolor/src`,
    loader: 'ts',
  },
  tsconfig: `${ROOT}/packages/moss-watercolor/tsconfig.json`,
  outfile: `${ROOT}/site/vendor/moss-watercolor/moss-watercolor.js`,
  bundle: true,
  format: 'iife',
  globalName: 'MossWatercolor',
  target: 'es2020',
  banner: { js: '// Built from packages/moss-watercolor by scripts/build-site-watercolor.mjs — do not hand-edit.' },
});
console.log('built site/vendor/moss-watercolor/moss-watercolor.js');
