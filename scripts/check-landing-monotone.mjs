#!/usr/bin/env node
// I-monotone (unit7-scroll-and-flicker-spec.md part A/C), committed from the
// forensics probe (scratchpad/forensics/probes/probe-mobile.mjs) rather than
// kept as a one-off script. A real continuous scroll down the whole phone
// page, then back up, sampled every animation frame from inside the page --
// stage.dataset.scene, the #gl canvas's rendered opacity, and the raw
// --wash-cover custom property. Small frame-paced scrollTo steps drive the
// scroll rather than CDP touch-gesture synthesis or mouse.wheel: CDP
// sessions only exist on Chromium, and mouse.wheel throws on mobile-
// emulated WebKit, so neither runs both engines with the same input. The
// page's own logic never looks at input modality -- onScroll(dy) and
// progressAt() only read scrollY -- so this exercises the identical code
// path a wheel or touch drag would.
import { loadPlaywright, resolveBaseURL, trackErrors, whenReady, PRESETS } from './landing-harness.mjs';

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const engines = await loadPlaywright();
const assert = (condition, message) => { if (!condition) throw new Error(message); };

const INSTALL_SAMPLER = () => {
  window.__S = [];
  window.__samplerOn = true;
  const stage = document.getElementById('stage');
  const gl = document.getElementById('gl');
  const frame = () => {
    if (!window.__samplerOn) return;
    let progress = NaN;
    try { progress = window.__landing.state().progress; } catch (e) { /* boot not finished */ }
    window.__S.push({
      y: Math.round(scrollY),
      progress,
      scene: stage.dataset.scene,
      cover: parseFloat(stage.style.getPropertyValue('--wash-cover')) || 0,
      glOp: Number(getComputedStyle(gl).opacity),
    });
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
};

// mouse.wheel is unsupported on mobile-emulated WebKit (Playwright throws),
// so both engines move the same way here: scrollTo in small frame-paced
// steps. onScroll(dy) and progressAt() read scrollY alone -- neither cares
// how it got there -- so this exercises the identical code path a wheel or
// touch-driven scroll would.
async function scrollTicks(page, { from, total, tick = 20, gapMs = 16 }) {
  let done = 0;
  const step = Math.sign(total) * Math.abs(tick);
  while (Math.abs(done) < Math.abs(total)) {
    done += step;
    await page.evaluate((y) => scrollTo(0, y), from + done);
    await page.waitForTimeout(gapMs);
  }
}

// Split the whole trace into per-leg, per-direction spans keyed by the
// integer scene band `progress` sits in (floor while descending toward it
// from above reads the same band a plain floor does, since progress is only
// ever read forward here). A leg is the span between two adjacent bands, so
// its samples are exactly the run of frames sharing one `Math.floor(progress)`
// -- the natural partition the fix's own p = clamp01(progressAt() - from) is
// defined over, independent of dataset.scene, which is the very thing under
// test.
function legs(samples) {
  const out = [];
  let cur = null;
  for (const s of samples) {
    if (!Number.isFinite(s.progress)) continue;
    const band = Math.floor(Math.min(s.progress, 4.999));
    if (!cur || cur.band !== band) { cur = { band, samples: [] }; out.push(cur); }
    cur.samples.push(s);
  }
  return out;
}

// Sign of the first difference, ignoring equal-value frames. The first
// direction found costs nothing (prev is still null); the intended shape --
// one rise, one fall -- costs exactly one more flip. A second flip, in
// either direction, is the bug: a rise-fall-rise chatters, and cover must
// never fall then rise at all (I-monotone clause 3).
function reversals(values, { forbidFallThenRise } = {}) {
  let dir = null, n = 0;
  const flips = [];
  for (let i = 1; i < values.length; i++) {
    const d = values[i] - values[i - 1];
    if (Math.abs(d) < 1e-6) continue;
    const sign = d > 0 ? 1 : -1;
    if (dir === null) { dir = sign; continue; }
    if (sign !== dir) {
      const isFallThenRise = dir < 0 && sign > 0;
      if (forbidFallThenRise && isFallThenRise) n++;
      else if (!forbidFallThenRise) n++;
      flips.push({ i, from: dir, to: sign });
      dir = sign;
    }
  }
  return { n, flips };
}

function sceneChanges(samples) {
  let n = 0; const at = [];
  for (let i = 1; i < samples.length; i++) {
    if (samples[i].scene !== samples[i - 1].scene) { n++; at.push(i); }
  }
  return { n, at };
}

async function runDirection(page, dir, height) {
  await page.evaluate(() => { window.__S = []; });
  const start = dir === 'down' ? 0 : height;
  await page.evaluate((y) => scrollTo(0, y), start);
  await page.waitForTimeout(200);
  await scrollTicks(page, { from: start, total: dir === 'down' ? height : -height });
  await page.waitForTimeout(300);
  return page.evaluate(() => window.__S);
}

// Collects every leg's numbers into `report` and every violation into
// `failures`, rather than throwing on the first one, so a run against a
// broken build reports the whole shape of the breakage in one pass instead
// of one leg at a time across repeated runs.
function checkLegs(label, samples, report, failures) {
  for (const leg of legs(samples)) {
    if (leg.samples.length < 3) continue; // too short to say anything about shape
    const tag = `${label} leg ${leg.band}->${leg.band + 1}`;
    const sc = sceneChanges(leg.samples);
    const coverRev = reversals(leg.samples.map(s => s.cover), { forbidFallThenRise: true });
    const opRev = reversals(leg.samples.map(s => s.glOp), { forbidFallThenRise: true });
    report.push({ tag, frames: leg.samples.length, sceneChanges: sc.n, coverFlips: coverRev.n, opFlips: opRev.n });
    if (sc.n > 1) failures.push(`${tag}: dataset.scene changed ${sc.n} times (want <= 1): ${JSON.stringify(sc.at.map(i => leg.samples[i].scene))}`);
    for (const i of sc.at) {
      const cover = leg.samples[i].cover;
      if (!(cover >= 0.98)) failures.push(`${tag}: scene switched at cover ${cover} (want >= 0.98)`);
    }
    if (coverRev.n !== 0) failures.push(`${tag}: --wash-cover is not unimodal, ${coverRev.n} bad flip(s): ${JSON.stringify(coverRev.flips)}`);
    if (opRev.n !== 0) failures.push(`${tag}: canvas opacity is not unimodal, ${opRev.n} bad flip(s): ${JSON.stringify(opRev.flips)}`);
  }
}

// Every engine runs to completion and prints its full per-leg table even
// when some legs fail, so one run against a broken build is enough evidence
// for both engines at once instead of a fail-fast run per engine.
const allFailures = [];
try {
for (const name of ['chromium', 'webkit']) {
  const browser = await engines[name].launch();
  try {
    const page = await browser.newPage(PRESETS.phone);
    const errors = trackErrors(page);
    await page.goto(baseURL);
    await whenReady(page, { timeout: 60000 });
    // The bug this probes is a flicker while reading an already-loaded
    // page, not a cold-load race (that is check-landing-cold-bottom.mjs's
    // job): the ambient warmer that captures a scene's print is throttled
    // to roughly one a second and only runs once the page is briefly still,
    // so a scroll that never pauses can outrun it and leave a leg "cut" --
    // no print, no cover -- for the length of that race, a real but
    // different failure mode from the one under test here. A nudge past
    // the opening first, same as check-landing-mobile-handoff.mjs: resting
    // at y=0 (scene 0 only) does not prioritize the far scenes' prints the
    // way landing on scene 1 does, and this wait alone timed out without it.
    await page.evaluate(() => scrollTo(0, window.__landing.restY(1) + 24));
    await page.waitForFunction(() => [0, 1, 2, 3].every((i) => window.__landing.prints[i]), null, { timeout: 30000 });
    await page.evaluate(() => scrollTo(0, 0));
    const height = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
    await page.evaluate(INSTALL_SAMPLER);

    const down = await runDirection(page, 'down', height);
    const up = await runDirection(page, 'up', height);
    await page.evaluate(() => { window.__samplerOn = false; });

    const report = [], failures = [];
    checkLegs(`${name}/down`, down, report, failures);
    checkLegs(`${name}/up`, up, report, failures);
    if (errors.length) failures.push(`${name}: page errors: ${errors.join('; ')}`);

    console.log(`${name}: ${report.length} legs, ${down.length + up.length} frames sampled`);
    for (const r of report) console.log(`  ${r.tag}: frames=${r.frames} sceneChanges=${r.sceneChanges} coverFlips=${r.coverFlips} opFlips=${r.opFlips}`);
    allFailures.push(...failures);
    await page.close();
  } finally { await browser.close(); }
}
} finally { await close(); }
assert(allFailures.length === 0, `${allFailures.length} violation(s) across engines:\n${allFailures.join('\n')}`);
