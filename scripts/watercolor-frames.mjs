#!/usr/bin/env node
// Capture the wash before changing it: --write records hashes; default compares.
// PNGs retain canvas alpha and stay outside the repository (--out, or a temp dir).
// Run twice on the pinned renderer before accepting an initial baseline.
import { build } from 'esbuild';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { loadPlaywright } from './landing-harness.mjs';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const BASELINE = join(ROOT, 'scripts/watercolor-frames.hashes.json');
const PLAYWRIGHT_VERSION = '1.61.1';
export const CASES = ['light', 'dark', 'transparent', 'to-nothing', 'turn'];
export const GRIDS = [{ name: 'phone-205x195', divisor: 4 }, { name: 'desktop-410x390', divisor: 2 }];
export const frameKey = (name, grid, i) => `${name}/${grid}/p${String(i).padStart(2, '0')}-of-60`;
export const frameKeys = () => CASES.flatMap(name => GRIDS.flatMap(grid =>
  Array.from({ length: 61 }, (_, i) => frameKey(name, grid.name, i)))).sort();

// Resolve existing ancestors too: a symlink outside the repo must not lead back in.
export function outsideRepository(path, root = ROOT) {
  let ancestor = resolve(path);
  const tail = [];
  while (!existsSync(ancestor)) { tail.unshift(basename(ancestor)); ancestor = dirname(ancestor); }
  const canonical = join(realpathSync(ancestor), ...tail), repo = realpathSync(root);
  if (canonical === repo || canonical.startsWith(repo + sep)) throw new Error('--out must be outside the repository');
  return canonical;
}

export function compareFrames(expected, actual) {
  const differences = [];
  const planned = new Set(frameKeys());
  for (const key of [...new Set([...planned, ...Object.keys(expected), ...Object.keys(actual)])].sort()) {
    if (!planned.has(key)) differences.push(`extra frame: ${key}`);
    else if (!(key in expected)) differences.push(`missing baseline: ${key}`);
    else if (!(key in actual)) differences.push(`extra baseline / missing capture: ${key}`);
    else if (expected[key] !== actual[key]) differences.push(`different: ${key}`);
  }
  return differences;
}

export function compareMetadata(expected, actual) {
  return [...new Set([...Object.keys(expected), ...Object.keys(actual)])].sort()
    .filter(key => JSON.stringify(expected[key]) !== JSON.stringify(actual[key]))
    .map(key => `metadata mismatch ${key}: baseline=${JSON.stringify(expected[key])}, actual=${JSON.stringify(actual[key])}`);
}

export async function pinnedPlaywright() {
  const require = createRequire(import.meta.url);
  const testPath = require.resolve('@playwright/test/package.json');
  const playwrightPath = createRequire(testPath).resolve('playwright/package.json');
  const corePath = createRequire(playwrightPath).resolve('playwright-core/package.json');
  for (const path of [testPath, playwrightPath, corePath]) {
    const { name, version } = JSON.parse(readFileSync(path, 'utf8'));
    if (version !== PLAYWRIGHT_VERSION) throw new Error(`${name}: expected ${PLAYWRIGHT_VERSION}, loaded ${version}`);
  }
  const loaded = await loadPlaywright();
  if (loaded.chromium !== require('@playwright/test').chromium) {
    throw new Error('PLAYWRIGHT_MODULE must resolve to the repository\'s pinned Playwright installation');
  }
  return { chromium: loaded.chromium, version: JSON.parse(readFileSync(playwrightPath, 'utf8')).version };
}

export async function bundleEngine() {
  const bundle = await build({
    stdin: { contents: "export * from './engine/index.js'; export * from './model/index.js';",
      resolveDir: join(ROOT, 'packages/moss-watercolor/src'), loader: 'ts' },
    tsconfig: join(ROOT, 'packages/moss-watercolor/tsconfig.json'),
    bundle: true, format: 'iife', globalName: 'MossWatercolor', target: 'es2020', write: false,
  });
  return bundle.outputFiles[0].text;
}

// Runs inside Chromium. No images, fonts, timers, random input, or page compositing.
export function prepareInPage({ name, divisor }) {
  const W = 820, H = 780, dark = name === 'dark';
  const ground = ['transparent', 'to-nothing', 'turn'].includes(name) ? 'transparent' : 'paper';
  const tint = dark ? '#1d201e' : '#f4efe6';
  const make = () => { const c = document.createElement('canvas'); c.width = W; c.height = H; return c; };
  const prints = [0, 1, 2].map(index => {
    const c = make(), g = c.getContext('2d');
    if (ground === 'paper') { g.fillStyle = tint; g.fillRect(0, 0, W, H); }
    g.strokeStyle = dark ? '#d8dcd4' : '#263447'; g.lineWidth = 4;
    for (let x = 24; x < W; x += 48) {
      g.beginPath(); g.moveTo(x, 24); g.lineTo(index === 1 ? x + 240 : x, H - 24); g.stroke();
    }
    g.fillStyle = dark ? '#e89c70' : '#ac3828';
    g.beginPath(); g.arc(210 + index * 170, 220 + index * 100, 110, 0, Math.PI * 2); g.fill();
    g.fillStyle = dark ? '#bacdf0' : '#235a86';
    g.fillRect(100 + index * 60, 530 - index * 130, 320, 80);
    g.fillStyle = dark ? '#e6d5a5' : '#847122';
    g.beginPath(); g.moveTo(560, 420); g.lineTo(740, 640); g.lineTo(480, 700); g.closePath(); g.fill();
    return c;
  });
  const foot = make(), fg = foot.getContext('2d'); fg.fillStyle = tint; fg.fillRect(0, 0, W, H);
  const canvas = document.querySelector('canvas');
  const sim = MossWatercolor.createSim({ canvas, texW: W, texH: H, divisor,
    rect: () => ({ x: 0, y: 0, w: W, h: H }), ground, tint });
  if (!sim) throw new Error('WebGL2 + EXT_color_buffer_float required');
  const gl = canvas.getContext('webgl2'), debug = gl.getExtension('WEBGL_debug_renderer_info');
  if (!debug) throw new Error('Renderer identification unavailable');
  const renderer = gl.getParameter(debug.UNMASKED_RENDERER_WEBGL);
  if (!/swiftshader/i.test(renderer)) throw new Error(`Expected SwiftShader, got ${renderer}`);
  const sides = prints.slice(0, name === 'turn' ? 3 : name === 'to-nothing' ? 1 : 2)
    .map(print => sim.sceneSide(print, foot));
  const metadata = { renderer, vendor: gl.getParameter(debug.UNMASKED_VENDOR_WEBGL),
    webgl: gl.getParameter(gl.VERSION), shadingLanguage: gl.getParameter(gl.SHADING_LANGUAGE_VERSION) };
  let baked;
  window.watercolorFrames = {
    // Each call is bounded so the host can identify a stalled recording.
    record(index) { sim.recordSide(sides[index], 36); return sim.sideRecorded(sides[index]); },
    count: sides.length,
    capture(i) {
      const p = i / 60;
      let a = sides[0], b = sides[1] ?? null, q = p;
      // p<.5 follows A->B. At .5 bake its midpoint and capture the resumed turn.
      // p>=.5 maps linearly from turnAt(.5).p to 1 on baked->C, ending on C.
      if (name === 'turn' && i >= 30) {
        if (!baked) {
          const { ux, uy, k } = MossWatercolor.legAt(.5);
          baked = sim.bake(a, ux, b, uy, k, .5);
        }
        const turn = MossWatercolor.turnAt(.5);
        if (turn.from !== 'mix') throw new Error('Expected a midpoint mix turn');
        a = baked; b = sides[2]; q = turn.p + (1 - turn.p) * (p - .5) * 2;
      }
      const { ux, uy, k } = MossWatercolor.legAt(q);
      if (!sim.playSides(a, ux, b, uy, k, q, { toNothing: name === 'to-nothing' })) throw new Error('Frame not ready');
      const png = canvas.toDataURL('image/png');
      if (gl.isContextLost()) throw new Error('WebGL context lost');
      return png;
    },
    dispose() { if (baked) sim.freeSide(baked); sides.forEach(side => sim.freeSide(side)); sim.dispose(); },
  };
  return { metadata, count: sides.length };
}

async function captureCase(browser, script, name, grid, out, hashes, expectedMetadata, runtime) {
  const page = await browser.newPage({ viewport: { width: 820, height: 780 }, deviceScaleFactor: 1 });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.setContent('<!doctype html><canvas></canvas>');
    await page.addScriptTag({ content: script });
    const prepared = await page.evaluate(prepareInPage, { name, divisor: grid.divisor });
    const metadata = { ...runtime, ...prepared.metadata };
    const mismatches = expectedMetadata ? compareMetadata(expectedMetadata, metadata) : [];
    if (mismatches.length) throw new Error(mismatches.join('\n'));
    for (let side = 0; side < prepared.count; side++) {
      let done = false;
      for (let attempt = 0; attempt < 2000 && !done; attempt++) {
        done = await page.evaluate(index => window.watercolorFrames.record(index), side);
      }
      if (!done) throw new Error(`${name}/${grid.name}: side ${side} never finished recording`);
    }
    for (let i = 0; i <= 60; i++) {
      const key = frameKey(name, grid.name, i);
      const png = await page.evaluate(index => window.watercolorFrames.capture(index), i);
      if (!png.startsWith('data:image/png;base64,')) throw new Error(`${key}: PNG capture failed`);
      const bytes = Buffer.from(png.split(',')[1], 'base64');
      const path = join(out, `${key}.png`);
      mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, bytes, { flag: 'wx' });
      hashes[key] = createHash('sha256').update(bytes).digest('hex');
    }
    if (errors.length) throw new Error(errors.join('\n'));
    await page.evaluate(() => window.watercolorFrames.dispose());
    console.log(`${name}/${grid.name}: 61 frames`);
    return metadata;
  } finally { await page.close(); }
}

export async function main(args = process.argv.slice(2)) {
  const { values } = parseArgs({ args, options: { write: { type: 'boolean' }, out: { type: 'string' }, help: { type: 'boolean' } } });
  if (values.help) {
    console.log('Usage: node scripts/watercolor-frames.mjs [--write] [--out DIR]\n610 alpha-preserving PNGs; DIR must be outside the repo (default: temp directory).\nDefault compares scripts/watercolor-frames.hashes.json. --write records an initial baseline.');
    return;
  }
  const outputParent = outsideRepository(values.out ?? tmpdir());
  if (!values.write && !existsSync(BASELINE)) throw new Error('Missing scripts/watercolor-frames.hashes.json; capture the initial baseline with --write on the pinned renderer');
  const expected = values.write ? null : JSON.parse(readFileSync(BASELINE, 'utf8'));
  if (!values.write && (!expected?.metadata || !expected?.frames ||
    typeof expected.metadata !== 'object' || typeof expected.frames !== 'object')) {
    throw new Error('Invalid baseline: metadata and frames objects are required');
  }
  const { chromium, version } = await pinnedPlaywright();
  const script = await bundleEngine();
  mkdirSync(outputParent, { recursive: true });
  const out = mkdtempSync(join(outputParent, 'watercolor-frames-'));
  console.log(`PNG evidence: ${out}`);
  const runtime = { schema: 1, playwright: version, chromium: '', angle: 'swiftshader',
    platform: process.platform, arch: process.arch, texW: 820, texH: 780, deviceScaleFactor: 1,
    hash: 'sha256-png', timeline: 'turn-at-half-resume-through-one' };
  const browser = await chromium.launch({ headless: true, args: ['--use-angle=swiftshader'] });
  const hashes = {};
  let metadata;
  try {
    runtime.chromium = browser.version();
    for (const name of CASES) for (const grid of GRIDS) {
      metadata = await captureCase(browser, script, name, grid, out, hashes, expected?.metadata ?? metadata, runtime);
    }
  } finally { await browser.close(); }
  const record = { metadata, frames: Object.fromEntries(Object.entries(hashes).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) };
  const json = JSON.stringify(record, null, 2) + '\n';
  writeFileSync(join(out, 'hashes.json'), json);
  const differences = compareFrames(expected?.frames ?? hashes, hashes);
  if (differences.length) throw new Error(differences.join('\n'));
  if (values.write) writeFileSync(BASELINE, json);
  console.log(`${values.write ? 'Recorded' : 'Matched'} ${Object.keys(hashes).length} frames; evidence: ${out}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
