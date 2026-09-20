#!/usr/bin/env node
// The rules the page must keep (design doc R1-R8), each with a named fault
// shown red then green before it was trusted -- see each function below for
// the one-line record:
//
// I-commit  (R3) fault: watchScrollIntent's own `setTarget(Math.max(0,
//           scene))` call site deleted -- no scroll ever picks a target
//           scene again, so every boundary times out instead of committing.
//           RED/GREEN recorded below each function.
// I-rect    (R2) fault: the scene-3 card-drop rect-expansion call site
//           (`printExpanded = true; setPrintRect(currentPrintRect());` in
//           endDrag()) deleted -- a dragged-outside card leaves the print
//           rectangle. (The scene-1 plate's own equivalent call site is a
//           second, independent one; deleting only the plate's still passed,
//           because the card drop's own currentPrintRect() call unions the
//           plates too -- recorded as a finding, not silently dropped.)
// I-scene3  (R6) fault: CARDS.video's `docked: true` flipped to false -- the
//           video is no longer inside the preview on cold load.
// I-reduced (R7) fault: the `if (reduce) return scrollTo(0, y);` call site in
//           settleAtRest deleted -- reduced motion falls through to the
//           per-frame spring, writing scrollTo on every frame of the settle.
//           (Needed a deliberately short wheel delta to expose: an exact one
//           already lands settleAtRest at x=0, which returns before ever
//           choosing between the two paths.)
// I-fuzz    (R7) no fault turned this one red. Tried, in order: deleting
//           fillPrints()'s call site in warm() with one print nulled mid-fuzz
//           (check-landing-readiness.mjs's own fault); the same with every
//           print nulled; deleting maybeJoin()'s runJoin() call outright;
//           and forcing every fresh capture to hang via the existing
//           faults.captureHang. Every one still reached rest, because the
//           boot sequence (ready()) has already captured every scene's print
//           before any fuzzing starts, so nothing here ever needs a fresh
//           one -- a real, reported finding about the page's own robustness,
//           not a weakened check. The assertions (rest, rendered, zero
//           errors, the Chromium time bound) are real and passing; this one
//           invariant ships without ablation evidence.
// I-default (checks, not the page) fault: a scratch script with
//           `page.goto(base + '?carry=intent')` -- the static scan catches it.
//
// Rules: every scene reach uses a real armed gesture plus window.__landing's
// own restY()/state(), never a fixed sleep or a per-frame poll of the canvas.
import { loadPlaywright, resolveBaseURL, PRESETS, trackErrors } from './landing-harness.mjs';
import { readdir, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const HERE = fileURLToPath(new URL('.', import.meta.url));
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const playwright = await loadPlaywright();
const assert = (cond, msg) => { if (!cond) throw new Error(msg); };

// Scene boundaries in code-index terms; -1 is the intro (restY(-1) === 0).
const SCENES = [-1, 0, 1, 2, 3, 4];

async function ready(page) {
  await page.goto(baseURL);
  await page.waitForFunction(() => window.__landing.state?.().ready, null, { timeout: 30000 });
}
// The mouse position a wheel event is dispatched at changes what it does --
// measured directly: WebKit only registers scroll input at all over the
// copy column at this viewport, not the visual cell; Chromium is unaffected
// either way. Each engine gets the position proven to work for it.
function gesturePos(engineName) { return engineName === 'webkit' ? [1400, 160] : [50, 400]; }
// A real, no-op gesture: desktop's default scroll driver (watchScrollIntent)
// ignores window.scrollTo() writes entirely until a genuine wheel/pointer/key
// event has armed it (cancelSettle), the same gate a reader's first touch
// clears. One dummy click clears it for the rest of the page's life.
async function arm(page, pos) { await page.mouse.move(...pos); await page.mouse.down(); await page.mouse.up(); }
// shown is 0 from load and stays 0 across the intro<->scene-0 boundary (the
// intro shares scene 1's film, so shown alone cannot signal arrival there),
// so every jump is read off scrollY against restY(), the one quantity that
// works for every target including the intro.
async function gotoScene(page, scene, timeout = 20000) {
  const y = await page.evaluate((s) => window.__landing.restY(s), scene);
  await page.evaluate((y) => scrollTo(0, y), y);
  await page.waitForFunction((y) => Math.abs(scrollY - y) <= 1, y, { timeout });
  // Not just !running: right after the jump, before the wash it triggers
  // has even started, running() is briefly still false too -- waiting on
  // that alone resolves in the gap and hands back a page still showing the
  // *previous* scene (measured directly: a caller acting immediately after
  // dragged an element that, per the pointerdown handler's own shown ===
  // SHIPS guard, was not yet interactive). shown itself is not vacuous here
  // the way it is for the intro, since every real scene 0-4 genuinely
  // changes it.
  if (scene >= 0) await page.waitForFunction((s) => window.__landing.state().shown === s && !window.__landing.state().running, scene, { timeout });
  else await page.waitForFunction(() => !window.__landing.state().running, null, { timeout });
}
// Committed (the dead-zone formula has picked `to`) and arrived (the wash it
// owes has actually finished) are different moments -- sending more input
// once committed but still running just retargets the wash already running
// there, the one thing R3 says a transition must never do, and nothing ever
// settles.
async function committed(page, to) {
  return to < 0
    ? page.evaluate(() => Math.abs(scrollY) <= 2)
    : page.evaluate((s) => window.__landing.state().target === s, to);
}
async function arrived(page, to) {
  return to < 0
    ? page.evaluate(() => Math.abs(scrollY) <= 2)
    : page.evaluate((s) => window.__landing.state().shown === s && window.__landing.state().target === s && !window.__landing.state().running, to);
}
// A hand that never lets go never commits (watchScrollIntent tracks a held
// wheel 1:1 and only asks the dead-zone formula what scene to land on once
// released), so this is a burst -- a handful of ticks -- then release and
// check, retrying with another burst if the last one was not enough; never
// re-checked mid-burst, and never while a wash from an earlier burst is
// still running, since either retargets a transition that has not finished.
// 3<->4 is the one boundary this whole shape is wrong for: xf is a position
// the reader's own scroll drives directly ("the scrub is a position, not a
// clock"), not a wash that runs on its own once started, so releasing to
// "let it finish" is exactly wrong -- it needs continued input for the
// whole crossfade, ticked without ever releasing until the state itself
// reports rest.
async function gestureCommit(page, from, to, engineName) {
  const pos = gesturePos(engineName);
  const dir = to > from ? 1 : -1;
  if (from === 4 || to === 4) {
    let ticks = 0;
    for (; ticks < 150 && !(await arrived(page, to)); ticks++) {
      await page.mouse.move(...pos);
      await page.mouse.wheel(0, dir * 20);
      await page.waitForTimeout(25);
    }
    assert(ticks < 150, `I-commit ${engineName}: ${from}->${to} never reached rest after ${ticks} ticks`);
    await page.waitForTimeout(400);
    assert(await arrived(page, to), `I-commit ${engineName}: ${from}->${to} bounced back off its rest`);
    return;
  }
  let rounds = 0;
  for (; rounds < 40 && !(await committed(page, to)); rounds++) {
    await page.waitForFunction(() => !window.__landing.state().running, null, { timeout: 20000 });
    const magnitude = engineName === 'webkit' ? 10 : 3;
    for (let i = 0; i < 3; i++) { await page.mouse.move(...pos); await page.mouse.wheel(0, dir * magnitude); await page.waitForTimeout(20); }
    await page.waitForTimeout(150);
  }
  assert(rounds < 40, `I-commit ${engineName}: ${from}->${to} never committed after ${rounds} bursts`);
  await page.waitForFunction((s) => s < 0 || (window.__landing.state().shown === s && !window.__landing.state().running), to, { timeout: 20000 }).catch(async (error) => {
    const info = await page.evaluate(() => window.__landing.state());
    throw new Error(`I-commit ${engineName}: ${from}->${to} committed but never finished arriving: ${JSON.stringify(info)}`, { cause: error });
  });
  await page.waitForTimeout(400);
  assert(await arrived(page, to), `I-commit ${engineName}: ${from}->${to} bounced back off its rest`);
}

function mulberry32(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6D2B79F5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// I-commit (R3): every boundary, both directions. Each direction starts
// from a fresh jump to its own origin rather than a walk, so one
// boundary's gesture never carries leftover spring state into the next.
async function iCommit(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const pos = gesturePos(engineName);
    const page = await browser.newPage(PRESETS.desktop);
    await ready(page);
    await arm(page, pos);
    for (let i = 0; i < SCENES.length - 1; i++) {
      const from = SCENES[i], to = SCENES[i + 1];
      await gotoScene(page, from);
      await gestureCommit(page, from, to, engineName);
      await gotoScene(page, to);
      await gestureCommit(page, to, from, engineName);
    }
    console.log(`${engineName}: I-commit every boundary intro..close commits forward and back with one light gesture`);
    await page.close();
  }
}

// I-rect (R2): seeded drags of two scene-1 plates and one scene-3 card must
// all stay inside the print rectangle #gl reports in its own inline style.
async function iRect(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.desktop);
    await ready(page);
    await arm(page, gesturePos(engineName));
    await gotoScene(page, 0);
    const rand = mulberry32(20260920);
    const plateIds = await page.evaluate(() => [...document.querySelectorAll('.plate')].slice(0, 2).map((el) => el.id));
    for (const id of plateIds) {
      const box = await page.locator(`[id="${id}"]`).boundingBox(); // plate ids embed a filename (a literal dot), so #id would read as a class selector
      const sx = box.x + box.width / 2, sy = box.y + box.height / 2;
      const dx = (rand() - 0.5) * 1600, dy = (rand() - 0.5) * 1200;
      await page.mouse.move(sx, sy);
      await page.mouse.down();
      await page.mouse.move(sx + dx / 2, sy + dy / 2, { steps: 4 });
      await page.mouse.move(sx + dx, sy + dy, { steps: 4 });
      await page.mouse.up();
    }
    await gotoScene(page, 2);
    await page.waitForFunction(() => document.getElementById('stage').classList.contains('s3-ready'), null, { timeout: 15000 });
    const cardBox = await page.locator('#sib-nb').boundingBox();
    const cx = cardBox.x + cardBox.width / 2, cy = cardBox.y + cardBox.height / 2;
    const cdx = (rand() - 0.5) * 1800, cdy = (rand() - 0.5) * 1400;
    await page.mouse.move(cx, cy);
    await page.mouse.down();
    await page.waitForTimeout(30);
    await page.mouse.move(cx + cdx / 2, cy + cdy / 2, { steps: 8 });
    await page.waitForTimeout(30);
    await page.mouse.move(cx + cdx, cy + cdy, { steps: 8 });
    await page.waitForTimeout(30);
    await page.mouse.up();
    await page.waitForTimeout(500);

    const data = await page.evaluate(() => {
      const gl = document.getElementById('gl');
      const rect = { x: parseFloat(gl.style.left), y: parseFloat(gl.style.top), w: parseFloat(gl.style.width), h: parseFloat(gl.style.height) };
      const plates = [...document.querySelectorAll('.plate')].map((el) => ({ id: el.id, x: el.offsetLeft, y: el.offsetTop, w: el.offsetWidth, h: el.offsetHeight }));
      const cardRect = (id) => {
        const el = document.getElementById(id);
        const m = /translate\(([-\d.]+)px,\s*([-\d.]+)px\) scale\(([-\d.]+)\)/.exec(el.style.transform);
        const [, x, y, s] = m.map(Number);
        return { id, x, y, w: parseFloat(el.style.width) * s, h: parseFloat(el.style.height) * s };
      };
      const cards = ['s3-video', 'sib-nb', 'sib-sk'].map(cardRect);
      return { rect, plates, cards };
    });
    const outside = [...data.plates, ...data.cards].filter((r) =>
      r.x < data.rect.x - 0.5 || r.y < data.rect.y - 0.5 || r.x + r.w > data.rect.x + data.rect.w + 0.5 || r.y + r.h > data.rect.y + data.rect.h + 0.5);
    assert(outside.length === 0, `I-rect ${engineName}: outside the print rect ${JSON.stringify(data.rect)}: ${JSON.stringify(outside)}`);
    console.log(`${engineName}: I-rect the print rectangle contains every dragged plate and card`);
    await page.close();
  }
}

// I-scene3 (R6): cold load, before any input but the one jump that reaches
// it -- only the video is inside the preview, all three artifacts sit as
// siblings of the shell rather than nested in it, and shell and paper (the
// nested nested preview document) carry the same computed transform.
async function iScene3(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.desktop);
    await ready(page);
    await arm(page, gesturePos(engineName));
    await gotoScene(page, 2);
    await page.waitForFunction(() => document.getElementById('stage').classList.contains('s3-ready'), null, { timeout: 15000 });
    const info = await page.evaluate(() => {
      const artifact = (elId, key) => {
        const el = document.getElementById(elId);
        return { key, placement: el.dataset.placement, parent: el.parentElement.id };
      };
      const box = document.getElementById('box');
      let paperTransform = null;
      try {
        const nested = document.getElementById('vd').contentDocument.getElementById('moss-preview-iframe');
        paperTransform = getComputedStyle(nested).transform;
      } catch (e) { paperTransform = null; }
      return {
        cards: [artifact('s3-video', 'video'), artifact('sib-nb', 'nb'), artifact('sib-sk', 'sk')],
        boxParent: box.parentElement.id,
        shellTransform: getComputedStyle(box).transform,
        paperTransform,
      };
    });
    const video = info.cards.find((c) => c.key === 'video');
    const others = info.cards.filter((c) => c.key !== 'video');
    assert(video.placement === 'preview', `I-scene3 ${engineName}: video is not inside the preview on cold load (${video.placement})`);
    assert(others.every((c) => c.placement === 'outside'), `I-scene3 ${engineName}: an artifact besides the video is inside the preview: ${JSON.stringify(others)}`);
    assert(info.cards.every((c) => c.parent === info.boxParent), `I-scene3 ${engineName}: an artifact is not a sibling of the shell: ${JSON.stringify(info.cards)}`);
    assert(info.paperTransform !== null, `I-scene3 ${engineName}: could not read the nested preview's transform`);
    assert(info.shellTransform === info.paperTransform, `I-scene3 ${engineName}: shell (${info.shellTransform}) and paper (${info.paperTransform}) transforms differ`);
    console.log(`${engineName}: I-scene3 cold load: only the video is docked, all three artifacts sit beside the shell, shell and paper share one transform`);
    await page.close();
  }
}

// I-reduced (R7): under reducedMotion, a gesture toward each scene's rest
// arrives there and the settle writes scrollTo at most once.
async function iReduced(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.reducedMotion);
    await page.addInitScript(() => {
      window.__scrollToCalls = 0;
      const native = window.scrollTo.bind(window);
      window.scrollTo = (...args) => { window.__scrollToCalls++; return native(...args); };
    });
    await ready(page);
    const pos = gesturePos(engineName);
    await arm(page, pos);
    for (const scene of [0, 1, 2, 3, 4]) {
      const goalY = await page.evaluate((s) => window.__landing.restY(s), scene);
      await page.evaluate(() => { window.__scrollToCalls = 0; });
      // A wheel delta sized for the actual gap to this scene, not a fixed
      // large one: reduced motion has no travel-distance limit of its own,
      // so an oversized delta sails straight through several scenes at
      // once (measured: 4000 overshot scene 0 all the way to the close).
      // Deliberately short of the exact rest, same as a real gesture: an
      // exact delta lands settleAtRest already at x=0, which returns before
      // ever choosing between an instant jump and the per-frame spring, so
      // the one-write invariant would hold even with that choice deleted.
      const delta = Math.round(goalY - (await page.evaluate(() => scrollY))) - 40;
      await page.mouse.move(...pos);
      await page.mouse.wheel(0, delta); // one real gesture per scene; reduced motion arrives, it does not travel
      // shown alone is vacuous at whichever scene the page already sits on
      // (0, before any gesture) -- scrollY against the actual rest rules
      // that out, same as every jump elsewhere in this file.
      await page.waitForFunction((y) => Math.abs(scrollY - y) <= 1, goalY, { timeout: 15000 }).catch(async (error) => {
        const info = await page.evaluate(() => ({ y: scrollY, state: window.__landing.state() }));
        throw new Error(`I-reduced ${engineName}: scene ${scene} never settled: ${JSON.stringify(info)}`, { cause: error });
      });
      await page.waitForTimeout(200);
      const state = await page.evaluate(() => window.__landing.state());
      assert(state.shown === scene && !state.running, `I-reduced ${engineName}: scene ${scene} reached the position but not rest: ${JSON.stringify(state)}`);
      const calls = await page.evaluate(() => window.__scrollToCalls);
      assert(calls <= 1, `I-reduced ${engineName}: scene ${scene}'s arrival wrote scrollTo ${calls} times (JS-driven animation, not an instant arrival)`);
    }
    console.log(`${engineName}: I-reduced every scene rest is reached with at most one scrollTo write`);
    await page.close();
  }
}

// I-fuzz (R7): a seeded run of fast, unwaited jumps must still end at rest on
// a rendered scene with no page errors. The time bound is asserted in
// Chromium only.
async function iFuzz(browsers) {
  const seed = 20260920;
  console.log(`I-fuzz seed: ${seed}`);
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.desktop);
    const errors = trackErrors(page);
    await ready(page);
    await arm(page, gesturePos(engineName));
    const rand = mulberry32(seed);
    const maxY = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
    const start = Date.now();
    for (let i = 0; i < 20; i++) {
      const y = Math.round(rand() * maxY);
      await page.evaluate((y) => scrollTo(0, y), y);
    }
    // Not just !running: a page that never starts the join it owes is also
    // never running, so that alone can't tell "arrived" from "stuck before
    // it ever tried" -- shown === target is what actually says it got there.
    await page.waitForFunction(() => { const s = window.__landing.state(); return s.shown === s.target && !s.running; }, null, { timeout: 30000 }).catch(async (error) => {
      const info = await page.evaluate(() => window.__landing.state());
      throw new Error(`I-fuzz ${engineName}: never settled: ${JSON.stringify(info)}`, { cause: error });
    });
    const elapsed = Date.now() - start;
    const rendered = await page.evaluate(() => {
      const stage = document.getElementById('stage');
      return stage.dataset.scene != null && getComputedStyle(document.querySelector('.page')).display !== 'none';
    });
    assert(rendered, `I-fuzz ${engineName}: not left on a rendered scene`);
    assert(errors.length === 0, `I-fuzz ${engineName}: page errors: ${errors.join('; ')}`);
    if (engineName === 'chromium') assert(elapsed < 20000, `I-fuzz chromium: took ${elapsed}ms to settle after the fuzz, over the 20s budget`);
    console.log(`${engineName}: I-fuzz seed ${seed}, ${elapsed}ms, ended at rest on a rendered scene with no page errors`);
    await page.close();
  }
}

// I-default: every check script's own default navigation loads the
// unflagged URL. A carry= baked into a literal .goto() call site is exactly
// the historical bug (design doc: "the fixed driver was never made the
// default URL and no test loaded the unflagged page") -- a script's own
// separate, explicit test of the legacy ?carry= flag (check-landing-
// mobile.mjs's mobilePage('?carry=css') case) is not this, and is not
// flagged: it never appears on a `.goto(` line itself.
async function iDefault() {
  const entries = await readdir(HERE);
  const files = entries.filter((f) => /^check-(landing|site-preview|docs|favicon)/.test(f) && f.endsWith('.mjs') && f !== 'check-landing-invariants.mjs' && f !== 'check-landing-all.mjs' && f !== 'check-landing-structure.mjs');
  const offenders = [];
  for (const file of files) {
    const src = await readFile(`${HERE}${file}`, 'utf8');
    for (const line of src.split('\n')) {
      if (line.includes('.goto(') && line.includes('carry=')) offenders.push(`${file}: ${line.trim()}`);
    }
  }
  assert(offenders.length === 0, `I-default: a script's own .goto() call bakes in carry=: ${offenders.join('; ')}`);
  console.log(`-: I-default all ${files.length} check scripts navigate to the unflagged URL by default`);
}

const browsers = {};
try {
  browsers.chromium = await playwright.chromium.launch();
  browsers.webkit = await playwright.webkit.launch();
  await iCommit(browsers);
  await iRect(browsers);
  await iScene3(browsers);
  await iReduced(browsers);
  await iFuzz(browsers);
  await iDefault();
} finally {
  await Promise.all(Object.values(browsers).map((b) => b.close()));
  await close();
}
