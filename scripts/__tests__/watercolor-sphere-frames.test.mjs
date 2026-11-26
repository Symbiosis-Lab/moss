import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runInNewContext } from 'node:vm';
import { GRIDS } from '../watercolor-frames.mjs';
import { compareSphereFrames, sphereKeys, prepareSphereInPage } from '../watercolor-sphere-frames.mjs';

test('sphere evidence adds 366 identities without changing the wash inventory', () => {
  const keys = sphereKeys(GRIDS);
  assert.equal(keys.length, 366); assert.equal(new Set(keys).size, 366);
  const frames = Object.fromEntries(keys.map(key => [key, 'synthetic']));
  assert.deepEqual(compareSphereFrames(frames, frames, GRIDS), []);
  assert.equal(compareSphereFrames({}, {}, GRIDS).length, 366);
  const actual = { ...frames, extra: 'x', [keys[0]]: 'changed' }; delete actual[keys[1]];
  assert.deepEqual(compareSphereFrames(frames, actual, GRIDS).sort(), [
    'extra sphere frame: extra', `different sphere frame: ${keys[0]}`, `different sphere frame: ${keys[1]}`,
  ].sort());
});

test('each post-hold comparison replays the pristine In with lift and grain zero', () => {
  const calls = [], gl = { NO_ERROR: 0, getError: () => 0, isContextLost: () => false };
  const sim = { pigmentAt: (side, u) => ({ side, u }), playSides(...args) { calls.push(['wash', ...args]); return true; } };
  const canvas = { getContext: () => gl, toDataURL: () => 'data:image/png;base64,mock' };
  const window = { watercolorFrames: { sphereInput: { sim, canvas, sides: ['a', 'b'] } } };
  const MossWatercolor = {
    createSphere: () => ({ draw(...args) { calls.push(['sphere', ...args]); return true; }, dispose() {} }),
    sphereLegAt: p => ({ ux: .55, uy: .55, k: p, lift: 1, grain: 1 }),
    legAt: p => ({ ux: 1, uy: (1 - p) / .42, k: 1 }),
    DEFAULT_PRESET: { playIn: .58, playOut: .42 },
    DEFAULT_SPHERE_BOUNDS: { outEnd: .18, inStart: .82 },
  };
  runInNewContext(`(${prepareSphereInPage.toString()})`, { window, MossWatercolor })();
  for (const i of [0, 15, 30]) {
    calls.length = 0;
    const result = window.sphereFrames.release(i);
    assert.equal(result.before, result.after);
    const washes = calls.filter(c => c[0] === 'wash'), spheres = calls.filter(c => c[0] === 'sphere');
    assert.deepEqual(washes[0], washes.at(-1));
    assert.equal(spheres.length, 9);
    assert.equal(spheres.at(-1)[3].lift, 0); assert.equal(spheres.at(-1)[3].grain, 0);
    assert.equal(spheres.at(-1)[3].k, 1);
  }
});
