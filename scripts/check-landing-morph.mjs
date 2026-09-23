#!/usr/bin/env node
// Measures whether a scene's dissolve/consolidate wash actually carries the
// outgoing scene's own ink into the incoming scene, for every real
// transition the page defines, in both directions, at desktop and phone
// viewports. This is a measurement script, not a regression gate: it prints
// a table and writes a JSON dump plus filmstrips, and only fails the
// process (non-zero exit) if a page error is thrown or a capture step
// itself breaks -- clause pass/fail is reported, not asserted, because the
// owner asked for numbers to set a real threshold from, not a red/green
// gate tuned after the fact.
//
// Owner report (2026-09-22, paraphrased): scene 2 doesn't consolidate from
// a colourful/dark-enough wash out of scene 1; scene 3 doesn't start from
// the corresponding wash and shows no purple; scene 4 doesn't dissolve at
// all. Background: scratchpad/unit6-membership-spec.md section 1 (the
// membership fix, not landed at this commit -- grep confirms no
// wash-members/refreshVolatile/VOLATILE in site/landing.js) and
// scratchpad/forensics/findings.md item 4 ("not a regression; identical in
// BEFORE... unfinished membership work").
//
// Scene count and transitions, read from the code rather than assumed:
// site/index.html has five `.scene` sections (c1..c5), matching PHASE =
// ['write','live','ships','deploy','share'] in site/landing.js, and JOINS
// has four entries. The real legs are 0-1, 1-2, 2-3, 3-4 -- not five. The
// brief that commissioned this script asked for a fifth "4->5" leg too;
// scene position 5 does not exist (restY(5) reads past scenesEl's own five
// elements and throws), and forensics/probes/probe-progress-grid.mjs
// already enumerates the identical four legs independently, from the same
// investigation. This script tests all four real legs, both directions
// (2 layouts x 2 engines x 4 legs x 2 directions = 32 direction-runs), and
// says so in the report rather than fabricating a leg that isn't there.
//
// JOINS[3] (deploy->share) is a plain opacity crossfade (fade(), driven by
// xfAt()/--xf) -- there is no #gl wash for it. Clauses 1/2/5 read a
// full-viewport screenshot instead of #cell's rect for that leg, and clause
// 3 reads .page/#five computed opacity instead of the canvas alpha channel;
// both are called out inline and in the printed row.
//
// Driving technique: frame-paced scrollTo ticks, not mouse.wheel or a
// synthetic touch gesture -- the same choice check-landing-monotone.mjs
// already made and documented (mouse.wheel throws on mobile-emulated
// WebKit, so a wheel-based driver could never run both engines the same
// way; onScroll(dy)/progressAt() only ever read scrollY, never how it got
// there, so a tight loop of small scrollTo writes exercises the identical
// code path a real wheel or drag would). Desktop's watchScrollDesktop does
// pull scrollY toward the nearest scene's rest point once ticks stop (the
// carry spring, CARRY_K=40) -- letting it run is correct for the p=1
// arrival sample, and for every other checkpoint the capture happens one
// frame after the last tick, before the spring has moved the page far
// enough to matter (a few px at most; see the module comment on
// settleFrames).
import { resolveBaseURL, loadPlaywright, whenReady, PRESETS } from './landing-harness.mjs';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolve as resolvePath } from 'node:path';

const HERE = fileURLToPath(new URL('.', import.meta.url));
// MORPH_OUT_DIR points this at the run's own scratch folder (never a path
// under site/ -- this script only measures, per the brief). Defaults to a
// worktree-local folder so a bare invocation still works.
const OUT_ROOT = process.env.MORPH_OUT_DIR ? resolvePath(process.env.MORPH_OUT_DIR) : resolvePath(HERE, '..', 'morph-matrix');
const FILM_DIR = resolvePath(OUT_ROOT, 'film');

const SCENES = ['write', 'live', 'ships', 'deploy', 'share'];
// [from, to] pairs, forward. JOINS.length === 4 in site/landing.js; wash
// flags copied from JOINS[i].wash there (write-live, live-ships, ships-
// deploy carry a pigment wash; deploy-share is JOINS[3], a crossfade).
const LEGS = [[0, 1], [1, 2], [2, 3], [3, 4]];
const WASH_LEG = [true, true, true, false];
const GRID = { cols: 6, rows: 4 };
const CHECKPOINTS = [0, 0.05, 0.1, 0.2, 0.25, 0.3, 0.35, 0.4, 0.5, 0.6, 0.65, 0.7, 0.75, 0.8, 0.9, 0.95, 1];
const FILM_POINTS = [0, 0.2, 0.35, 0.5, 0.65, 0.8, 1];
const MID_POINTS = [0.4, 0.5, 0.6];
const COVER_POINTS = [0.3, 0.4, 0.5, 0.6, 0.7];

// Provisional -- stated per the brief, not tuned to make anything pass.
const THRESH = { fidelityDE: 12, chromaFrac: 0.7, darkFrac: 0.7, continuityFloor: 0.5 };

// The three-phase model (site/watercolor-morph.js; the owner's definition of
// a scene transition): A dissolves alone into a well-mixed wash by p = a,
// that wash becomes B's well-mixed wash by p = b, and B consolidates out of
// it, its own dissolve run backwards. These clauses read the wash canvas
// itself (never a screenshot: the canvas spans the print rectangle, which
// runs past the viewport), as optical density against the page colour,
// -ln(pixel / paper) per channel -- the pigment's own quantity by Beer-
// Lambert, and the one the engine sums, so "the same amount and colour
// ratio" is a comparison of mean density vectors, not of mean pixel colours
// (a sparse print and its uniform wash have the same pigment and different
// mean pixels). A and B are read off the prints the leg actually carries.
//   mass     per-channel relative error of the mean density vector, against
//            the print whose wash it is;
//   mixCV    coefficient of variation of the per-cell mean density over the
//            cells lying (80% or more) inside the print's footprint -- the
//            well-mixed test, which a print itself fails by a wide margin;
//   held     through phase 1 the wash holds A's pigment, and through phase 3
//            B's, each channel within this of the print's own (a dissolve
//            moves pigment, it does not make or lose it);
//   mono     phase 2's projection of the mean density onto B - A may fall
//            back by at most this between samples;
//   reverseDE  mean per-cell CIE76 deltaE between this leg's phase 3 and the
//            opposite direction's phase 1 at 1 - p (B's own dissolve);
//   noBlank  at every sampled p where a leg is mounted: total density at or
//            above massFloor x the lighter end's, and coverage of the union of
//            A's and B's footprints at or above covFloor x the least of the
//            four reference states (both prints, both washes), a pixel
//            counting as covered at half the lighter wash's own mean density.
//   restQuiet  once arrived and at rest, no dissolve is recorded again: steps
//            spent recording in a window from REST_WAIT_MS to REST_WAIT_MS +
//            REST_WINDOW_MS after arrival must be zero (warming neighbours is
//            one record each, done within the wait).
const MODEL = { mass: 0.15, held: 0.05, mixCV: 0.12, mono: 0.02, reverseDE: 3, massFloor: 0.75, covFloor: 0.8, pTol: 0.02 };
const REST_WAIT_MS = 2200, REST_WINDOW_MS = 1200;

const LAYOUTS = [
  { name: 'desktop', preset: PRESETS.desktop },
  { name: 'phone', preset: PRESETS.phone },
];
const ENGINE_NAMES = (process.env.MORPH_ENGINES || 'chromium,webkit').split(',').filter(Boolean);
// Narrowing for iteration only: MORPH_LAYOUTS=desktop, MORPH_LEGS=0,1 (leg indices).
const ONLY_LAYOUTS = process.env.MORPH_LAYOUTS ? process.env.MORPH_LAYOUTS.split(',') : null;
const ONLY_LEGS = process.env.MORPH_LEGS ? process.env.MORPH_LEGS.split(',').map(Number) : null;

const DEPLOY = 3; // PHASE.indexOf('deploy') in site/landing.js

// Members that must be covered by the pigment (or gone) partway through a
// wash -- unit6-membership-spec.md's own list, minus the Publish control
// (#box/#ed/#sh/#vd), which the brief exempts on legs 2 and 3 and which is
// never a "member" being dissolved on legs 0 and 1 in the first place (it is
// the shell, continuously on screen through those, not something the wash
// is supposed to cover -- checking it there would be a false positive, not
// a finding). #fan is checked by its own visibility, the literal mechanism
// index.html:300 (`#stage.morphing #fan { visibility: hidden; }`) applies;
// the other two are checked by whether they are the topmost hit at their
// own centre, since they are single rectangles the canvas is meant to sit
// above (z-index 6 vs 1 and 5).
//
// #fan is asymmetric on purpose (unit6-membership-spec.md's own words,
// "Decision already taken"): arriving at DEPLOY, the logos are not members
// of the composition being arrived at -- they run their own pop-out force
// animation as deploy's entrance performance, so a script that flagged
// #fan visible there would be reporting the design working as intended,
// not a bug. Leaving DEPLOY (toward SHARE, or back toward SHIPS) they are
// real members, in their live positions, and must dissolve like any other.
// activeMembers() below applies that gate; SHIPS's own volatile members
// (nb/sk/video) carry no such asymmetry in the spec and are always checked.
const MEMBERS_ALWAYS = [
  { sel: '#sib-nb', mode: 'topmost' },
  { sel: '#sib-sk', mode: 'topmost' },
  { sel: '#s3-video', mode: 'topmost' },
];
const FAN_MEMBER = { sel: '#fan', mode: 'visibility' };
const activeMembers = (from) => (from === DEPLOY ? [...MEMBERS_ALWAYS, FAN_MEMBER] : MEMBERS_ALWAYS);

const meanOf = (arr) => arr.reduce((a, b) => a + b, 0) / arr.length;

// ---- colour math: sRGB (0-255) -> CIE Lab (D65), CIE76 deltaE ----
function srgbToLinear(c) { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); }
function rgbToLab(r, g, b) {
  const rl = srgbToLinear(r), gl = srgbToLinear(g), bl = srgbToLinear(b);
  const x = rl * 0.4124564 + gl * 0.3575761 + bl * 0.1804375;
  const y = rl * 0.2126729 + gl * 0.7151522 + bl * 0.0721750;
  const z = rl * 0.0193339 + gl * 0.1191920 + bl * 0.9503041;
  const Xn = 0.95047, Yn = 1, Zn = 1.08883;
  const f = (t) => (t > 0.008856 ? Math.cbrt(t) : 7.787 * t + 16 / 116);
  const fx = f(x / Xn), fy = f(y / Yn), fz = f(z / Zn);
  return { L: 116 * fy - 16, a: 500 * (fx - fy), b: 200 * (fy - fz) };
}
const deltaE = (l1, l2) => Math.sqrt((l1.L - l2.L) ** 2 + (l1.a - l2.a) ** 2 + (l1.b - l2.b) ** 2);
const chroma = (lab) => Math.sqrt(lab.a ** 2 + lab.b ** 2);
const darkness = (lab) => 1 - lab.L / 100;
const cellLab = (c) => rgbToLab(c.r, c.g, c.b);

// ---- browser-side helpers (passed to page.evaluate, no Node closures) ----

function decodeAndReduce({ b64, cols, rows }) {
  return new Promise((res, rej) => {
    const img = new Image();
    img.onload = () => {
      const c = document.createElement('canvas');
      c.width = img.naturalWidth; c.height = img.naturalHeight;
      const g = c.getContext('2d', { willReadFrequently: true });
      g.drawImage(img, 0, 0);
      const W = c.width, H = c.height;
      const cellW = W / cols, cellH = H / rows;
      const cells = [];
      for (let ry = 0; ry < rows; ry++) {
        for (let rx = 0; rx < cols; rx++) {
          const x0 = Math.floor(rx * cellW), x1 = Math.floor((rx + 1) * cellW);
          const y0 = Math.floor(ry * cellH), y1 = Math.floor((ry + 1) * cellH);
          const w = Math.max(1, x1 - x0), h = Math.max(1, y1 - y0);
          const d = g.getImageData(x0, y0, w, h).data;
          let r = 0, gg = 0, b = 0, n = 0;
          for (let i = 0; i < d.length; i += 4) { r += d[i]; gg += d[i + 1]; b += d[i + 2]; n++; }
          cells.push({ col: rx, row: ry, r: r / n, g: gg / n, b: b / n });
        }
      }
      res({ w: W, h: H, cells });
    };
    img.onerror = () => rej(new Error('decodeAndReduce: image decode failed'));
    img.src = 'data:image/png;base64,' + b64;
  });
}

function namesGridBrowser({ rect, cols, rows }) {
  const out = [];
  for (let ry = 0; ry < rows; ry++) {
    for (let rx = 0; rx < cols; rx++) {
      const cx = rect.x + (rx + 0.5) * rect.width / cols;
      const cy = rect.y + (ry + 0.5) * rect.height / rows;
      const el = document.elementFromPoint(cx, cy);
      let name = null;
      if (el) {
        const withId = el.id ? el : el.closest('[id]');
        name = withId ? '#' + withId.id : el.tagName.toLowerCase();
      }
      out.push(name);
    }
  }
  return out;
}

function memberCheckBrowser(members) {
  return members.map(({ sel, mode }) => {
    const el = document.querySelector(sel);
    if (!el) return { sel, present: false, flagged: false };
    const cs = getComputedStyle(el);
    const visible = cs.visibility !== 'hidden' && cs.display !== 'none' && parseFloat(cs.opacity) > 0.05;
    if (!visible) return { sel, present: true, visible: false, flagged: false };
    if (mode === 'visibility') return { sel, present: true, visible: true, flagged: true };
    const r = el.getBoundingClientRect();
    const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    const top = document.elementFromPoint(cx, cy);
    const onTop = top === el || (el.contains && el.contains(top));
    return { sel, present: true, visible: true, onTop, topId: top ? top.id || top.tagName : null, flagged: !!onTop };
  });
}

function inkFractionBrowser() {
  const cv = document.getElementById('gl');
  const small = document.createElement('canvas');
  small.width = 160; small.height = 120;
  const g = small.getContext('2d', { willReadFrequently: true });
  g.drawImage(cv, 0, 0, small.width, small.height);
  const d = g.getImageData(0, 0, small.width, small.height).data;
  let inked = 0, n = 0;
  for (let i = 0; i < d.length; i += 4) { n++; if (d[i + 3] >= 32) inked++; }
  return inked / n;
}

// The mounted leg, the p it presented (in this run's own direction), and the
// wash canvas as optical density, downsampled to `side` columns; the two
// prints the same way when asked. Density is -ln(pixel / paper), clamped
// where absorb() clamps it (0.02 transmittance).
function washReadBrowser({ side, withPrints, from, to }) {
  const L = window.__landing;
  const cv = document.getElementById('gl');
  const cs = getComputedStyle(cv);
  const visible = cs.display !== 'none' && cs.visibility !== 'hidden' && parseFloat(cs.opacity) > 0.99;
  // A page without the morph surface (the commit before it) is read the same
  // way off what it does expose: the run's own scenes and their prints, and
  // the scroll progress for p, mounted whenever its canvas is displayed.
  const legacy = !L.morph;
  const leg = legacy ? (cs.display !== 'none' ? { from, to } : null) : L.morph.leg();
  const cur = legacy ? { p: (L.state().progress - from) / (to - from), a: L.prints[from], b: L.prints[to] } : L.morph.current();
  if (!leg || !cur) return { mounted: false, visible };
  const W = side, H = Math.max(1, Math.round(side * cv.height / cv.width));
  const hex = getComputedStyle(document.documentElement).getPropertyValue('--bg').trim();
  const n = parseInt(hex.slice(1), 16), bg = [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
  // Density per native pixel, then averaged into the W x H grid: averaging
  // colours first and taking the log after would understate any ink finer
  // than a grid cell, a print's most of all, and never a uniform wash's.
  const read = (src, composite) => {
    const c = document.createElement('canvas'); c.width = src.width; c.height = src.height;
    const g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(src, 0, 0);
    const d = g.getImageData(0, 0, c.width, c.height).data;
    const acc = new Float64Array(W * H * 3), inked = new Float64Array(W * H), cnt = new Float64Array(W * H);
    let alphaSum = 0;
    for (let y = 0; y < c.height; y++) {
      const gy = Math.min(H - 1, Math.floor(y * H / c.height));
      for (let x = 0; x < c.width; x++) {
        const i = (y * c.width + x) * 4, j = gy * W + Math.min(W - 1, Math.floor(x * W / c.width)), a = d[i + 3] / 255;
        alphaSum += a; cnt[j]++; if (a >= 0.5) inked[j]++;
        for (let ch = 0; ch < 3; ch++) {
          const v = d[i + ch] / 255;
          const r = composite ? (v * a + bg[ch] * (1 - a)) / bg[ch] : 1 + (Math.min(1, v / bg[ch]) - 1) * a;
          acc[j * 3 + ch] -= Math.log(Math.max(0.02, Math.min(1, r)));
        }
      }
    }
    const dens = Array.from(acc, (v, k) => +(v / Math.max(1, cnt[Math.floor(k / 3)])).toFixed(4));
    const foot = Array.from(inked, (v, j) => (v / Math.max(1, cnt[j]) >= 0.5 ? 1 : 0));
    return { dens, foot, alphaSum };
  };
  const shown = read(cv, true);
  const out = { mounted: true, visible, legFrom: leg.from, legTo: leg.to, p: cur.p, W, H, bg, dens: shown.dens, cleared: shown.alphaSum === 0 };
  if (withPrints) { out.a = read(cur.a, false); out.b = read(cur.b, false); }
  return out;
}

// ---- driving ----

// The same frame-paced scrollTo technique check-landing-monotone.mjs uses
// (see its own module comment for why mouse.wheel is not an option here):
// onScroll(dy) and progressAt() only ever read scrollY, so a tight loop of
// small writes exercises the identical code path a real scroll would,
// identically on both engines.
async function tickScrollTo(page, fromY, toY, { tick = 32, gapMs = 16 } = {}) {
  let y = fromY;
  const dir = Math.sign(toY - fromY);
  if (dir === 0) return fromY;
  while (dir > 0 ? y < toY - 0.5 : y > toY + 0.5) {
    y = dir > 0 ? Math.min(toY, y + tick) : Math.max(toY, y - tick);
    await page.evaluate((yy) => scrollTo(0, yy), Math.round(y));
    await page.waitForTimeout(gapMs);
  }
  return toY;
}

// Waits n real animation frames -- the render loop's own cadence, not a
// fixed timeout -- so a capture reflects a frame that has actually drawn
// since the last scrollTo tick, without waiting long enough for the desktop
// carry spring (once ticks stop) or the wash's own cure phase (once t
// passes T_WET) to have moved the page materially further. One frame is
// enough for renderMorphAt/advanceWash to draw the tick just applied;
// letting the loop win the race between "drawn" and "moved on" is the
// difference between an intermediate-p sample and an accidental rest one.
async function settleFrames(page, n = 1) {
  await page.evaluate((n) => new Promise((res) => {
    let left = n;
    const step = () => { if (--left <= 0) res(); else requestAnimationFrame(step); };
    requestAnimationFrame(step);
  }), n);
}

async function cellRect(page, wash) {
  if (!wash) return page.evaluate(() => ({ x: 0, y: 0, width: innerWidth, height: innerHeight }));
  return page.evaluate(() => {
    const r = document.getElementById('cell').getBoundingClientRect();
    return { x: r.left, y: r.top, width: r.width, height: r.height };
  });
}

async function captureGrid(page, rect) {
  const clip = { x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height) };
  const buf = await page.screenshot({ clip });
  const reduced = await page.evaluate(decodeAndReduce, { b64: buf.toString('base64'), cols: GRID.cols, rows: GRID.rows });
  return { ...reduced, buf, rect };
}

async function namesGrid(page, rect) {
  return page.evaluate(namesGridBrowser, { rect, cols: GRID.cols, rows: GRID.rows });
}

async function saveFilm(dir, legLabel, layout, engine, p, buf) {
  const name = `${layout}-${engine}-${legLabel}-p${String(p).replace('.', '_')}.png`;
  await writeFile(resolvePath(dir, name), buf);
}

// ---- per-leg-direction run ----

async function runLegDirection(page, { legIdx, from, to, wash, layout, engine, filmDir, bounds }) {
  const legLabel = `${from}-${to}`;
  const dir = Math.sign(to - from);
  const isDesktop = layout === 'desktop';

  // Fresh target reference, captured immediately before the leg it belongs
  // to, never reused from an earlier visit -- unit6-membership-spec.md's own
  // point is that a cached print goes stale (scene 2's continuously-zooming
  // Mandelbrot canvas most of all), so a reference held over from minutes
  // earlier would be exactly the kind of staleness this script exists to
  // catch, not a fair baseline to catch it against.
  await page.evaluate((to) => { window.__landing.still(to); scrollTo(0, window.__landing.restY(to)); }, to);
  await settleFrames(page, 3);
  const rectTo = await cellRect(page, wash);
  const refTo = await captureGrid(page, rectTo);
  const refToNames = await namesGrid(page, rectTo);

  await page.evaluate((from) => { window.__landing.still(from); scrollTo(0, window.__landing.restY(from)); }, from);
  await settleFrames(page, 3);
  const rectFrom = await cellRect(page, wash);
  const refFrom = await captureGrid(page, rectFrom);
  const refFromNames = await namesGrid(page, rectFrom);
  if (FILM_POINTS.includes(0)) await saveFilm(filmDir, legLabel, layout, engine, 0, refFrom.buf);

  // Desktop's watchScrollDesktop reads `travel` (the last wheel/pointer
  // direction) to disambiguate an in-between scroll position into the scene
  // ahead or behind (restSceneAt's dir<0?floor:ceil, DEAD-widened) -- a
  // plain scrollTo() write never sets `travel` itself (only a real wheel
  // event or a held pointer gesture do; watchScrollDesktop only reads it),
  // so without ever dispatching one the page falls back to dir===0's
  // round-to-nearest and never starts a wash before the reader crosses the
  // exact midpoint between the two scenes. Mobile has no such gap --
  // watchScrollNative sets travel from the raw scrollY delta itself, which
  // is why this is desktop-only. Found live in this leg: a scrollTo-only
  // version of this script (no wheel tick at all) read meanDE=0 at p=0.05,
  // i.e. no wash had started. One real wheel tick, in the leg's own
  // direction, seeds `travel`; the position it leaves scrollY at is then
  // overwritten back to restY(from) so the tick's own 1:1 push (if any)
  // does not shift where the checkpoint loop starts from.
  if (isDesktop) {
    await page.mouse.wheel(0, dir * 40);
    await page.evaluate((from) => scrollTo(0, window.__landing.restY(from)), from);
    await settleFrames(page, 2);
    // Desktop presents a transition as a function of scroll position now, and
    // its carry spring pulls a released page toward the nearest rest. A
    // pointer held on the scrollbar is the real input that suspends that pull
    // (watchScrollDesktop's held branch: direct manipulation, no spring), so
    // each checkpoint stays exactly where the ticks put it while it is read.
    await page.evaluate(() => dispatchEvent(new PointerEvent('pointerdown', { clientX: 1e5, clientY: 10, pointerType: 'mouse', button: 0 })));
  }

  const yFrom = await page.evaluate(() => scrollY);
  const yTo = await page.evaluate((to) => window.__landing.restY(to), to);

  const samples = { 0: refFrom };
  let prints = null;
  // Progress is flat while the reading line stands in a text and linear in
  // scroll across the gap between two (the phone's ramps too), so a line
  // through yFrom to yTo lands most checkpoints on a plateau. The slope is
  // measured once, just into the ramp, in small forward ticks, and places
  // every checkpoint; a correction or two absorbs its rounding.
  const pAt = async () => ((await page.evaluate(() => window.__landing.state().progress)) - from) / (to - from);
  const sgn = Math.sign(yTo - yFrom);
  let curY = yFrom, first = null, second = null;
  while (!second && (yTo - curY) * sgn > 8) {
    curY += sgn * 6;
    await page.evaluate((y) => scrollTo(0, y), curY);
    await settleFrames(page, 1);
    const q = await pAt();
    if (!first) { if (q > 0.004) first = [curY, q]; } else if (q > 0.03) second = [curY, q];
  }
  const slope = second ? (second[1] - first[1]) / (second[0] - first[0]) : 1 / (yTo - yFrom);
  for (const p of CHECKPOINTS) {
    if (p === 0) continue;
    if (p === 1) curY = await tickScrollTo(page, curY, yTo);
    else for (let i = 0; i < 4; i++) {
      const err = p - await pAt();
      if (Math.abs(err) < 0.003) break;
      curY = await tickScrollTo(page, curY, Math.round(curY + err / slope), i ? { tick: 60, gapMs: 8 } : undefined);
    }
    await settleFrames(page, 1);
    const rect = wash ? await cellRect(page, wash) : rectTo;
    const shot = await captureGrid(page, rect);
    const st = await page.evaluate(() => window.__landing.state());
    shot.achievedP = (st.progress - from) / (to - from);
    shot.steps = st.steps; shot.washT = st.washT;
    if (p >= COVER_POINTS[0] && p <= COVER_POINTS.at(-1)) {
      shot.members = await page.evaluate(memberCheckBrowser, activeMembers(from));
      if (wash) shot.inkFrac = await page.evaluate(inkFractionBrowser);
      else shot.xfOpacity = await page.evaluate(() => ({
        page: +getComputedStyle(document.querySelector('.page')).opacity,
        five: +getComputedStyle(document.getElementById('five')).opacity,
      }));
    }
    if (wash) {
      shot.wash = await page.evaluate(washReadBrowser, { side: 96, withPrints: !prints, from, to });
      // Only this run's own leg: at a scene's rest the phone already has the
      // next leg mounted at p = 0, which is not this transition.
      const own = shot.wash.mounted && Math.min(shot.wash.legFrom, shot.wash.legTo) === Math.min(from, to) && Math.max(shot.wash.legFrom, shot.wash.legTo) === Math.max(from, to);
      if (!own) shot.wash = { mounted: false, visible: shot.wash.visible };
      if (shot.wash.mounted) {
        // This run's own direction: a mobile leg is always mounted lower-to-
        // upper, so a run downward-to-upward reads its p as is and the
        // opposite run reads 1 - p, with its a and b swapped.
        const same = shot.wash.legFrom === from;
        shot.wash.pDir = same ? shot.wash.p : 1 - shot.wash.p;
        if (shot.wash.a) prints = same ? { A: shot.wash.a, B: shot.wash.b } : { A: shot.wash.b, B: shot.wash.a };
        delete shot.wash.a; delete shot.wash.b;
      }
    }
    if (p === 1) {
      shot.arrived = await page.evaluate(() => window.__landing.state().shown);
      if (isDesktop) await page.evaluate(() => dispatchEvent(new PointerEvent('pointerup', { clientX: 1e5, clientY: 10, pointerType: 'mouse', button: 0 })));
      if (wash) {
        await page.waitForTimeout(REST_WAIT_MS);
        const r0 = await page.evaluate(() => window.__landing.state().recordSteps);
        await page.waitForTimeout(REST_WINDOW_MS);
        shot.restRecord = (await page.evaluate(() => window.__landing.state().recordSteps)) - r0;
      }
    }
    if (FILM_POINTS.includes(p)) await saveFilm(filmDir, legLabel, layout, engine, p, shot.buf);
    samples[p] = shot;
  }

  return judge({ legIdx, from, to, dir, wash, layout, engine, refFrom, refTo, refFromNames, refToNames, samples, prints, bounds });
}

function worstCells(refCells, sampleCells, names) {
  const de = refCells.map((c, i) => deltaE(cellLab(c), cellLab(sampleCells[i])));
  const meanDE = meanOf(de);
  const order = [...de.keys()].sort((a, b) => de[b] - de[a]).slice(0, 3);
  const worst = order.map((i) => ({ col: refCells[i].col, row: refCells[i].row, dE: +de[i].toFixed(1), element: names[i] }));
  return { meanDE: +meanDE.toFixed(2), worst };
}

function judge(ctx) {
  const { legIdx, from, to, wash, layout, engine, refFrom, refTo, refFromNames, refToNames, samples } = ctx;
  const clauses = {};

  // Clause 1: source fidelity at p~=0.05.
  {
    const { meanDE, worst } = worstCells(refFrom.cells, samples[0.05].cells, refFromNames);
    clauses.source = { p: 0.05, meanDE, pass: meanDE < THRESH.fidelityDE, worst };
  }

  // Clause 2: mid-wash pigment body, p=0.4/0.5/0.6, as a fraction of the
  // outgoing scene's own mean chroma/darkness. "Covered area" is
  // approximated as the whole clip region -- a wash is defined to cover the
  // full composition, and clause 3 independently checks how much of it
  // actually carries ink, so this does not also try to segment covered vs
  // bare pixels.
  {
    const refChroma = meanOf(refFrom.cells.map((c) => chroma(cellLab(c))));
    const refDark = meanOf(refFrom.cells.map((c) => darkness(cellLab(c))));
    const per = MID_POINTS.map((p) => {
      const s = samples[p];
      const ch = meanOf(s.cells.map((c) => chroma(cellLab(c))));
      const dk = meanOf(s.cells.map((c) => darkness(cellLab(c))));
      return { p, chromaFrac: refChroma > 0.01 ? ch / refChroma : 1, darkFrac: refDark > 0.01 ? dk / refDark : 1 };
    });
    const worst = per.reduce((a, b) => Math.min(a.chromaFrac, a.darkFrac) <= Math.min(b.chromaFrac, b.darkFrac) ? a : b);
    clauses.mid = { per, worst, refChroma: +refChroma.toFixed(1), refDark: +refDark.toFixed(3), pass: per.every((x) => x.chromaFrac >= THRESH.chromaFrac && x.darkFrac >= THRESH.darkFrac) };
  }

  // Clause 3: continuity. Wash legs read the canvas's own alpha coverage
  // (the technique scratchpad/forensics/probes/probe-wash2.mjs already
  // validated for this exact bug report); the crossfade leg has no canvas,
  // so it reads .page/#five's own computed opacity instead and is judged by
  // "both sides carry some ink whenever the crossfade is running", not by
  // the 0.5 pigment-coverage floor, which measures a different mechanism
  // and would not mean the same thing here.
  if (wash) {
    const points = COVER_POINTS.map((p) => ({ p, frac: +samples[p].inkFrac.toFixed(3) }));
    const worst = points.reduce((a, b) => (a.frac <= b.frac ? a : b));
    clauses.continuity = { mechanism: 'wash-alpha', points, worst, pass: points.every((x) => x.frac >= THRESH.continuityFloor) };
  } else {
    const points = COVER_POINTS.map((p) => ({ p, ...samples[p].xfOpacity }));
    const worst = points.reduce((a, b) => Math.min(a.page, a.five) <= Math.min(b.page, b.five) ? a : b);
    clauses.continuity = { mechanism: 'crossfade-opacity', points, worst, pass: points.every((x) => x.page > 0.02 && x.five > 0.02) };
  }

  // Clause 4: nothing left behind, p=0.3..0.7, except the Publish control on
  // legs 2 and 3 (excluded from MEMBERS entirely -- see the module comment
  // there for why checking it on legs 0/1 would be a false positive).
  {
    const flags = [];
    for (const p of COVER_POINTS) for (const m of samples[p].members) if (m.flagged) flags.push({ p, sel: m.sel, topId: m.topId ?? null });
    clauses.leftBehind = { points: COVER_POINTS, flags, pass: flags.length === 0 };
  }

  // Clause 5: target fidelity at p~=0.95.
  {
    const { meanDE, worst } = worstCells(refTo.cells, samples[0.95].cells, refToNames);
    clauses.target = { p: 0.95, meanDE, pass: meanDE < THRESH.fidelityDE, worst };
  }

  if (wash) Object.assign(clauses, judgeModel(ctx));
  const _wash = {};
  for (const p of CHECKPOINTS) if (ctx.samples[p]?.wash?.mounted) _wash[p] = ctx.samples[p].wash;
  return { legIdx, from: SCENES[from], to: SCENES[to], fromIdx: from, toIdx: to, wash, layout, engine, clauses, bounds: ctx.bounds, _wash };
}

// ---- the three-phase clauses ----

const meanVec = (dens) => { const m = [0, 0, 0]; for (let i = 0; i < dens.length; i += 3) { m[0] += dens[i]; m[1] += dens[i + 1]; m[2] += dens[i + 2]; } return m.map((v) => v / (dens.length / 3)); };
const sum3 = (m) => m[0] + m[1] + m[2];
const maxAt = (dens, j) => Math.max(dens[j * 3], dens[j * 3 + 1], dens[j * 3 + 2]);
const r3 = (m) => m.map((v) => +v.toFixed(4));
// Per-cell mean density vectors over the GRID, for the well-mixed test and
// the reverse comparison; `share` is the fraction of each cell in `foot`.
function cells(dens, W, H, foot) {
  const out = [];
  for (let ry = 0; ry < GRID.rows; ry++) for (let rx = 0; rx < GRID.cols; rx++) {
    const x0 = Math.floor(rx * W / GRID.cols), x1 = Math.floor((rx + 1) * W / GRID.cols);
    const y0 = Math.floor(ry * H / GRID.rows), y1 = Math.floor((ry + 1) * H / GRID.rows);
    const m = [0, 0, 0]; let n = 0, inFoot = 0;
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) { const j = y * W + x; m[0] += dens[j * 3]; m[1] += dens[j * 3 + 1]; m[2] += dens[j * 3 + 2]; n++; if (foot?.[j]) inFoot++; }
    out.push({ m: m.map((v) => v / Math.max(1, n)), share: inFoot / Math.max(1, n) });
  }
  return out;
}
function footCV(dens, W, H, foot) {
  const inside = cells(dens, W, H, foot).filter((c) => c.share >= 0.8).map((c) => sum3(c.m));
  if (inside.length < 2) return null;
  const mu = meanOf(inside), sd = Math.sqrt(meanOf(inside.map((v) => (v - mu) ** 2)));
  return mu > 1e-4 ? sd / mu : 0;
}
function coverage(dens, U, tau) { let n = 0, hit = 0; for (let j = 0; j < U.length; j++) if (U[j]) { n++; if (maxAt(dens, j) >= tau) hit++; } return n ? hit / n : 1; }
function footMean(dens, foot) { let n = 0, s = 0; for (let j = 0; j < foot.length; j++) if (foot[j]) { n++; s += maxAt(dens, j); } return n ? s / n : 0; }
const densLab = (m, bg) => rgbToLab(...m.map((d, ch) => 255 * bg[ch] * Math.exp(-d)));

function judgeModel({ samples, prints, to, refTo, refToNames, bounds }) {
  const [a, b] = bounds;
  const out = {};
  const fail = (why) => ({ pass: false, why });
  if (!prints) { for (const k of ['mixA', 'held', 'phase2', 'mixB', 'noBlank']) out[k] = fail('no leg ever mounted'); }
  else {
    const at = (p) => samples[p]?.wash?.mounted ? samples[p].wash : null;
    const MA = meanVec(prints.A.dens), MB = meanVec(prints.B.dens);
    const mixClause = (p, P, M) => {
      const w = at(p);
      if (!w) return fail(`no mounted leg at p=${p}`);
      const Mw = meanVec(w.dens);
      const err = Mw.map((v, ch) => Math.abs(v - M[ch]) / Math.max(M[ch], 0.02));
      const cv = footCV(w.dens, w.W, w.H, P.foot), printCV = footCV(P.dens, w.W, w.H, P.foot);
      const pOk = Math.abs(w.pDir - p) <= MODEL.pTol;
      return { p: +w.pDir.toFixed(3), mean: r3(Mw), print: r3(M), err: r3(err), cv: cv == null ? null : +cv.toFixed(3), printCV: printCV == null ? null : +printCV.toFixed(3),
        pass: pOk && err.every((e) => e <= MODEL.mass) && (cv == null || cv <= MODEL.mixCV) };
    };
    out.mixA = mixClause(a, prints.A, MA);
    out.mixB = mixClause(b, prints.B, MB);
    {
      const bad = [];
      for (const p of CHECKPOINTS) {
        const w = at(p); if (!w || (p > a && p < b) || p <= 0 || p >= 1) continue;
        const M = p <= a ? MA : MB, err = meanVec(w.dens).map((v, ch) => Math.abs(v - M[ch]) / Math.max(M[ch], 0.02));
        if (err.some((e) => e > MODEL.held)) bad.push({ p, err: r3(err) });
      }
      out.held = { bad, pass: bad.length === 0 };
    }
    {
      const d = MB.map((v, ch) => v - MA[ch]), dd = d.reduce((s, v) => s + v * v, 0);
      const pts = CHECKPOINTS.filter((p) => p >= a && p <= b).map((p) => {
        const w = at(p); if (!w) return { p, missing: true };
        const M = meanVec(w.dens);
        return { p: +w.pDir.toFixed(3), proj: dd > 1e-8 ? +(M.reduce((s, v, ch) => s + (v - MA[ch]) * d[ch], 0) / dd).toFixed(3) : null };
      });
      const mono = pts.every((x, i) => !x.missing && (i === 0 || x.proj == null || x.proj >= pts[i - 1].proj - MODEL.mono));
      out.phase2 = { pts, pass: mono };
    }
    {
      const wa = at(a), wb = at(b);
      if (!wa || !wb) out.noBlank = fail('no well-mixed sample to measure the floor from');
      else {
        const tau = 0.5 * Math.min(footMean(wa.dens, prints.A.foot), footMean(wb.dens, prints.B.foot));
        const U = prints.A.foot.map((v, j) => v || prints.B.foot[j] ? 1 : 0);
        const refs = { printA: coverage(prints.A.dens, U, tau), printB: coverage(prints.B.dens, U, tau), mixA: coverage(wa.dens, U, tau), mixB: coverage(wb.dens, U, tau) };
        const covFloor = MODEL.covFloor * Math.min(...Object.values(refs));
        const massFloor = MODEL.massFloor * Math.min(sum3(MA), sum3(MB));
        const bad = [];
        const pts = [];
        for (const p of CHECKPOINTS) {
          if (p <= 0 || p >= 1) continue;
          const w = samples[p]?.wash;
          const mid = p >= 0.2 && p <= 0.8;
          if (!w?.mounted) { if (mid) bad.push({ p, why: 'no leg mounted' }); continue; }
          const mass = sum3(meanVec(w.dens)), cov = coverage(w.dens, U, tau);
          pts.push({ p: +w.pDir.toFixed(2), mass: +mass.toFixed(3), cov: +cov.toFixed(3) });
          if (mid && (!w.visible || w.cleared)) bad.push({ p, why: w.cleared ? 'canvas cleared' : 'canvas hidden' });
          if (mass < massFloor) bad.push({ p, why: `mass ${mass.toFixed(3)} < ${massFloor.toFixed(3)}` });
          if (cov < covFloor) bad.push({ p, why: `coverage ${cov.toFixed(3)} < ${covFloor.toFixed(3)}` });
        }
        out.noBlank = { tau: +tau.toFixed(4), refs: Object.fromEntries(Object.entries(refs).map(([k, v]) => [k, +v.toFixed(3)])), covFloor: +covFloor.toFixed(3), massFloor: +massFloor.toFixed(3), pts, bad, pass: bad.length === 0 };
      }
    }
  }
  out.restQuiet = { steps: samples[1].restRecord, pass: samples[1].restRecord === 0 };
  {
    const s1 = samples[1];
    const { meanDE, worst } = worstCells(refTo.cells, s1.cells, refToNames);
    out.arrive = { shown: s1.arrived, meanDE, worst, pass: s1.arrived === to && meanDE < THRESH.fidelityDE };
  }
  return out;
}

// Phase 3 of one direction against phase 1 of the other at 1 - p: B's own
// dissolve, run backwards. Pairs whose presented p differ by more than pTol
// are reported, not compared.
function judgeReverse(fwd, back) {
  const [, b] = fwd.bounds;
  const pairs = [];
  for (const p of CHECKPOINTS.filter((q) => q > b && q < 1)) {
    const x = fwd._wash[p], y = back._wash[+(1 - p).toFixed(2)];
    if (!x || !y) { pairs.push({ p, missing: true }); continue; }
    if (Math.abs(x.pDir - (1 - y.pDir)) > MODEL.pTol) { pairs.push({ p, skipped: `p ${x.pDir.toFixed(3)} vs ${(1 - y.pDir).toFixed(3)}` }); continue; }
    const cx = cells(x.dens, x.W, x.H), cy = cells(y.dens, y.W, y.H);
    const de = cx.map((c, i) => deltaE(densLab(c.m, x.bg), densLab(cy[i].m, y.bg)));
    pairs.push({ p, meanDE: +meanOf(de).toFixed(2), maxDE: +Math.max(...de).toFixed(1) });
  }
  const compared = pairs.filter((q) => q.meanDE != null);
  return { pairs, pass: compared.length > 0 && compared.every((q) => q.meanDE <= MODEL.reverseDE) };
}

function fmtClause(c) { return c.pass ? 'PASS' : 'FAIL'; }

function printRow(r) {
  const label = `${r.fromIdx}->${r.toIdx} (${r.from}->${r.to})`;
  console.log(`${r.layout}/${r.engine} ${label}`);
  console.log(`  1 source  p=0.05  meanDE=${r.clauses.source.meanDE}  ${fmtClause(r.clauses.source)}${r.clauses.source.pass ? '' : '  worst=' + JSON.stringify(r.clauses.source.worst)}`);
  console.log(`  2 mid     p=.4/.5/.6  worst chromaFrac=${r.clauses.mid.worst.chromaFrac.toFixed(2)} darkFrac=${r.clauses.mid.worst.darkFrac.toFixed(2)} (at p=${r.clauses.mid.worst.p})  ${fmtClause(r.clauses.mid)}`);
  console.log(`  3 continuity [${r.clauses.continuity.mechanism}]  worst=${JSON.stringify(r.clauses.continuity.worst)}  ${fmtClause(r.clauses.continuity)}`);
  console.log(`  4 leftBehind  ${fmtClause(r.clauses.leftBehind)}${r.clauses.leftBehind.flags.length ? '  flags=' + JSON.stringify(r.clauses.leftBehind.flags.slice(0, 4)) : ''}`);
  console.log(`  5 target  p=0.95  meanDE=${r.clauses.target.meanDE}  ${fmtClause(r.clauses.target)}${r.clauses.target.pass ? '' : '  worst=' + JSON.stringify(r.clauses.target.worst)}`);
  if (!r.wash) return;
  const c = r.clauses, brief = (x) => x.why ? x.why : `p=${x.p} err=${JSON.stringify(x.err)} cv=${x.cv} (print ${x.printCV})`;
  console.log(`  6 mixA    ${brief(c.mixA)}  ${fmtClause(c.mixA)}`);
  console.log(`  7 phase2  ${c.phase2.why || JSON.stringify(c.phase2.pts.map((x) => x.proj))}  ${fmtClause(c.phase2)}`);
  console.log(`  7b held   ${c.held.why || JSON.stringify(c.held.bad.slice(0, 3))}  ${fmtClause(c.held)}`);
  console.log(`  8 mixB    ${brief(c.mixB)}  ${fmtClause(c.mixB)}`);
  console.log(`  9 reverse ${c.reverse ? JSON.stringify(c.reverse.pairs) : 'not run'}  ${c.reverse ? fmtClause(c.reverse) : ''}`);
  console.log(`  10 arrive shown=${c.arrive.shown} meanDE=${c.arrive.meanDE}  ${fmtClause(c.arrive)}`);
  console.log(`  10b restQuiet recordSteps=${c.restQuiet.steps}  ${fmtClause(c.restQuiet)}`);
  console.log(`  11 noBlank ${c.noBlank.why || `tau=${c.noBlank.tau} floors cov=${c.noBlank.covFloor} mass=${c.noBlank.massFloor} refs=${JSON.stringify(c.noBlank.refs)} bad=${JSON.stringify(c.noBlank.bad.slice(0, 3))}`}  ${fmtClause(c.noBlank)}`);
}

async function main() {
  const t0 = Date.now();
  await mkdir(FILM_DIR, { recursive: true });
  const { baseURL, close } = await resolveBaseURL(process.argv[2]);
  const engines = await loadPlaywright();
  const results = [];
  try {
    for (const engineName of ENGINE_NAMES) {
      const engine = engines[engineName];
      if (!engine) { console.log(`skip ${engineName}: not a known Playwright browser type`); continue; }
      const browser = await engine.launch();
      try {
        for (const layout of LAYOUTS.filter((l) => !ONLY_LAYOUTS || ONLY_LAYOUTS.includes(l.name))) {
          const page = await browser.newPage(layout.preset);
          const pageErrors = [];
          page.on('pageerror', (e) => pageErrors.push(e.message));
          await page.goto(baseURL);
          await whenReady(page);
          await page.waitForTimeout(500);
          const bounds = await page.evaluate(() => window.__landing.morph?.bounds || [0.35, 0.65]);
          for (let legIdx = 0; legIdx < LEGS.length; legIdx++) {
            if (ONLY_LEGS && !ONLY_LEGS.includes(legIdx)) continue;
            const [a, b] = LEGS[legIdx];
            const wash = WASH_LEG[legIdx];
            const pair = [];
            for (const [from, to] of [[a, b], [b, a]]) {
              const r = await runLegDirection(page, { legIdx, from, to, wash, layout: layout.name, engine: engineName, filmDir: FILM_DIR, bounds });
              results.push(r); pair.push(r);
            }
            if (wash) { pair[0].clauses.reverse = judgeReverse(pair[0], pair[1]); pair[1].clauses.reverse = judgeReverse(pair[1], pair[0]); }
            for (const r of pair) { printRow(r); delete r._wash; }
          }
          if (pageErrors.length) console.log(`${engineName}/${layout.name}: page errors: ${pageErrors.join('; ')}`);
          await page.close();
        }
      } finally { await browser.close(); }
    }
  } finally { await close(); }

  const elapsedMs = Date.now() - t0;
  await writeFile(resolvePath(OUT_ROOT, 'results.json'), JSON.stringify({ elapsedMs, thresholds: THRESH, grid: GRID, results }, null, 1));

  const failCount = results.reduce((n, r) => n + Object.values(r.clauses).filter((c) => !c.pass).length, 0);
  console.log(`\n${results.length} direction-runs, ${failCount} clause failures against provisional thresholds, ${(elapsedMs / 1000).toFixed(1)}s total. Full dump: ${resolvePath(OUT_ROOT, 'results.json')}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
