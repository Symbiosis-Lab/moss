import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { runInNewContext } from 'node:vm';
import { bundleEngine, compareFrames, compareMetadata, frameKeys, main, outsideRepository,
  pinnedPlaywright, prepareInPage } from '../watercolor-frames.mjs';

test('all 610 frame identities are unique, sorted, and include both endpoints', () => {
  const keys = frameKeys();
  assert.equal(keys.length, 610);
  assert.equal(new Set(keys).size, 610);
  assert.deepEqual(keys, [...keys].sort());
  assert.ok(keys.includes('turn/phone-205x195/p00-of-60'));
  assert.ok(keys.includes('turn/desktop-410x390/p60-of-60'));
});

test('comparison names every changed, missing, and extra frame', () => {
  // Synthetic labels exercise comparison only; these are not rendered hashes.
  const frames = Object.fromEntries(frameKeys().map(key => [key, 'synthetic']));
  const expected = { ...frames }, [changed, missing] = frameKeys();
  expected[changed] = 'changed'; delete expected[missing]; expected['obsolete/grid/p0'] = 'extra';
  assert.deepEqual(compareFrames(expected, frames).sort(), [
    `different: ${changed}`, `missing baseline: ${missing}`, 'extra frame: obsolete/grid/p0',
  ].sort());
  assert.deepEqual(compareFrames(frames, frames), []);
  assert.equal(compareFrames({}, {}).length, 610);
  assert.deepEqual(compareFrames({ ...frames, extra: 'x' }, { ...frames, extra: 'x' }), ['extra frame: extra']);
  const absent = { ...frames }; delete absent[changed];
  assert.deepEqual(compareFrames(frames, absent), [`extra baseline / missing capture: ${changed}`]);
});

test('metadata comparison includes renderer and browser/version drift', () => {
  const expected = { renderer: 'SwiftShader A', playwright: '1.61.1', chromium: '1', schema: 1 };
  assert.deepEqual(compareMetadata(expected, { ...expected }), []);
  const errors = compareMetadata(expected, { ...expected, renderer: 'GPU', chromium: '2' });
  assert.equal(errors.length, 2);
  assert.match(errors[0], /metadata mismatch chromium/);
  assert.match(errors[1], /metadata mismatch renderer/);
});

test('output validation rejects repo paths, traversal and symlinks before browser loading', async t => {
  // All fixture writes stay in this checkout; the nested repo is the guard's test input.
  const dir = mkdtempSync(join(process.cwd(), '.watercolor-frames-test-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const repo = join(dir, 'repo'); mkdirSync(repo);
  symlinkSync(repo, join(dir, 'alias'));
  for (const path of [repo, join(repo, 'new/deep'), join(dir, 'alias/new'), join(dir, 'elsewhere/../repo/new')]) {
    assert.throws(() => outsideRepository(path, repo), /outside the repository/);
  }
  assert.equal(outsideRepository(join(dir, 'external/new'), repo), join(dir, 'external/new'));
  await assert.rejects(main(['--out', process.cwd()]), /outside the repository/);
  await assert.rejects(main(['--unknown']), /Unknown option/);
});

test('installed Playwright pin and in-memory engine bundle load without launching a browser', async () => {
  assert.equal((await pinnedPlaywright()).version, '1.61.1');
  const source = await bundleEngine();
  assert.match(source, /var MossWatercolor/);
  assert.match(source, /createSim/);
});

test('capture uses recorded sides, null incoming for to-nothing, and a baked midpoint turn', () => {
  for (const name of ['light', 'dark', 'transparent', 'to-nothing', 'turn']) {
    const calls = [], window = {}, gl = {
      UNMASKED_RENDERER_WEBGL: 'renderer', UNMASKED_VENDOR_WEBGL: 'vendor', VERSION: 'version',
      getExtension() { return this; }, getParameter(key) { return key === 'renderer' ? 'SwiftShader' : 'test'; },
      isContextLost() { return false; },
    };
    const context = new Proxy({}, { get: () => () => {} });
    const canvas = { getContext: type => type === 'webgl2' ? gl : context, toDataURL: () => 'data:image/png;base64,mock' };
    const document = { createElement: () => ({ ...canvas }), querySelector: () => canvas };
    let id = 0, options;
    const sim = {
      sceneSide(print, foot) { assert.notEqual(print, foot); return ++id; },
      recordSide(side, budget) { calls.push(['record', side, budget]); }, sideRecorded: () => true,
      playSides(...args) { calls.push(['play', ...args]); return true; },
      bake(...args) { calls.push(['bake', ...args]); return 'baked'; },
      freeSide() {}, dispose() {},
    };
    const MossWatercolor = { createSim(o) { options = o; return sim; },
      legAt: p => ({ ux: p, uy: 1 - p, k: p }), turnAt: () => ({ from: 'mix', p: .42 }) };
    const prepare = runInNewContext(`(${prepareInPage.toString()})`, { window, document, MossWatercolor });
    const result = prepare({ name, divisor: 4 });
    assert.equal(options.texW / options.divisor, 205);
    assert.equal(options.texH / options.divisor, 195);
    assert.equal(options.ground, ['light', 'dark'].includes(name) ? 'paper' : 'transparent');
    for (let i = 0; i < result.count; i++) window.watercolorFrames.record(i);
    for (const call of calls) assert.equal(call[2], 36);
    for (const i of [0, 29, 30, 45, 60]) window.watercolorFrames.capture(i);
    const plays = calls.filter(c => c[0] === 'play');
    if (name === 'to-nothing') for (const call of plays) {
      assert.equal(call[3], null); assert.equal(call[7].toNothing, true);
    }
    if (name === 'turn') {
      assert.equal(calls.filter(c => c[0] === 'bake').length, 1);
      assert.equal(calls.find(c => c[0] === 'bake')[6], .5);
      assert.equal(plays[1][1], 1);
      assert.equal(plays[2][1], 'baked'); assert.equal(plays[2][3], 3);
      assert.equal(plays[2][6], .42); assert.equal(plays[3][6], .71); assert.equal(plays[4][6], 1);
    }
    window.watercolorFrames.dispose();
  }
});

test('an existing wash baseline cannot be overwritten', async () => {
  await assert.rejects(main(['--write']), /immutable/);
});
