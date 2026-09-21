#!/usr/bin/env node
// The rules the page must keep (design doc R1-R8), each with a named fault
// shown red then green before it was trusted -- see each function below for
// the one-line record:
//
// I-commit  (R3) fault: watchScrollIntent's own `setTarget(Math.max(0,
//           scene))` call site deleted -- no scroll ever picks a target
//           scene again, so every boundary times out instead of committing.
//           RED/GREEN recorded below each function.
// I-rect    (R2) checked after EACH drag now, not only once at the end --
//           plates first, before any card is ever touched, then again after
//           the card. Two independent faults, each proven on its own: the
//           plate drop's own `printExpanded = true;
//           setPrintRect(currentPrintRect());` (site/landing.js, the
//           outsideBase branch) deleted -- red on the very next plate check,
//           restored -- green. The same two lines deleted from endDrag()'s
//           card-drop branch instead -- red on the card check, restored --
//           green. (Checking only once at the end, as the first version of
//           this file did, let the *other* site's later call rescue a
//           broken one retroactively and stay green -- that gap is closed.)
// I-scene3  (R6) fault: CARDS.video's `docked: true` flipped to false -- the
//           video is no longer inside the preview on cold load.
// I-reduced (R7) fault: the `if (reduce) return scrollTo(0, y);` call site in
//           settleAtRest deleted -- reduced motion falls through to the
//           per-frame spring, writing scrollTo on every frame of the settle.
//           (Needed a deliberately short wheel delta to expose: an exact one
//           already lands settleAtRest at x=0, which returns before ever
//           choosing between the two paths.)
// I-fuzz    (R7) shown === target was too weak: with maybeJoin()'s own
//           `runJoin()` call site deleted (site/landing.js), shown never
//           moves again, yet that check still passed every time -- not
//           because of print pre-capture (the first theory here), but
//           because this file's own ready(page) helper waited on
//           state().ready, which can go true *while boot's own ready() is
//           still deciding whether to fire its own, separate runJoin() call
//           (site/landing.js, the very end of that function, independent of
//           maybeJoin() entirely)* -- so the fuzz's early jumps raced that
//           decision and a real join sometimes ran anyway, settling shown
//           for reasons that had nothing to do with the call site under
//           test. Fixed by waiting on document.documentElement.dataset.ready
//           instead, the flag boot's ready() sets only after that decision
//           is made (matches check-landing-transitions.mjs's own wait).
//           With the race closed, the end condition was also strengthened
//           to demand shown === landing.sceneForRest(progress, travel) (the
//           formula settleAtRest itself uses) plus the DOM actually showing
//           that scene, not covered by the canvas. Under the same
//           maybeJoin() deletion this now times out for real: state() at
//           the 30s timeout reports shown stuck at 0, target and
//           sceneForRest both moved to 2, joins:0, washes:0 -- restored --
//           green. A second seeded variant, iFuzzInvalidated, drags a
//           scene-1 plate far enough to expand the print rectangle first
//           (the one path that actually clears every held print mid-visit,
//           the historical deadlock's real precondition -- boot pre-capture
//           is why nulling prints or hanging every capture, tried earlier,
//           never got there); it still reaches rest on the right scene with
//           no fault, both engines. faults.captureHang against that same
//           invalidated state is a real, reported finding, not a bug fixed
//           here: shown/target freeze and never move, but the loose
//           shown === target shape would call that "settled" -- state().
//           primed and state().ready both go false and stay false, which is
//           the tell. Left OUT of this suite and saved as
//           scratchpad/phase2c/fuzz-capturehang.mjs with its captured
//           output, for phase 7 (bounded print acquisition) to pick up as
//           an exit test.
// I-default (checks, not the page) fault: a scratch script with
//           `page.goto(base + '?carry=intent')` -- the static scan catches it.
// I-plate, I-window-radius, I-pub-cue, I-header-scrim: added 2026-09-20 for
//           the site owner's four visual-polish requests (scene 1 plate
//           margins, scene 2's Publish cue, the preview window's corner
//           radius, mobile scene 5's header contrast). Each function's own
//           header comment carries its RED/GREEN record.
//
// Rules: every scene reach uses a real armed gesture plus window.__landing's
// own restY()/state(), never a fixed sleep or a per-frame poll of the canvas.
import { loadPlaywright, resolveBaseURL, PRESETS, trackErrors, whenReady } from './landing-harness.mjs';
import { readdir, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const HERE = fileURLToPath(new URL('.', import.meta.url));
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const playwright = await loadPlaywright();
const assert = (cond, msg) => { if (!cond) throw new Error(msg); };

// Scene boundaries in code-index terms; -1 is the intro (restY(-1) === 0).
const SCENES = [-1, 0, 1, 2, 3, 4];
// site/landing.js's own PHASE names for the two indices this file's new
// checks read by name rather than a bare number -- PHASE itself lives in
// that module's scope, not this process's, so the names are repeated here.
const LIVE = 1, SHIPS = 2;

// landing-harness.mjs's whenReady() is what found and closed this file's own
// I-fuzz race (see the header above): state().ready can go true while boot's
// ready() is still deciding whether to fire its own runJoin() call, so
// waiting on it alone let a real join run for reasons unrelated to the call
// site under test. Every landing check now shares that one wait instead of
// keeping a per-script variant of it.
async function ready(page) {
  await page.goto(baseURL);
  await whenReady(page);
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
// all stay inside the print rectangle #gl reports in its own inline style --
// checked right after EACH drag, not only once at the end. A plate's own
// expansion call site and the card's are two independent sites (both listed
// in currentPrintRect()'s own callers); deleting only one used to still pass,
// because the *other* site's later currentPrintRect() call unions in
// whatever the first drag left stranded and rescues it retroactively. Fault:
// deleting the plate drop handler's `printExpanded = true;
// setPrintRect(currentPrintRect());` (site/landing.js, the outsideBase
// branch) -- red on the plate check, before any card is ever touched, then
// restored -- green. Fault: deleting the same two lines from endDrag()'s
// card-drop branch -- red on the card check, then restored -- green. Each
// proven independently, in that order, with the other site left intact.
async function readPrintRect(page) {
  return page.evaluate(() => {
    const gl = document.getElementById('gl');
    return { x: parseFloat(gl.style.left), y: parseFloat(gl.style.top), w: parseFloat(gl.style.width), h: parseFloat(gl.style.height) };
  });
}
async function readPlateRects(page) {
  return page.evaluate(() => [...document.querySelectorAll('.plate')].map((el) => ({ id: el.id, x: el.offsetLeft, y: el.offsetTop, w: el.offsetWidth, h: el.offsetHeight })));
}
async function readCardRects(page, ids) {
  return page.evaluate((ids) => ids.map((id) => {
    const el = document.getElementById(id);
    const m = /translate\(([-\d.]+)px,\s*([-\d.]+)px\) scale\(([-\d.]+)\)/.exec(el.style.transform);
    const [, x, y, s] = m.map(Number);
    return { id, x, y, w: parseFloat(el.style.width) * s, h: parseFloat(el.style.height) * s };
  }), ids);
}
function assertContained(rect, rects, engineName, label) {
  const outside = rects.filter((r) =>
    r.x < rect.x - 0.5 || r.y < rect.y - 0.5 || r.x + r.w > rect.x + rect.w + 0.5 || r.y + r.h > rect.y + rect.h + 0.5);
  assert(outside.length === 0, `I-rect ${engineName}: ${label}: outside the print rect ${JSON.stringify(rect)}: ${JSON.stringify(outside)}`);
}
async function dragBy(page, sx, sy, dx, dy, steps = 4) {
  await page.mouse.move(sx, sy);
  await page.mouse.down();
  await page.waitForTimeout(30);
  await page.mouse.move(sx + dx / 2, sy + dy / 2, { steps });
  await page.waitForTimeout(30);
  await page.mouse.move(sx + dx, sy + dy, { steps });
  await page.waitForTimeout(30);
  await page.mouse.up();
  await page.waitForTimeout(150);
}
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
      const dx = (rand() - 0.5) * 1600, dy = (rand() - 0.5) * 1200;
      await dragBy(page, box.x + box.width / 2, box.y + box.height / 2, dx, dy);
      // Checked here, before the scene-3 card is ever touched: the card
      // drop's own call site is a second, later chance to union this plate
      // back in, which would hide a broken plate call site entirely.
      assertContained(await readPrintRect(page), await readPlateRects(page), engineName, `after dragging plate ${id}`);
    }
    await gotoScene(page, 2);
    await page.waitForFunction(() => document.getElementById('stage').classList.contains('s3-ready'), null, { timeout: 15000 });
    const cardBox = await page.locator('#sib-nb').boundingBox();
    const cdx = (rand() - 0.5) * 1800, cdy = (rand() - 0.5) * 1400;
    await dragBy(page, cardBox.x + cardBox.width / 2, cardBox.y + cardBox.height / 2, cdx, cdy, 8);
    await page.waitForTimeout(350);
    const rect = await readPrintRect(page);
    assertContained(rect, await readPlateRects(page), engineName, 'after dragging the card (plates)');
    assertContained(rect, await readCardRects(page, ['s3-video', 'sib-nb', 'sib-sk']), engineName, 'after dragging the card (cards)');
    console.log(`${engineName}: I-rect the print rectangle contains every dragged plate and card, checked after each drag`);
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
// the RIGHT rendered scene with no page errors, not merely "some scene,
// whatever shown last happened to hold". shown === target was too weak: with
// runJoin()'s call site deleted from maybeJoin() (site/landing.js), shown
// never moves again, yet that check still passed on every run, both engines
// -- because watchScrollIntent keeps computing target from the live scroll
// position every frame regardless of whether a join ever ran, and this
// codebase's rest formula (sceneForRest) is defined purely on that position,
// not on shown, so target kept landing back on whatever scene the reader's
// last motion implied. shown was simply never being asked to prove it
// tracked anything. Now it is: the end condition also demands shown equal
// landing.sceneForRest(progress, travel) itself -- the same formula
// settleAtRest uses -- plus the DOM actually showing that scene (stage's own
// data-scene attribute) with the canvas not covering it (stage not
// .morphing). Under the same runJoin() deletion this new condition times out
// for real: state() at the 30s timeout reports shown stuck at its pre-fuzz
// value while target (and sceneForRest of the final position) has moved on,
// confirmed red then the deletion was restored and confirmed green -- see
// the commit this landed in for the captured state().
async function fuzzToRest(page, engineName, rand, minY, maxY, label) {
  const start = Date.now();
  for (let i = 0; i < 20; i++) {
    const y = Math.round(minY + rand() * (maxY - minY));
    await page.evaluate((y) => scrollTo(0, y), y);
  }
  await page.waitForFunction(() => {
    const s = window.__landing.state();
    if (s.running || s.shown !== s.target) return false;
    return s.shown === window.__landing.sceneForRest(s.progress, s.travel);
  }, null, { timeout: 30000 }).catch(async (error) => {
    const diag = await page.evaluate(() => ({ state: window.__landing.state(), sceneForRest: window.__landing.sceneForRest(window.__landing.state().progress, window.__landing.state().travel) }));
    throw new Error(`${label} ${engineName}: never settled on the right scene: ${JSON.stringify(diag)}`, { cause: error });
  });
  const elapsed = Date.now() - start;
  // The morphing class comes off on its own held/!running rAF check (still()'s
  // caller elsewhere in site/landing.js), not synchronously with shown/target
  // settling -- a frame or two of lag here is real and not itself a fault.
  await page.waitForTimeout(300);
  const info = await page.evaluate(() => {
    const stage = document.getElementById('stage');
    const state = window.__landing.state();
    return {
      shown: state.shown,
      sceneAttr: stage.dataset.scene,
      morphing: stage.classList.contains('morphing'),
      rendered: stage.dataset.scene != null && getComputedStyle(document.querySelector('.page')).display !== 'none',
    };
  });
  assert(info.rendered, `${label} ${engineName}: not left on a rendered scene`);
  assert(!info.morphing, `${label} ${engineName}: canvas still covering the stage at rest`);
  assert(info.sceneAttr === String(info.shown), `${label} ${engineName}: stage shows scene ${info.sceneAttr}, shown says ${info.shown}`);
  return elapsed;
}
async function iFuzz(browsers) {
  const seed = 20260920;
  console.log(`I-fuzz seed: ${seed}`);
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.desktop);
    const errors = trackErrors(page);
    await ready(page);
    await arm(page, gesturePos(engineName));
    const rand = mulberry32(seed);
    const minY = await page.evaluate(() => window.__landing.restY(0));
    const maxY = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
    const elapsed = await fuzzToRest(page, engineName, rand, minY, maxY, 'I-fuzz');
    assert(errors.length === 0, `I-fuzz ${engineName}: page errors: ${errors.join('; ')}`);
    if (engineName === 'chromium') assert(elapsed < 20000, `I-fuzz chromium: took ${elapsed}ms to settle after the fuzz, over the 20s budget`);
    console.log(`${engineName}: I-fuzz seed ${seed}, ${elapsed}ms, ended at rest on the right rendered scene with no page errors`);
    await page.close();
  }
}

// I-fuzz, invalidated-print variant: the historical deadlock this guards
// against needed a print actually missing mid-visit, not merely never taken
// -- boot's ready() pre-captures every scene before any fuzzing starts,
// which is why nulling one print or all of them (site/landing.js's own
// fillPrints() path) and forcing every fresh capture to hang
// (faults.captureHang) both did nothing to the plain I-fuzz above. Dragging
// a plate or card far enough to expand the print rectangle is the one path
// that actually clears every held print (setPrintRect -> sheets.fill(null)),
// mid-session, the same way the incident this rule traces to happened. Run
// with no fault: must still reach rest on the right scene, same as I-fuzz.
async function iFuzzInvalidated(browsers) {
  const seed = 20260921;
  console.log(`I-fuzz (invalidated prints) seed: ${seed}`);
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.desktop);
    const errors = trackErrors(page);
    await ready(page);
    await arm(page, gesturePos(engineName));
    await gotoScene(page, 0);
    const glRect = () => page.evaluate(() => { const g = document.getElementById('gl'); return `${g.style.left}/${g.style.top}/${g.style.width}/${g.style.height}`; });
    const before = await glRect();
    const box = await page.locator('.plate').first().boundingBox();
    await dragBy(page, box.x + box.width / 2, box.y + box.height / 2, 1400, 900);
    const after = await glRect();
    // setPrintRect nulls every held print the instant the rect actually
    // changes (sheets.fill(null)), but retakeShown() -- called unconditionally
    // after every plate drop, not only an outside-base one -- recaptures the
    // scene on screen right behind it, so by the time this check runs the
    // currently shown scene's own print is legitimately back. The rect
    // itself changing is the real signal this path fired at all; the other
    // four scenes' prints (not the one just recaptured) still prove the
    // invalidation reached them.
    assert(before !== after, `I-fuzz-invalidated ${engineName}: the drag did not expand the print rect (${before}), so this variant tests nothing beyond I-fuzz`);
    const stillCleared = await page.evaluate(() => { const shown = window.__landing.state().shown; return window.__landing.prints.every((p, i) => i === shown || p == null); });
    assert(stillCleared, `I-fuzz-invalidated ${engineName}: a print for a scene not on screen survived the rect change`);
    const rand = mulberry32(seed);
    const minY = await page.evaluate(() => window.__landing.restY(0));
    const maxY = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
    const elapsed = await fuzzToRest(page, engineName, rand, minY, maxY, 'I-fuzz-invalidated');
    assert(errors.length === 0, `I-fuzz-invalidated ${engineName}: page errors: ${errors.join('; ')}`);
    console.log(`${engineName}: I-fuzz-invalidated seed ${seed}, ${elapsed}ms, reached rest on the right scene after every print was cleared mid-visit`);
    await page.close();
  }
}

// I-plate (site owner, 2026-09-20) fault: PLATE_ASPECT's ternary (site/
// landing.js) only ever filled in the zh set -- the English (blake) set
// defaulted every plate to aspect 1, `|| 1` -- so scatterPlates sized a
// square box for an image of any real shape, and object-fit: contain then
// painted the box's own #d9d2c4 background into whatever the square left
// over. RED on that state (restored below, one line): every blake plate's
// boxAspect/natAspect ratio was off by double digits, not 1%. GREEN once
// PLATE_ASPECT carried real per-file ratios for both sets. A second,
// independent fault was found composing the fix: the 96px short-side floor
// clamped only the short side, so a very wide or very tall image (blake's
// 640x237, aspect 2.7) still got a distorted box whenever the random draw
// put `long` under ~short-floor*aspect -- fixed by raising `long` itself
// instead of clamping the side computed from it.
async function readPlateAspects(page) {
  return page.evaluate(() => [...document.querySelectorAll('.plate')].map((pl) => {
    const img = pl.querySelector('img');
    const box = { w: pl.offsetWidth, h: pl.offsetHeight };
    const nat = { w: img.naturalWidth, h: img.naturalHeight };
    // Replicate object-fit: contain by hand against the box the page
    // actually rendered -- not a second look at the aspect number
    // scatterPlates used, which would only ever re-confirm its own math.
    const scale = Math.min(box.w / nat.w, box.h / nat.h);
    const cw = nat.w * scale, ch = nat.h * scale;
    const c = document.createElement('canvas'); c.width = Math.round(box.w); c.height = Math.round(box.h);
    const g = c.getContext('2d');
    g.fillStyle = getComputedStyle(pl).backgroundColor; g.fillRect(0, 0, c.width, c.height);
    g.drawImage(img, (box.w - cw) / 2, (box.h - ch) / 2, cw, ch);
    const uniform = (data) => { for (let i = 4; i < data.length; i += 4) if (data[i] !== data[0] || data[i + 1] !== data[1] || data[i + 2] !== data[2]) return false; return true; };
    const edgeBar = ['top', 'bottom', 'left', 'right'].some((side) => {
      const d = side === 'top' ? g.getImageData(0, 0, c.width, 1).data
        : side === 'bottom' ? g.getImageData(0, c.height - 1, c.width, 1).data
        : side === 'left' ? g.getImageData(0, 0, 1, c.height).data
        : g.getImageData(c.width - 1, 0, 1, c.height).data;
      return uniform(d);
    });
    return { id: pl.id, boxAspect: box.w / box.h, natAspect: nat.w / nat.h, edgeBar };
  }));
}
async function iPlate(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.desktop);
    await ready(page);
    await page.waitForFunction(() => [...document.querySelectorAll('.plate img')].every((im) => im.classList.contains('in')), null, { timeout: 15000 });
    const rows = await readPlateAspects(page);
    for (const r of rows) {
      const err = Math.abs(r.boxAspect - r.natAspect) / r.natAspect;
      assert(err <= 0.01, `I-plate ${engineName}: ${r.id} box aspect ${r.boxAspect.toFixed(3)} vs natural ${r.natAspect.toFixed(3)} (${(err * 100).toFixed(1)}% off)`);
      assert(!r.edgeBar, `I-plate ${engineName}: ${r.id} renders a solid-colour bar at a box edge`);
    }
    console.log(`${engineName}: I-plate all ${rows.length} plates render at their image's natural aspect (<=1% off) with no edge letterbox`);
    await page.close();
  }
}

// I-pub-cue (site owner, 2026-09-20; made event-driven 2026-09-21): the old
// "Try publishing" label plus a small pulsing ring (site/ui/shell.html) is
// replaced by a body-level ring layer (site/index.html #pub-cue), synced
// from scenes()'s own dispatch rather than a poll -- R8 says the page does
// no periodic work at rest. RED (#pub-cue's `phase === 'live'` condition in
// syncPubCue changed to always-true, restored after): the ring stays on in
// scene 3 too, where this asserts it must not. GREEN restored. A second RED,
// for the timer assertion specifically (site/landing.js's own prior
// `setInterval(syncPubCue, 250)`, before it was replaced by the scenes()/
// resize/reduced-motion/click listeners below, restored after removing the
// interval): 7-8 setInterval ticks over the 2s window instead of 0.
async function iPubCue(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.desktop);
    // Wraps every setInterval callback (registered at any time, including
    // during boot) so a tick anywhere is visible -- counting calls to
    // setInterval itself would miss an interval registered once at load and
    // still firing, which is exactly the fault this exists to catch.
    await page.addInitScript(() => {
      window.__intervalTicks = 0;
      const real = window.setInterval;
      window.setInterval = function (fn, ms, ...rest) {
        return real.call(this, (...args) => { window.__intervalTicks++; return fn(...args); }, ms, ...rest);
      };
    });
    await ready(page);
    await arm(page, gesturePos(engineName));
    const anyTryPublishing = await page.evaluate(() => {
      const docs = [document, document.getElementById('sh')?.contentDocument, document.getElementById('vd')?.contentDocument].filter(Boolean);
      return docs.some((d) => d.body && d.body.innerText.includes('Try publishing') || d.body && (d.body.innerText.includes('試試發布') || d.body.innerText.includes('试试发布')));
    });
    assert(!anyTryPublishing, `I-pub-cue ${engineName}: "Try publishing" (or its zh strings) still appears in the DOM`);
    await gotoScene(page, LIVE);
    const atRest = await page.evaluate(() => {
      const el = document.getElementById('pub-cue');
      const cs = getComputedStyle(el);
      return { on: el.classList.contains('on'), cx: parseFloat(cs.getPropertyValue('--pub-ring-cx')), cy: parseFloat(cs.getPropertyValue('--pub-ring-cy')), r0: parseFloat(cs.getPropertyValue('--pub-ring-r0')), k: parseFloat(cs.getPropertyValue('--pub-ring-k')) };
    });
    assert(atRest.on, `I-pub-cue ${engineName}: cue is off at rest in scene 2 (LIVE)`);
    const rFinal = atRest.r0 * atRest.k;
    const vp = page.viewportSize();
    const exceeds = atRest.cx - rFinal < 0 && atRest.cx + rFinal > vp.width && atRest.cy - rFinal < 0 && atRest.cy + rFinal > vp.height;
    assert(exceeds, `I-pub-cue ${engineName}: a ring's end-of-life box (${JSON.stringify(atRest)}, rFinal ${rFinal}) does not clear all four viewport edges (${vp.width}x${vp.height})`);
    // Reset here, after settling at rest (boot's own short-lived polls, e.g.
    // waiting for each iframe's window API, have already ticked and
    // cleared by now) -- what is asserted is silence from THIS point.
    await page.evaluate(() => { window.__intervalTicks = 0; });
    await page.waitForTimeout(2000);
    const ticks = await page.evaluate(() => window.__intervalTicks);
    assert(ticks === 0, `I-pub-cue ${engineName}: ${ticks} setInterval tick(s) fired in 2s at rest in scene 2 (LIVE) -- R8 forbids periodic work at rest`);
    await gotoScene(page, SHIPS);
    const animCount = await page.evaluate(() => document.getElementById('pub-cue').getAnimations({ subtree: true }).length);
    assert(animCount === 0, `I-pub-cue ${engineName}: ${animCount} ring animation(s) still running in scene 3, where the cue must be off`);
    console.log(`${engineName}: I-pub-cue no "Try publishing" text; ring at rest clears the viewport on all four sides; zero animating outside scene 2; zero interval ticks in 2s at rest`);
    await page.close();
  }
}

// I-window-radius (site owner, 2026-09-20): #box shares the harvested
// shell's own coordinate space 1:1 (the iframe is width/height:100%, no
// separate zoom), so its border-radius is directly comparable to the
// Publish button's own radius measured inside that iframe. moss-desktop's
// own tokens.css: --moss-window-radius: 24px (mirrors Rust
// WINDOW_CORNER_RADIUS), --moss-pill-size: 36px with border-radius
// --moss-pill-radius (half the pill) = 18px -- a 24:18 relationship. RED
// (site/index.html's --win-r reverted to its old 12px, restored after):
// ratio 12/18 = 0.667, nowhere near 24/18 = 1.333. GREEN at --win-r: 24px.
async function iWindowRadius(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.desktop);
    await ready(page);
    await arm(page, gesturePos(engineName));
    await gotoScene(page, LIVE);
    const { boxRadius, btnRadius } = await page.evaluate(() => {
      const box = document.getElementById('box');
      const btn = document.getElementById('sh').contentDocument.querySelector('.moss-publish-button');
      return { boxRadius: parseFloat(getComputedStyle(box).borderRadius), btnRadius: btn.getBoundingClientRect().width / 2 };
    });
    const ratio = boxRadius / btnRadius, want = 24 / 18;
    assert(Math.abs(ratio - want) < 0.02, `I-window-radius ${engineName}: box radius ${boxRadius}px, button radius ${btnRadius}px, ratio ${ratio.toFixed(3)} != moss-desktop's ${want.toFixed(3)}`);
    console.log(`${engineName}: I-window-radius box ${boxRadius}px / button ${btnRadius}px matches moss-desktop's 24:18`);
    await page.close();
  }
}

// I-header-scrim (site owner, 2026-09-20): mobile scene 5's header (.brand,
// .language-picker) sits over the moving closing film with no dark backing
// of its own -- unlike the closing text, which gets #scrim (opacity var(
// --scrim), itself riding on #five's own var(--xf)). Contrast is checked
// against the worst case ANY frame could show behind a translucent
// background: literal white, not a sampled frame -- provably safe rather
// than dependent on this one clip (measured separately with ffmpeg's
// signalstats over the whole loop: peak luma 215/255, well under white, so
// real footage has more margin than this check demands). RED (the
// site/index.html .brand/.language-picker background rule's own call site
// deleted, restored after): contrast collapses to that of a fully
// transparent background (~1:1, alpha 0). GREEN restored.
function srgbToLinear(c) { c /= 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; }
function relativeLuminance([r, g, b]) { return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b); }
function contrastRatio(rgb1, rgb2) { const [a, b] = [relativeLuminance(rgb1), relativeLuminance(rgb2)].sort((x, y) => y - x); return (a + 0.05) / (b + 0.05); }
function parseRGBA(str) { const m = str.match(/[\d.]+/g).map(Number); return { r: m[0], g: m[1], b: m[2], a: m[3] ?? 1 }; }
function compositeOverWhite({ r, g, b, a }) { return [r * a + 255 * (1 - a), g * a + 255 * (1 - a), b * a + 255 * (1 - a)]; }
async function iHeaderScrim(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.phone);
    await ready(page);
    await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    await page.waitForFunction(() => window.__landing.state().xf === 1 && document.getElementById('five').classList.contains('on'), null, { timeout: 20000 });
    const style = await page.evaluate(() => ({
      brandBg: getComputedStyle(document.querySelector('.brand')).backgroundColor,
      brandFg: getComputedStyle(document.querySelector('.brand .moss-wordmark')).color,
      langBg: getComputedStyle(document.querySelector('.language-picker')).backgroundColor,
      langFg: getComputedStyle(document.querySelector('.language-picker .nav-lang-current')).color,
    }));
    for (const [label, bgStr, fgStr] of [['brand', style.brandBg, style.brandFg], ['language-picker', style.langBg, style.langFg]]) {
      const bg = parseRGBA(bgStr), fg = parseRGBA(fgStr);
      assert(bg.a > 0.3, `I-header-scrim ${engineName}: .${label} background did not darken at xf=1 (${bgStr})`);
      const cr = contrastRatio([fg.r, fg.g, fg.b], compositeOverWhite(bg));
      assert(cr >= 4.5, `I-header-scrim ${engineName}: .${label} contrast ${cr.toFixed(2)} < 4.5 against a worst-case white frame (fg ${fgStr}, bg ${bgStr})`);
      console.log(`${engineName}: I-header-scrim .${label} ${cr.toFixed(2)}:1 against a worst-case white frame`);
    }
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
  await iFuzzInvalidated(browsers);
  await iPlate(browsers);
  await iPubCue(browsers);
  await iWindowRadius(browsers);
  await iHeaderScrim(browsers);
  await iDefault();
} finally {
  await Promise.all(Object.values(browsers).map((b) => b.close()));
  await close();
}
