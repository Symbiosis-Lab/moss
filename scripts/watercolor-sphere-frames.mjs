// The optional renderer's evidence is separate from the immutable wash baseline.
import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

export const SPHERE_CASES = ['light', 'dark', 'transparent'];
export const sphereKeys = grids => SPHERE_CASES.flatMap(name => grids.flatMap(grid =>
  Array.from({ length: 61 }, (_, i) => `sphere-${name}/${grid.name}/p${String(i).padStart(2, '0')}-of-60`))).sort();

export function compareSphereFrames(expected, actual, grids) {
  const planned = new Set(sphereKeys(grids));
  return [...new Set([...planned, ...Object.keys(expected), ...Object.keys(actual)])].sort().flatMap(key =>
    !planned.has(key) ? [`extra sphere frame: ${key}`] : !expected[key] ? [`missing sphere baseline: ${key}`]
      : expected[key] !== actual[key] ? [`different sphere frame: ${key}`] : []);
}

// Runs in the same page as prepareInPage; only the fixture exposes its inputs.
export function prepareSphereInPage() {
  const { sim, canvas, sides: [a, b] } = window.watercolorFrames.sphereInput;
  const sphere = MossWatercolor.createSphere(sim), gl = canvas.getContext('webgl2');
  const draw = (p, t) => {
    const pose = MossWatercolor.sphereLegAt(p), { ux, uy, k } = pose;
    const bounds = MossWatercolor.DEFAULT_SPHERE_BOUNDS, preset = MossWatercolor.DEFAULT_PRESET;
    const washP = p < bounds.outEnd ? ux * preset.playOut : p > bounds.inStart
      ? 1 - uy * (1 - preset.playIn) : preset.playOut + k * (preset.playIn - preset.playOut);
    if (!sim.playSides(a, ux, b, uy, k, washP) || !sphere.draw(sim.pigmentAt(a, ux), sim.pigmentAt(b, uy), { ...pose, t })) {
      throw new Error('Sphere frame not ready');
    }
  };
  const png = () => {
    if (gl.isContextLost() || gl.getError() !== gl.NO_ERROR) throw new Error('Sphere WebGL error');
    return canvas.toDataURL('image/png');
  };
  window.sphereFrames = {
    capture(i) { draw(i / 60, i / 30); return png(); },
    release(i) {
      // A pristine In frame is the oracle. Exercise a real hold, then replay
      // exactly that In frame with the sphere flattened and fully handed back.
      const p = MossWatercolor.DEFAULT_PRESET.playIn + (1 - MossWatercolor.DEFAULT_PRESET.playIn) * i / 30;
      const { ux, uy, k } = MossWatercolor.legAt(p);
      sim.playSides(a, ux, b, uy, k, p); const before = png();
      for (let j = 0; j < 8; j++) draw(.34 + .32 * j / 7, j * .5);
      sim.playSides(a, ux, b, uy, k, p);
      sphere.draw(sim.pigmentAt(a, ux), sim.pigmentAt(b, uy), { k, lift: 0, grain: 0, t: 4 });
      return { before, after: png() };
    },
    async intervals(spherical) {
      const samples = [];
      let previous;
      for (let i = 0; i < 141; i++) {
        const now = await new Promise(resolve => requestAnimationFrame(resolve));
        if (i > 20) samples.push(now - previous);
        previous = now;
        if (spherical) draw(.5, now / 1000);
        else sim.playSides(a, 1, b, 1, .5, .5);
      }
      samples.sort((a, b) => a - b);
      return { p95: samples[Math.ceil(samples.length * .95) - 1], frames: samples.length };
    },
    dispose() { sphere.dispose(); },
  };
}

function storePNG(out, key, png) {
  if (!png.startsWith('data:image/png;base64,')) throw new Error(`${key}: PNG capture failed`);
  const bytes = Buffer.from(png.split(',')[1], 'base64'), path = join(out, `${key}.png`);
  mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, bytes, { flag: 'wx' });
  return createHash('sha256').update(bytes).digest('hex');
}

export async function captureSphereCase(page, name, grid, out, hashes, timing) {
  await page.evaluate(prepareSphereInPage);
  try {
    for (let i = 0; i <= 60; i++) {
      const key = `sphere-${name}/${grid.name}/p${String(i).padStart(2, '0')}-of-60`;
      hashes[key] = storePNG(out, key, await page.evaluate(i => window.sphereFrames.capture(i), i));
    }
    for (let i = 0; i <= 30; i++) {
      const key = `release-${name}/${grid.name}/in${i}`;
      const { before, after } = await page.evaluate(i => window.sphereFrames.release(i), i);
      const original = storePNG(out, `${key}-before`, before), resumed = storePNG(out, `${key}-after`, after);
      if (original !== resumed) throw new Error(`Post-hold In differs: ${key}`);
    }
    console.log(`sphere-${name}/${grid.name}: 61 frames; 31 post-hold In frames identical`);
    if (timing && grid.divisor === 4) {
      const wash = await page.evaluate(() => window.sphereFrames.intervals(false));
      const sphere = await page.evaluate(() => window.sphereFrames.intervals(true));
      const result = { name, grid: grid.name, wash, sphere };
      writeFileSync(join(out, `timing-${name}-${grid.name}.json`), JSON.stringify(result, null, 2) + '\n');
      console.log(JSON.stringify(result));
      if (sphere.p95 > wash.p95 * 1.25 + 1) throw new Error('Sphere p95 exceeds wash by more than 25% + 1ms');
    }
  } finally { await page.evaluate(() => window.sphereFrames.dispose()); }
}
