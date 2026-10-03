#!/usr/bin/env node
// A host that hands off between its own page and the watercolor canvas does so
// at p = 0 and p = 1, so what the canvas shows there must be the print itself:
// no refraction offset, no absorption change, no paper modulation. This check
// renders both ends of a recorded transition in real WebGL (headless Chromium's
// software rasterizer provides WebGL2 and float render targets), composites the
// canvas over the page colour, and compares it with the print it should be.
//
// Two numbers per end: the largest per-channel difference from the print
// (<= TOLERANCE of 255, which allows for resampling and 8-bit rounding), and
// the vertical offset, within +-SEARCH px, at which the canvas best matches the
// print (must be 0 -- a refraction shift shows up here even when the colours
// are close).
//
// Usage: node scripts/check-watercolor-endpoints.mjs
// Playwright is loaded from PLAYWRIGHT_MODULE, as in the other browser checks.
import { build } from 'esbuild';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { loadPlaywright } from './landing-harness.mjs';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const TOLERANCE = 2;
const SEARCH = 20;

const bundle = await build({
  stdin: {
    contents: "export * from './engine/index.js'; export * from './model/index.js';",
    resolveDir: `${ROOT}packages/moss-watercolor/src`,
    loader: 'ts',
  },
  tsconfig: `${ROOT}packages/moss-watercolor/tsconfig.json`,
  bundle: true, format: 'iife', globalName: 'MossWatercolor', target: 'es2020', write: false,
});
const script = bundle.outputFiles[0].text;

// Runs in the page. Two prints with sharp features (a grid, text, a coloured
// disc, diagonals) on the page colour, a recorded dispersion between them, and
// the canvas read back over the page colour at each end.
async function inPage({ tolerance, search, dark, ground }) {
  const W = 410, H = 390;
  const bg = getComputedStyle(document.documentElement).getPropertyValue('--bg').trim();
  const make = (draw) => { const c = document.createElement('canvas'); c.width = W; c.height = H; const g = c.getContext('2d'); g.fillStyle = bg; g.fillRect(0, 0, W, H); draw(g); return c; };
  // dark pages: the page colour is dark, and the prints are light marks on it
  const printA = make(dark ? (g) => {
    g.strokeStyle = '#d8dcd4'; g.lineWidth = 2;
    for (let x = 10; x < W; x += 20) { g.beginPath(); g.moveTo(x, 0); g.lineTo(x, H); g.stroke(); }
    for (let y = 10; y < H; y += 20) { g.beginPath(); g.moveTo(0, y); g.lineTo(W, y); g.stroke(); }
    g.fillStyle = '#ece6d8'; g.font = 'bold 64px serif'; g.fillText('Moss', 40, 220);
  } : (g) => {
    g.strokeStyle = '#202020'; g.lineWidth = 2;
    for (let x = 10; x < W; x += 20) { g.beginPath(); g.moveTo(x, 0); g.lineTo(x, H); g.stroke(); }
    for (let y = 10; y < H; y += 20) { g.beginPath(); g.moveTo(0, y); g.lineTo(W, y); g.stroke(); }
    g.fillStyle = '#181818'; g.font = 'bold 64px serif'; g.fillText('Moss', 40, 220);
  });
  const printB = make(dark ? (g) => {
    g.strokeStyle = '#b4c0ec'; g.lineWidth = 3;
    for (let i = -H; i < W; i += 26) { g.beginPath(); g.moveTo(i, 0); g.lineTo(i + H, H); g.stroke(); }
    g.fillStyle = '#d8643c'; g.beginPath(); g.arc(250, 130, 70, 0, 7); g.fill();
    g.fillStyle = '#e4ded0'; g.font = 'italic 56px serif'; g.fillText('Wash', 60, 300);
  } : (g) => {
    g.strokeStyle = '#1a2a60'; g.lineWidth = 3;
    for (let i = -H; i < W; i += 26) { g.beginPath(); g.moveTo(i, 0); g.lineTo(i + H, H); g.stroke(); }
    g.fillStyle = '#a02818'; g.beginPath(); g.arc(250, 130, 70, 0, 7); g.fill();
    g.fillStyle = '#101010'; g.font = 'bold 56px sans-serif'; g.fillText('Wash', 60, 300);
  });
  const canvas = document.getElementById('c');
  const sim = MossWatercolor.createSim({ canvas, texW: W, texH: H, divisor: 2, rect: () => ({ x: 0, y: 0, w: W, h: H }), ground });
  if (!sim) throw new Error('no WebGL2 + EXT_color_buffer_float');
  sim.setPrints(printA, printB);
  const pair = sim.pair();
  let guard = 0; while (sim.show(pair, true, 0.5)) if (++guard > 1000) throw new Error('recording never finished');

  const pixels = (src) => { const c = document.createElement('canvas'); c.width = W; c.height = H; const g = c.getContext('2d', { willReadFrequently: true }); g.fillStyle = bg; g.fillRect(0, 0, W, H); g.drawImage(src, 0, 0, W, H); return g.getImageData(0, 0, W, H).data; };
  const compare = (shown, print) => {
    let max = 0;
    for (let i = 0; i < shown.length; i += 4) for (let k = 0; k < 3; k++) max = Math.max(max, Math.abs(shown[i + k] - print[i + k]));
    // the best vertical shift: mean absolute difference over a margin that stays inside both images
    let best = 0, bestErr = Infinity;
    for (let dy = -search; dy <= search; dy++) {
      let err = 0, n = 0;
      for (let y = search; y < H - search; y += 1) for (let x = 0; x < W; x += 2) { const a = ((y + dy) * W + x) * 4, b = (y * W + x) * 4; err += Math.abs(shown[a] - print[b]) + Math.abs(shown[a + 1] - print[b + 1]) + Math.abs(shown[a + 2] - print[b + 2]); n += 3; }
      err /= n; if (err < bestErr) { bestErr = err; best = dy; }
    }
    return { max, offset: best };
  };
  const canvasLit = (src) => { const c = document.createElement('canvas'); c.width = W; c.height = H; const g = c.getContext('2d', { willReadFrequently: true }); g.drawImage(src, 0, 0, W, H); const d = g.getImageData(0, 0, W, H).data; let n = 0; for (let i = 3; i < d.length; i += 4) if (d[i] > 8) n++; return n; };
  // luminance spread and the bright tail of a frame, against the page's own luminance
  const luma = (r, g, b) => 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const frameStats = (shown) => {
    const hist = new Array(256).fill(0); let sum = 0, sum2 = 0, n = 0;
    for (let i = 0; i < shown.length; i += 4) { const l = luma(shown[i], shown[i + 1], shown[i + 2]); hist[Math.round(l)]++; sum += l; sum2 += l * l; n++; }
    let acc = 0, p99 = 0; for (let v = 0; v < 256; v++) { acc += hist[v]; if (acc >= 0.99 * n) { p99 = v; break; } }
    const mean = sum / n; return { std: Math.sqrt(Math.max(0, sum2 / n - mean * mean)), p99 };
  };
  const paperLuma = (() => { const n = parseInt(bg.slice(1), 16); return luma(n >> 16, (n >> 8) & 255, n & 255); })();
  if (ground === 'transparent') {
    // A host whose page is a live background behind the canvas: the canvas is the print at either end
    // (opaque), pigment over nothing in between, and clear at the middle of the leg.
    const readAlpha = (src) => { const c = document.createElement('canvas'); c.width = W; c.height = H; const g = c.getContext('2d', { willReadFrequently: true }); g.drawImage(src, 0, 0, W, H); const d = g.getImageData(0, 0, W, H).data; let sum = 0, min = 255, lit = 0; for (let i = 3; i < d.length; i += 4) { sum += d[i]; min = Math.min(min, d[i]); if (d[i] > 20) lit++; } return { mean: sum / (d.length / 4) / 255, min, lit }; };
    const pix = (src) => { const c = document.createElement('canvas'); c.width = W; c.height = H; const g = c.getContext('2d', { willReadFrequently: true }); g.fillStyle = bg; g.fillRect(0, 0, W, H); g.drawImage(src, 0, 0, W, H); return g.getImageData(0, 0, W, H).data; };
    const alone = (print, s = sim) => { s.setPrints(print, print); const rec = s.pair({ only: true }); let n = 0; while (s.show(rec, true, 0.5)) if (++n > 1000) throw new Error('recording never finished'); return rec; };
    const recA = alone(printA), recB = alone(printB);
    sim.setPrints(printA, printB);
    recA.sides[0].fwd = true; recB.sides[0].fwd = false;
    const composed = { sides: [recA.sides[0], recB.sides[0]] };
    const wantA = pix(printA), wantB = pix(printB);
    const results = [], png = {};
    for (const [p, want, name] of [[0, wantA, 'A'], [1, wantB, 'B']]) {
      sim.show(composed, true, p);
      const al = readAlpha(canvas), cmp = compare(pix(canvas), want);
      results.push({ case: `transparent ground, show(p=${p}) vs print ${name}: alpha min ${al.min}`, ...cmp, ok: cmp.max <= tolerance && cmp.offset === 0 && al.min === 255 });
    }
    // a host's page is rarely the colour the sim was given: a print on its own paper is still exact at the ends
    const printC = document.createElement('canvas'); printC.width = W; printC.height = H;
    { const g = printC.getContext('2d'); g.fillStyle = dark ? '#050505' : '#ffffff'; g.fillRect(0, 0, W, H); g.strokeStyle = dark ? '#c8d8ff' : '#203060'; g.lineWidth = 3; for (let x = 10; x < W; x += 30) { g.beginPath(); g.moveTo(x, 0); g.lineTo(x, H); g.stroke(); } g.fillStyle = dark ? '#f0c060' : '#703010'; g.font = 'bold 60px serif'; g.fillText('Own', 50, 200); }
    const recC = alone(printC);
    sim.setPrints(printC, printB);
    recC.sides[0].fwd = true; recB.sides[0].fwd = false;
    sim.show({ sides: [recC.sides[0], recB.sides[0]] }, true, 0);
    { const al = readAlpha(canvas), cmp = compare(pix(canvas), pix(printC));
      results.push({ case: `transparent ground, show(p=0) on a print whose paper is not --bg: alpha min ${al.min}`, ...cmp, ok: cmp.max <= tolerance && cmp.offset === 0 && al.min === 255 }); }
    sim.setPrints(printA, printB); recB.sides[0].fwd = false;
    // the default floor leaves a translucent film at the middle of the leg: neither cut out nor opaque
    sim.show(composed, true, 0.5);
    { const al = readAlpha(canvas);
      results.push({ case: `transparent ground, show(p=0.5), default drainFloor: mean alpha ${al.mean.toFixed(3)}, ${al.lit} px of pigment (partial)`, max: 0, offset: 0, ok: al.mean > 0.03 && al.mean < 0.95 && al.lit > 2000 }); }
    // a floor of 0 clears the canvas at the middle, so the host's own background shows untouched
    { const canvas0 = document.createElement('canvas');
      const sim0 = MossWatercolor.createSim({ canvas: canvas0, texW: W, texH: H, divisor: 2, rect: () => ({ x: 0, y: 0, w: W, h: H }), ground, preset: { drainFloor: 0 } });
      const recA0 = alone(printA, sim0), recB0 = alone(printB, sim0);
      sim0.setPrints(printA, printB); recA0.sides[0].fwd = true; recB0.sides[0].fwd = false;
      sim0.show({ sides: [recA0.sides[0], recB0.sides[0]] }, true, 0.5);
      const al = readAlpha(canvas0);
      results.push({ case: `transparent ground, show(p=0.5), drainFloor 0: mean alpha ${al.mean.toFixed(4)} (grid fully visible)`, max: 0, offset: 0, ok: al.mean <= 0.02 }); }
    // the film thins and thickens smoothly: the change between neighbouring frames has no jump
    { const premult = (src) => { const c = document.createElement('canvas'); c.width = W; c.height = H; const g = c.getContext('2d', { willReadFrequently: true }); g.drawImage(src, 0, 0, W, H); const d = g.getImageData(0, 0, W, H).data; const o = new Float32Array(d.length); for (let i = 0; i < d.length; i += 4) { const a = d[i + 3] / 255; o[i] = d[i] * a; o[i + 1] = d[i + 1] * a; o[i + 2] = d[i + 2] * a; o[i + 3] = d[i + 3]; } return o; };
      const ps = [0.4, 0.45, 0.5, 0.55, 0.6], frames = ps.map((p) => { sim.show(composed, true, p); return premult(canvas); });
      const deltas = []; for (let k = 1; k < frames.length; k++) { let sum = 0; for (let i = 0; i < frames[k].length; i++) sum += Math.abs(frames[k][i] - frames[k - 1][i]); deltas.push(sum / frames[k].length); }
      const smooth = deltas.every((d, k) => d > 0 && d <= 2 * Math.max(...[deltas[k - 1], deltas[k + 1]].filter((v) => v !== undefined)));
      results.push({ case: `transparent ground, frame-to-frame change across p=${ps.join(', ')}: ${deltas.map((d) => d.toFixed(2)).join(', ')} (no jump over 2x a neighbour)`, max: 0, offset: 0, ok: smooth }); }
    // Pigment over nothing is the same wash as pigment over the page: composited over the page colour,
    // a transparent frame is the paper-ground frame of the same recording at the same position (with the
    // drain off, which the paper ground has no use for). Inside the endpoint margin only on a dark page:
    // there the paper ground blends the plain print in as well, which a light page's paper ground does not
    // (and a light transparent ground does, so a print on a slightly off-white paper still ends exact).
    const mk = (opts) => { const cv = document.createElement('canvas'); return { cv, s: MossWatercolor.createSim({ canvas: cv, texW: W, texH: H, divisor: 2, rect: () => ({ x: 0, y: 0, w: W, h: H }), ...opts }) }; };
    const compose = (s) => { const a = alone(printA, s), b = alone(printB, s); s.setPrints(printA, printB); a.sides[0].fwd = true; b.sides[0].fwd = false; return { sides: [a.sides[0], b.sides[0]] }; };
    const flat = mk({ ground, preset: { drainFloor: 1 } }), paper = mk({ ground: 'paper' });
    const flatLeg = compose(flat.s), paperLeg = compose(paper.s);
    const maxDiff = (x, y) => { let m = 0; for (let i = 0; i < x.length; i += 4) for (let k = 0; k < 3; k++) m = Math.max(m, Math.abs(x[i + k] - y[i + k])); return m; };
    const gl = canvas.getContext('webgl2');
    const premultiplied = () => { const b = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4); gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, b); return b; };
    for (const p of [0.03, 0.25, 0.5, 0.75]) {
      flat.s.show(flatLeg, true, p); paper.s.show(paperLeg, true, p);
      const d = maxDiff(pix(flat.cv), pix(paper.cv));
      if (p > 0.1 || dark) results.push({ case: `transparent ground over the page colour, show(p=${p}) vs the paper ground's frame: max diff ${d}/255`, max: d, offset: 0, ok: d <= 4 });
      // a premultiplied pixel can never carry more colour than coverage
      sim.show(composed, true, p);
      const b = premultiplied(); let over = 0; for (let i = 0; i < b.length; i += 4) for (let k = 0; k < 3; k++) if (b[i + k] > b[i + 3] + 1) over++;
      results.push({ case: `transparent ground, show(p=${p}): ${over} channels brighter than their alpha`, max: over, offset: 0, ok: over === 0 });
    }
    // a hair inside the margin the page's paper is still mostly there: no pixel is nearly clear (what keeps the host's background from showing through a leg that has barely begun)
    sim.show(composed, true, 0.03);
    { const b = premultiplied(); let minA = 255; for (let i = 3; i < b.length; i += 4) minA = Math.min(minA, b[i]);
      results.push({ case: `transparent ground, show(p=0.03): alpha min ${minA} (the paper still covers the background)`, max: minA, offset: 0, ok: minA >= 0.85 * 255 }); }
    // A leg whose incoming print is not on hand ends on nothing: the outgoing print is exact at p = 0, its
    // pigment is gone from the middle on (the host fades its own incoming page in over the clear canvas),
    // and none of the incoming print is drawn.
    { const out = { sides: [recA.sides[0], sim.pair({ only: true }).sides[1]], toNothing: true };
      sim.setPrints(printA, printB); recA.sides[0].fwd = true;
      sim.show(out, true, 0);
      { const al = readAlpha(canvas), cmp = compare(pix(canvas), wantA);
        results.push({ case: `transparent ground, leg to nothing, show(p=0) vs print A: alpha min ${al.min}`, ...cmp, ok: cmp.max <= tolerance && cmp.offset === 0 && al.min === 255 }); }
      sim.show(out, true, 0.25);
      { const b = premultiplied(); let sum = 0, over = 0; for (let i = 0; i < b.length; i += 4) { sum += b[i + 3]; for (let k = 0; k < 3; k++) if (b[i + k] > b[i + 3] + 1) over++; }
        results.push({ case: `transparent ground, leg to nothing, show(p=0.25): mean alpha ${(sum / (b.length / 4) / 255).toFixed(3)}, ${over} channels brighter than their alpha`, max: 0, offset: 0, ok: sum / (b.length / 4) / 255 > 0.03 && over === 0 }); }
      for (const p of [0.5, 0.75, 0.9, 1]) {
        sim.show(out, true, p);
        const b = premultiplied(); let maxA = 0, maxC = 0; for (let i = 0; i < b.length; i += 4) { maxA = Math.max(maxA, b[i + 3]); maxC = Math.max(maxC, b[i], b[i + 1], b[i + 2]); }
        results.push({ case: `transparent ground, leg to nothing, show(p=${p}): alpha max ${maxA}, colour max ${maxC} (clear)`, max: maxA, offset: 0, ok: maxA <= 0.02 * 255 && maxC <= maxA + 1 });
      }
      recB.sides[0].fwd = false; }
    for (const p of [0.2, 0.8]) {
      sim.show(composed, true, p); const al = readAlpha(canvas);
      results.push({ case: `transparent ground, show(p=${p}): mean alpha ${al.mean.toFixed(3)}, ${al.lit} px of pigment`, max: 0, offset: 0, ok: al.mean > 0.03 && al.mean < 0.95 && al.lit > 2000 });
    }
    for (const p of [0.25, 0.5]) { sim.show(composed, true, p); png[p] = canvas.toDataURL('image/png'); }
    return { results, png };
  }
  const png = {};
  const wantA = pixels(printA), wantB = pixels(printB);
  const results = [];
  for (const [fwd, p, want, name] of [[true, 0, wantA, 'A'], [true, 1, wantB, 'B'], [false, 0, wantB, 'B'], [false, 1, wantA, 'A']]) {
    sim.show(pair, fwd, p);
    results.push({ case: `show(fwd=${fwd}, p=${p}) vs print ${name}`, ...compare(pixels(canvas), want) });
  }
  // a pair recorded for one side only (a boundary whose incoming print is not on hand yet)
  // still ends on the incoming print
  const half = sim.pair({ only: true });
  guard = 0; while (sim.show(half, true, 0.5)) if (++guard > 1000) throw new Error('recording never finished');
  for (const [p, want, name] of [[0, wantA, 'A'], [1, wantB, 'B']]) {
    sim.show(half, true, p);
    results.push({ case: `show(one-sided pair, p=${p}) vs print ${name}`, ...compare(pixels(canvas), want) });
  }
  // the stepped display: whatever water the film holds and however far the cure has got, the ends are the prints
  sim.reset(); sim.step(true, 0, 0);
  sim.draw(true, 0, -0.2, 0);
  results.push({ case: 'draw(p=0) after one step vs print A', ...compare(pixels(canvas), wantA) });
  for (let i = 1; i < 60; i++) sim.step(true, i / 120, 0.3);
  sim.draw(true, 0.3, -0.2, 1);
  results.push({ case: 'draw(p=1, cure 0.3) with water on the sheet vs print B', ...compare(pixels(canvas), wantB) });
  // a hair inside the ends the wash has hardly started: no slide, and no jump in colour
  for (const [p, want, name] of [[0.01, wantA, 'A'], [0.99, wantB, 'B']]) {
    sim.show(pair, true, p);
    results.push({ case: `show(p=${p}) vs print ${name}: continuous with the end`, ...compare(pixels(canvas), want), nearEnd: true });
  }
  // a host that records each print alone and composes two of them for a leg by flipping `fwd`
  const alone = (print) => {
    sim.setPrints(print, print);
    const rec = sim.pair({ only: true });
    let n = 0; while (sim.show(rec, true, 0.5)) if (++n > 1000) throw new Error('recording never finished');
    return rec;
  };
  const recA = alone(printA), recB = alone(printB);
  sim.setPrints(printA, printB);
  const outSide = recA.sides[0], inSide = recB.sides[0];
  outSide.fwd = true; inSide.fwd = false;
  const composed = { sides: [outSide, inSide] };
  for (const p of [0, 0.02, 0.25, 0.5, 0.75, 0.98, 1]) {
    sim.show(composed, true, p);
    const shown = pixels(canvas);
    const r = { case: `composed show(p=${p}) vs print ${p < 0.5 ? 'A' : 'B'}`, ...compare(shown, p < 0.5 ? wantA : wantB) };
    if (p === 0) results.push({ ...r, case: 'composed show(p=0) vs print A' });
    else if (p === 1) results.push({ ...r, case: 'composed show(p=1) vs print B' });
    else if (dark && p >= 0.25 && p <= 0.75) {
      // mid-leg on a dark page (the flat frame the page used to show has std 0 and a 99th percentile at the paper's): neither a flat frame nor either print; light pigment visible over the paper
      const st = frameStats(shown);
      results.push({ case: `dark composed show(p=${p}): luminance std ${st.std.toFixed(1)}, 99th percentile ${st.p99} (paper ${paperLuma.toFixed(0)})`, ...st, paperLuma, stats: true });
      results.push({ case: `dark composed show(p=${p}) vs print A (must differ)`, ...compare(shown, wantA), expectDifferent: true });
      results.push({ case: `dark composed show(p=${p}) vs print B (must differ)`, ...compare(shown, wantB), expectDifferent: true });
      if (p === 0.25 || p === 0.5) png[p] = canvas.toDataURL('image/png');
    } else {
      // mid-leg the canvas must hold wash: not transparent, and not the plain page
      const rawLit = canvasLit(canvas);
      results.push({ case: `composed show(p=${p}): canvas has wash content`, max: rawLit, offset: 0, wash: true });
    }
  }
  // and the way back: the same two recordings, roles swapped, the prints slotted the other way round
  sim.setPrints(printB, printA);
  recB.sides[0].fwd = true; recA.sides[0].fwd = false;
  const back = { sides: [recB.sides[0], recA.sides[0]] };
  for (const [p, want, name] of [[0, wantB, 'B'], [1, wantA, 'A']]) {
    sim.show(back, true, p);
    results.push({ case: `composed back show(p=${p}) vs print ${name}`, ...compare(pixels(canvas), want) });
  }
  sim.show(back, true, 0.5);
  results.push({ case: 'composed back show(p=0.5): canvas has wash content', max: canvasLit(canvas), offset: 0, wash: true });
  // a preset that leaves the margin out must not blank the wash
  const bare = document.createElement('canvas'); bare.style.position = 'absolute'; document.body.appendChild(bare);
  const sim2 = MossWatercolor.createSim({ canvas: bare, texW: W, texH: H, divisor: 2, rect: () => ({ x: 0, y: 0, w: W, h: H }), preset: { endpointMargin: undefined } });
  sim2.setPrints(printA, printB); sim2.reset(); sim2.step(true, 0, 0); sim2.draw(true, 0, -0.2, 0.5);
  results.push({ case: 'draw(p=0.5) with a preset that has no endpointMargin: canvas has wash content', max: canvasLit(bare), offset: 0, wash: true });
  // the middle must really differ, or the comparison above proves nothing
  sim.show(pair, true, 0.5);
  results.push({ case: 'show(fwd=true, p=0.5) vs print A (must differ)', ...compare(pixels(canvas), wantA), expectDifferent: true });
  return { results, png };
}

// A page colour with a channel at 1 (pure blue here) has no complement in that channel to take density
// from: dividing by it must not blow the wash up. Light marks on the page that sit at 1 in that channel
// too (every pixel of the sheet itself does) are a clear sheet, not a dense one.
async function saturatedInPage({ tolerance }) {
  const W = 200, H = 180;
  const bg = getComputedStyle(document.documentElement).getPropertyValue('--bg').trim();
  const make = (draw) => { const c = document.createElement('canvas'); c.width = W; c.height = H; const g = c.getContext('2d'); g.fillStyle = bg; g.fillRect(0, 0, W, H); draw(g); return c; };
  const printA = make((g) => { g.fillStyle = '#80c0ff'; g.fillRect(20, 20, 70, 60); g.fillStyle = '#ffffff'; g.fillRect(100, 90, 70, 60); });
  const printB = make((g) => { g.fillStyle = '#c0e0ff'; g.beginPath(); g.arc(100, 90, 50, 0, 7); g.fill(); });
  const canvas = document.getElementById('c');
  const sim = MossWatercolor.createSim({ canvas, texW: W, texH: H, divisor: 2, rect: () => ({ x: 0, y: 0, w: W, h: H }) });
  if (!sim) throw new Error('no WebGL2 + EXT_color_buffer_float');
  sim.setPrints(printA, printB);
  const pair = sim.pair();
  let guard = 0; while (sim.show(pair, true, 0.5)) if (++guard > 1000) throw new Error('recording never finished');
  const pixels = (src) => { const c = document.createElement('canvas'); c.width = W; c.height = H; const g = c.getContext('2d', { willReadFrequently: true }); g.fillStyle = bg; g.fillRect(0, 0, W, H); g.drawImage(src, 0, 0, W, H); return g.getImageData(0, 0, W, H).data; };
  const maxDiff = (x, y) => { let m = 0; for (let i = 0; i < x.length; i += 4) for (let k = 0; k < 3; k++) m = Math.max(m, Math.abs(x[i + k] - y[i + k])); return m; };
  const results = [], png = {};
  for (const [p, want, name] of [[0, printA, 'A'], [1, printB, 'B']]) {
    sim.show(pair, true, p);
    const d = maxDiff(pixels(canvas), pixels(want));
    results.push({ case: `saturated page, show(p=${p}) vs print ${name}: max diff ${d}/255`, max: d, offset: 0, ok: d <= tolerance });
  }
  // mid-leg the sheet's own blue stays full (a non-finite density would land as 0 or garbage there), and
  // the light marks are still pigment over it
  for (const p of [0.25, 0.5, 0.75]) {
    sim.show(pair, true, p); png[p] = canvas.toDataURL('image/png');
    const d = pixels(canvas); let darkBlue = 0, other = 0;
    for (let i = 0; i < d.length; i += 4) { if (d[i + 2] < 250) darkBlue++; if (d[i] > 8 || d[i + 1] > 8) other++; }
    results.push({ case: `saturated page, show(p=${p}): ${darkBlue} px lost their blue, ${other} px carry pigment`, max: darkBlue, offset: 0, ok: darkBlue === 0 && other > 500 });
  }
  return { results, png };
}

const { chromium } = await loadPlaywright();
const browser = await chromium.launch();
let failed = false;
try {
  for (const [dark, bg, ground, saturated] of [[false, '#f4efe6'], [true, '#1d201e'], [false, '#f4efe6', 'transparent'], [true, '#1d201e', 'transparent'], [true, '#0000ff', undefined, true]]) {
  const page = await browser.newPage({ viewport: { width: 500, height: 450 }, deviceScaleFactor: 1 });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.setContent(`<!doctype html><style>:root{--bg:${bg}}body{margin:0;background:var(--bg)}canvas{position:absolute}</style><canvas id="c"></canvas>`);
  await page.addScriptTag({ content: script });
  const { results, png } = await page.evaluate(saturated ? saturatedInPage : inPage, { tolerance: TOLERANCE, search: SEARCH, dark, ground });
  if (errors.length) throw new Error(errors.join('\n'));
  if (process.env.WATERCOLOR_PNG_DIR) for (const [p, url] of Object.entries(png)) writeFileSync(`${process.env.WATERCOLOR_PNG_DIR}/${dark ? 'dark' : 'light'}${ground ? '-transparent' : ''}-p${Math.round(p * 100)}.png`, Buffer.from(url.split(',')[1], 'base64'));
  console.log(`-- ${dark ? 'dark' : 'light'} page (${bg})${ground ? `, ground: ${ground}` : ''}`);
  for (const r of results) {
    const ok = r.ok !== undefined ? r.ok : r.stats ? r.std >= 1 && r.p99 >= r.paperLuma + 25 : r.wash ? r.max > 1000 : r.expectDifferent ? r.max > 32 : r.nearEnd ? r.max <= 24 && r.offset === 0 : r.max <= TOLERANCE && r.offset === 0;
    if (!ok) failed = true;
    console.log(r.stats || r.ok !== undefined ? `${ok ? 'ok  ' : 'FAIL'} ${r.case}` : r.wash ? `${ok ? 'ok  ' : 'FAIL'} ${r.case}: ${r.max} non-transparent px` : `${ok ? 'ok  ' : 'FAIL'} ${r.case}: max diff ${r.max}/255, best vertical offset ${r.offset} px`);
  }
  await page.close();
  }
} finally {
  await browser.close();
}
if (failed) { console.error(`endpoints are not the prints (tolerance ${TOLERANCE}/255, offset 0)`); process.exit(1); }
