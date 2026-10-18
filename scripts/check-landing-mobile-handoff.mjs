#!/usr/bin/env node
// On a phone the picture runs one leg per scene change under a canvas cover: the cover rises over the
// first tenth of the leg and falls over the last, the live stage switches scene only while the cover
// is up, and a leg can turn round or wait for a missing print without the reader seeing the stage move.
// The wash thins out before the canvas's edges, and scene 4's window-less stage shows no window frame.
import { pathToFileURL } from 'node:url';
const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium, webkit } = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const url = process.argv[2];
if (!url) throw Error('Usage: check-landing-mobile-handoff.mjs <url>');

const VIEWPORT = { width: 390, height: 844 };
const assert = (ok, message) => { if (!ok) throw Error(message); };
const open = async (browser, options = {}) => {
  const page = await browser.newPage({ viewport: VIEWPORT, isMobile: true, ...options });
  await page.goto(url);
  await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 60000 });
  return page;
};
const settle = (page, scene) => page.waitForFunction((s) => { const st = __state(); return st.shown === s && !st.leg && [0, 1, 2, 3].every((i) => __onHand[i]); }, scene, { timeout: 40000 });

// Scrolls to `to` and records every frame until the leg has arrived. `turn` ({ at, back }) scrolls to
// `back` once the leg is past p = `at`. `drop` removes a print first, in the same task as the scroll.
const trace = (page, { to, turn, drop }) => page.evaluate(async ({ to, turn, drop }) => {
  const cell = document.getElementById('cell'), gl = document.getElementById('gl'), frames = [];
  const scale = () => { const m = new DOMMatrix(getComputedStyle(cell).transform); return Math.hypot(m.a, m.b); };
  // How far below the page colour the wash is at the canvas's two edges a phone shows: ink that reaches
  // them is cut off in a line.
  const strip = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  const bg = getComputedStyle(document.body).backgroundColor.match(/\d+/g).map(Number);
  const edgeInk = () => {
    const w = gl.width, h = gl.height, n = Math.max(2, Math.round(h * .004)), out = {};
    for (const [side, sx, sy, sw, sh] of [['bottom', 0, h - n, w, n], ['right', w - n, 0, n, h]]) {
      strip.canvas.width = sw; strip.canvas.height = sh; strip.drawImage(gl, sx, sy, sw, sh, 0, 0, sw, sh);
      const d = strip.getImageData(0, 0, sw, sh).data; let worst = 0;
      for (let i = 0; i < d.length; i += 4) {
        const a = d[i + 3] / 255, lum = (c) => .2126 * c[0] + .7152 * c[1] + .0722 * c[2];
        worst = Math.max(worst, lum(bg) - lum([0, 1, 2].map((k) => d[i + k] + (1 - a) * bg[k])));
      }
      out[side] = Math.round(worst);
    }
    return out;
  };
  const groundShown = () => Math.max(...['g0', 'g1'].map((id) => Number(getComputedStyle(document.getElementById(id)).opacity))) * (1 - Number(stage.style.getPropertyValue('--wash-cover') || 0));
  const start = scale(), t0 = performance.now(), end = turn ? turn.back : to;
  if (drop != null) __onHand[drop] = null;
  scrollTo({ top: __restY(to), behavior: 'instant' });
  let turned = !turn, saw = false;
  for (;;) {
    await new Promise(requestAnimationFrame);
    const s = __state();
    if (performance.now() - t0 > 40000) throw Error(`the leg to scene ${to + 1} never arrived: ${JSON.stringify(s)}`);
    if (s.leg) saw = true;
    frames.push({ shown: s.shown, leg: s.leg, cover: Number(stage.style.getPropertyValue('--wash-cover') || 0), scene: Number(stage.dataset.scene), scale: scale(),
      edge: s.leg && s.leg.p > .02 && s.leg.p < .98 ? edgeInk() : null, ground: groundShown() });
    if (!turned && s.leg && s.leg.p >= turn.at) { turned = true; scrollTo({ top: __restY(turn.back), behavior: 'instant' }); }
    if (saw && turned && !s.leg && s.shown === end) break;
  }
  return { frames, start, end: scale(), mix: [PLAY_OUT, MossWatercolor.DEFAULT_PRESET.playIn] };
}, { to, turn, drop });

const smooth = (a, b, t) => { const x = Math.min(1, Math.max(0, (t - a) / (b - a))); return x * x * (3 - 2 * x); };
const zoom = (scene) => (scene === 3 ? 1.4 : 1);
// how far below the page colour a canvas edge may be (in 0-255 luminance) before it reads as a line
const EDGE_INK = 16;

function check(name, from, to, { frames, start, end, mix }, turned) {
  const legs = frames.filter((f) => f.leg);
  assert(legs.length > 3, `${name}: no leg ran`);
  for (const f of legs) {
    const want = Math.min(smooth(0, .1, f.leg.p), 1 - smooth(.9, 1, f.leg.p));
    assert(Math.abs(f.cover - want) < .05, `${name}: cover ${f.cover} at p ${f.leg.p}, expected ${want.toFixed(2)}`);
  }
  assert(legs.some((f) => f.cover > 0 && f.cover < 1), `${name}: the cover never passed through a part-way value`);
  // Until the cover is (nearly) opaque the live stage is still the leg's start; it may change only under it.
  if (!turned) for (const f of legs.filter((g) => g.cover < .9)) assert(f.scene === (f.leg.p < .5 ? from : to), `${name}: scene ${f.scene + 1} was live at p ${f.leg.p} with the cover at ${f.cover}`);
  // Scene 4's zoom follows the leg's own mix, and is only seen at an end's value: it moves under the cover.
  const unit = start / zoom(from);
  for (const f of legs) {
    const k = smooth(mix[0], mix[1], f.leg.p), want = unit * (zoom(from) + (zoom(to) - zoom(from)) * k);
    if (!turned) assert(Math.abs(f.scale - want) < .03 * want, `${name}: the zoom was ${f.scale.toFixed(3)} at p ${f.leg.p} (expected ${want.toFixed(3)})`);
    if (f.cover < .5) assert(Math.abs(f.scale - unit * zoom(f.leg.p < .5 ? from : to)) < .02 * f.scale, `${name}: the zoom was ${f.scale.toFixed(3)} with the cover at ${f.cover}`);
  }
  assert(Math.abs(end - unit * zoom(to)) < .02 * end, `${name}: the leg ended at zoom ${end} not ${unit * zoom(to)}`);
  // The wash thins out before the canvas's edge rather than stopping at it in a straight line.
  for (const f of legs.filter((g) => g.edge)) for (const side of ['bottom', 'right']) assert(f.edge[side] <= EDGE_INK, `${name}: the wash reached the canvas's ${side} edge at p ${f.leg.p} (${f.edge[side]} below the page)`);
  // Scene 4 stands under no window: as the cover falls onto it, no window frame or shadow shows beneath.
  for (const f of frames.filter((g) => g.scene === 3)) assert(f.ground < .02, `${name}: a window ground showed at ${f.ground.toFixed(2)} over scene 4 at p ${f.leg?.p}`);
  return legs;
}

for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    const page = await open(browser);
    await page.evaluate(() => scrollTo({ top: __restY(1), behavior: 'instant' }));
    await settle(page, 1);
    let at = 1;
    for (const to of [2, 3, 2]) {
      const run = await trace(page, { to });
      check(`${at + 1}->${to + 1}`, at, to, run);
      await settle(page, to);
      at = to;
    }

    // Turning round mid-leg plays the picture back to where it came from.
    const back = await trace(page, { to: 3, turn: { at: .3, back: 2 } });
    check('turn', 2, 2, back, true);
    const peak = Math.max(...back.frames.filter((f) => f.leg).map((f) => f.leg.p));
    const after = back.frames.slice(back.frames.findIndex((f) => f.leg?.p === peak));
    assert(!back.frames.some((f) => f.shown === 3), 'a reversed leg arrived at the scene it was leaving for');
    assert(after.some((f) => f.leg && f.leg.p < peak - .05), `a reversed leg did not run back: peak ${peak}`);
    await settle(page, 2);

    // A print the leg needs and nobody holds is taken under the cover; the reader never sees the stage change.
    check('2->1', 2, 1, await trace(page, { to: 1 }));
    await settle(page, 1);
    check('missing print', 1, 2, await trace(page, { to: 2, drop: 2 }));
    assert(await page.evaluate(() => !!__onHand[2]), 'the missing print was not taken');
    await page.close();

    // Taking a neighbour's print switches the live stage under a held canvas: the scene on screen keeps its size.
    const rest = await open(browser);
    await rest.evaluate(() => scrollTo({ top: __restY(1), behavior: 'instant' }));
    await settle(rest, 1);
    const retake = await rest.evaluate(async () => {
      const scales = new Set(), cell = document.getElementById('cell');
      let watching = true;
      const tick = () => { if (!watching) return; const m = new DOMMatrix(getComputedStyle(cell).transform); scales.add(Math.hypot(m.a, m.b).toFixed(3)); requestAnimationFrame(tick); };
      tick();
      __onHand[3] = null;
      const t0 = performance.now();
      while (!__onHand[3] && performance.now() - t0 < 20000) await new Promise((r) => setTimeout(r, 50));
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      watching = false;
      return { retaken: !!__onHand[3], scales: [...scales] };
    });
    assert(retake.retaken, 'scene 4 print was not retaken from scene 2');
    assert(retake.scales.length === 1, `scene 2 changed size while scene 4's print was taken: ${JSON.stringify(retake)}`);
    await rest.close();

    // Reduced motion cuts: no leg, no cover.
    const calm = await open(browser, { reducedMotion: 'reduce' });
    const cut = await calm.evaluate(async () => {
      scrollTo({ top: __restY(2), behavior: 'instant' });
      let legs = 0, cover = 0;
      for (let i = 0; i < 90; i++) {
        await new Promise(requestAnimationFrame);
        if (__state().leg) legs++;
        cover = Math.max(cover, Number(stage.style.getPropertyValue('--wash-cover') || 0));
      }
      return { legs, cover, shown: __state().shown };
    });
    assert(cut.legs === 0 && cut.cover === 0 && cut.shown === 2, `reduced motion did not cut: ${JSON.stringify(cut)}`);
    await calm.close();

    console.log(`${engine.name()}: cover follows the leg, stage switches under it, no wash at the canvas edge, no window over scene 4, turn plays back, missing print taken under cover, retake leaves the scene alone, reduced motion cuts`);
  } finally { await browser.close(); }
}
