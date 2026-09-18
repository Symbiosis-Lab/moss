#!/usr/bin/env node
import { cpSync, existsSync, mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const value = (name, fallback) => {
  const i = args.indexOf(name);
  return i < 0 ? fallback : args[i + 1];
};
const landing = resolve(value('--landing', ''));
const output = resolve(value('--out', join(repo, 'dist/site')));
const moss = resolve(value('--moss-bin', join(repo, 'target/debug/moss-cli')));
const allowPending = args.includes('--allow-pending-docs');

if (!value('--landing', '')) throw new Error('usage: build-site-with-landing.mjs --landing <landing-lab checkout> [--out <dir>] [--moss-bin <binary>] [--allow-pending-docs]');
for (const path of [moss, join(landing, 'movie6.html')]) if (!existsSync(path)) throw new Error(`missing ${path}`);

const scratch = mkdtempSync(join(tmpdir(), 'moss-landing-build-'));
const stagedSite = join(scratch, 'site');
try {
  cpSync(join(repo, 'site'), stagedSite, {
    recursive: true,
    filter: (source) => {
      const path = relative(join(repo, 'site'), source).split(sep);
      return path[0] !== '.cursor' && !(path[0] === '.moss' && ['build', 'cache', 'agents'].includes(path[1]));
    }
  });
  const built = spawnSync(moss, ['build', stagedSite], { stdio: 'inherit' });
  if (built.status !== 0) throw new Error(`moss build failed with status ${built.status}`);

  const compiled = realpathSync(join(stagedSite, '.moss/build/current'));
  rmSync(output, { recursive: true, force: true });
  mkdirSync(dirname(output), { recursive: true });
  cpSync(compiled, output, { recursive: true });
  cpSync(join(landing, 'movie6.html'), join(output, 'index.html'));
  for (const file of ['closing.js']) {
    const source = join(landing, file);
    if (!existsSync(source)) throw new Error(`landing root asset is missing: ${source}`);
    cpSync(source, join(output, file));
  }
  for (const dir of ['blake', 'zhuda', 'ui', 'scene4', 'vendor']) {
    const source = join(landing, dir);
    if (!existsSync(source)) throw new Error(`landing asset root is missing: ${source}`);
    cpSync(source, join(output, dir), { recursive: true });
  }
  // Scene 3 and 5 also contain contact sheets, discarded candidates, source
  // notebooks, and build tools. Publish only the files the landing executes.
  for (const path of [
    'scene3/article',
    'scene3/sketch/native.html',
    'scene3/sketch/native-canvas.js',
    'scene3/notebook/breed/mandelbrot-live.html',
    'scene3/notebook/breed/mandelbrot-renderer.js',
    'scene3/notebook/breed/jupyterlab-notebook-960.png',
    'scene3/movie/pool/clips/general-railroad-ties.mp4',
    'scene3/movie/pool/posters/general-railroad-ties.jpg',
    'scene5-loop/out/v5-makers.mp4',
    'scene5-loop/out/v5-makers-poster.jpg'
  ]) {
    const source = join(landing, path);
    if (!existsSync(source)) throw new Error(`landing runtime asset is missing: ${source}`);
    mkdirSync(dirname(join(output, path)), { recursive: true });
    cpSync(source, join(output, path), { recursive: true });
  }
  // Landing-owned shared assets (for example platform marks) merge into the
  // compiler's /assets tree; replacing it would discard documentation media.
  const sharedAssets = join(landing, 'assets');
  if (existsSync(sharedAssets)) cpSync(sharedAssets, join(output, 'assets'), { recursive: true });

  const contract = JSON.parse(readFileSync(join(repo, 'scripts/landing-routes.json'), 'utf8'));
  const missing = (routes) => routes.filter((route) => !existsSync(join(output, route.slice(1), 'index.html')));
  const missingRequired = missing(contract.required);
  const missingPending = missing(contract.pending_docs_merge);
  if (missingRequired.length) throw new Error(`compiled docs routes missing: ${missingRequired.join(', ')}`);
  if (missingPending.length && !allowPending) throw new Error(`docs integration not complete: ${missingPending.join(', ')}`);

  const html = readFileSync(join(output, 'index.html'), 'utf8');
  const refs = [...html.matchAll(/(?:src|poster)=["']([^"']+)["']/g), ...html.matchAll(/fetch\(["']([^"']+)["']/g)].map((m) => m[1]);
  const missingAssets = refs.filter((ref) => {
    if (/^(?:https?:|data:|blob:|#)/.test(ref)) return false;
    return !existsSync(join(output, ref.replace(/^\//, '').split(/[?#]/)[0]));
  });
  if (missingAssets.length) throw new Error(`landing asset references missing: ${[...new Set(missingAssets)].join(', ')}`);

  console.log(`site: ${output}`);
  console.log(`landing: ${join(output, 'index.html')}`);
  console.log(`docs routes: ${contract.required.length} ready`);
  console.log(`pending docs routes: ${missingPending.length ? missingPending.join(', ') : 'none'}`);
  console.log(`landing asset references: ${refs.length} resolved`);
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
