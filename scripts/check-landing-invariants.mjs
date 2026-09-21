#!/usr/bin/env node
// The rules the page must keep (design doc R1-R8), each with a named fault
// shown red then green before it was trusted -- see each function below for
// the one-line record:
//
// I-commit  (R3) fault: watchScrollDesktop's own `setTarget(Math.max(0,
//           scene))` call site deleted -- no scroll ever picks a target
//           scene again, so every boundary times out instead of committing.
//           RED/GREEN recorded below each function. Unit 0a: folded in
//           check-landing-intent.mjs's only extra coverage (three locales,
//           its own two viewports) as a second loop scoped to intent's own
//           boundaries (intro/0/1) -- never the 3<->4 continuous scrub,
//           which intent never tested either; looping the *full* boundary
//           walk over every combination instead surfaced a real, separate,
//           deterministic problem (gesturePos's fixed [50,400]/[1400,160]
//           does not reliably drive the 3<->4 scrub at 1280x720), outside
//           this fold's job to fix and reverted. Re-ran the setTarget
//           deletion above against the folded function -- still red on the
//           very first combination, restored -- green, both confirming the
//           fold didn't lose any detection power. check-landing-intent.mjs
//           deleted.
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
// I-default (checks, not the page) fault: originally a scratch script with
//           `page.goto(base + '?carry=intent')`. Unit 0a: `?carry=` itself
//           is gone (phase 4), so the literal grep could no longer catch
//           the same shape of mistake under a new name -- generalized to
//           any query string in a quoted `.goto()` literal (ALLOWLIST is
//           the sanctioned opt-out; empty today). RED with a scratch
//           `check-landing-scratch-fault.mjs` containing `page.goto(baseURL
//           + '?x=1')` -- caught and named by file; deleted -- GREEN.
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
async function ready(page, locale = '') {
  await page.goto(new URL(locale, baseURL).href);
  await whenReady(page);
}
// The mouse position a wheel event is dispatched at changes what it does --
// measured directly: WebKit only registers scroll input at all over the
// copy column at this viewport, not the visual cell; Chromium is unaffected
// either way. Each engine gets the position proven to work for it. Unit 0a:
// webkit's own x=1400 sits off the 1280px-wide viewport I-commit's fold-in
// added, and measured weakly there even so (5 ticks moved scrollY 13px vs
// ~135px for every other candidate tried at that width) -- deterministic
// across repeat runs, not load noise, so this now takes the viewport width
// and picks a position proven at 1280/1440/1920 alike rather than assuming
// every caller is at the 1440px desktop default.
function gesturePos(engineName, viewportWidth = 1440) {
  if (engineName !== 'webkit') return [50, 400];
  return viewportWidth < 1400 ? [700, 160] : [1400, 160];
}
// A real, no-op gesture: desktop's default scroll driver (watchScrollDesktop)
// ignores window.scrollTo() writes entirely until a genuine wheel/pointer/key
// event has armed it (cancelSettle), the same gate a reader's first touch
// clears. One dummy click clears it for the rest of the page's life.
async function arm(page, pos) { await page.mouse.move(...pos); await page.mouse.down(); await page.mouse.up(); }
// shown is 0 from load and stays 0 across the intro<->scene-0 boundary (the
// intro shares scene 1's film, so shown alone cannot signal arrival there),
// so every jump is read off scrollY against restY(), the one quantity that
// works for every target including the intro.
// review-phases-2-4.md, "Both flakes are racy by construction": scrollY
// reaching its target and shown/target/running settling are not the same
// moment as the carry spring actually coming to rest -- watchScrollDesktop's
// integrator can still be nudging scrollY toward its well for a few more
// frames after shown flips, which used to move a plate or card out from
// under a drag's own boundingBox() read. gesture.kind==='settling' (state().
// settle) is the page's own word for "still adjusting"; two consecutive
// scrollY reads a beat apart, both unmoved, is the direct check for the
// thing that actually matters (the drag's own read-then-click race), kept
// as a second condition rather than trusted alone since a slow settle could
// still be between ticks when sampled.
async function waitForScrollSettled(page, timeout = 15000) {
  const deadline = Date.now() + timeout;
  let lastY = await page.evaluate(() => scrollY);
  for (;;) {
    await page.waitForTimeout(60);
    const [y, settling] = await page.evaluate(() => [scrollY, window.__landing.state().settle]);
    if (!settling && Math.abs(y - lastY) < 0.5) return;
    if (Date.now() > deadline) throw new Error(`scroll never settled (y=${y}, settling=${settling}) after ${timeout}ms`);
    lastY = y;
  }
}
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
  await waitForScrollSettled(page, timeout);
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
// A hand that never lets go never commits (watchScrollDesktop tracks a held
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
async function gestureCommit(page, from, to, engineName, pos = gesturePos(engineName)) {
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
// check-landing-intent.mjs's only coverage this function didn't already
// have was these three locales and its own two viewports (1280 and the
// 1920 wide one specifically) -- folded in here as a loop rather than kept
// as a second script, since intent's own gesture shape (N one-pixel ticks
// at a fixed 35ms cadence, then a single fixed-goal assertion with no
// retry) is exactly what review-phases-2-4.md diagnosed as racy: the page
// classifies a held gesture by wall-clock gap (HOLD_GAP=100ms in site/
// landing.js), so under load a burst sent 35ms apart on paper can arrive
// more than 100ms apart in practice, splitting one intended gesture into
// several, each committing its own boundary. gestureCommit's retry-until-
// committed shape (below) does not have this failure mode: it does not
// care how many bursts the dead-zone formula actually saw, only whether
// the target it converged on is the one asked for.
const I_COMMIT_LOCALES = ['', 'zh-hans/', 'zh-hant/'];
const I_COMMIT_VIEWPORTS = [{ width: 1280, height: 720 }, { width: 1920, height: 1200 }];
async function iCommit(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const pos = gesturePos(engineName);
    // The full boundary walk, at the desktop default -- unchanged from
    // before the fold.
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
    // check-landing-intent.mjs's own scope, never the 3<->4 scrub: its six
    // goals were all within intro/0/1 (measured: [[1,0,1],[-1,-1,1],[1,0,3],
    // [1,1,3],[-1,0,3],[-1,-1,3]] in the deleted file, goals 0/-1/0/1/0/-1
    // only). First cut of this loop reused the outer pos (webkit's x=1400)
    // at every viewport and found a real, separate, deterministic problem
    // (not load noise, reproduced 10/10): 1280 is narrower than 1400, and
    // even though Playwright still dispatches a wheel there, it moved
    // scrollY only ~13px per 5 ticks against ~135px for every other
    // position tried at that width -- gesturePos now takes the viewport
    // width for exactly this reason.
    for (const locale of I_COMMIT_LOCALES) {
      for (const viewport of I_COMMIT_VIEWPORTS) {
        const p2 = await browser.newPage({ viewport });
        const pos2 = gesturePos(engineName, viewport.width);
        await ready(p2, locale);
        await arm(p2, pos2);
        for (const [from, to] of [[-1, 0], [0, 1]]) {
          await gotoScene(p2, from);
          await gestureCommit(p2, from, to, engineName, pos2);
          await gotoScene(p2, to);
          await gestureCommit(p2, to, from, engineName, pos2);
        }
        console.log(`${engineName}/${locale || 'en'} ${viewport.width}x${viewport.height}: I-commit intro/0/1 (intent's own region) commits forward and back`);
        await p2.close();
      }
    }
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
// Takes a locator, not coordinates: the caller used to read boundingBox()
// once, some turns of the event loop before the drag actually started,
// which is exactly the gap review-phases-2-4.md's diagnosis names -- a
// still-settling page moves the element out from under a box read that
// early. Waiting for scroll to settle and re-reading the box right here,
// immediately before the first mouse.move, is what "re-read the box after
// that wait" means; every caller gets it for free instead of having to
// remember the sequence. A second, independent race surfaced once this one
// closed: the plate pointerdown handler (site/landing.js) only arms a drag when
// shown===0 && !running() at the instant mouse.down() lands -- reading that
// once via waitForScrollSettled/gotoScene earlier is not the same guarantee,
// since a background join (a capture finishing, maybeJoin firing) can flip
// running() true again in the gap while this function reads a boundingBox()
// and moves the mouse into position. When that race lands, mouse.down()
// misses entirely: every subsequent pointermove is a no-op (`if (!held)
// return`), so the whole gesture silently drags nothing and the element
// never moves -- measured directly (repeat-fuzzinvalidated.mjs, unit0a): a
// 5-10% rate, the element left exactly at its pre-drag position. Retrying
// the mechanical gesture when the element didn't actually move is the same
// shape as gestureCommit's own retry-until-committed, not a retry on the
// assertion this exists to prove.
async function dragBy(page, locator, dx, dy, steps = 4) {
  await waitForScrollSettled(page);
  for (let attempt = 0; attempt < 3; attempt++) {
    const box = await locator.boundingBox();
    const sx = box.x + box.width / 2, sy = box.y + box.height / 2;
    await page.mouse.move(sx, sy);
    await page.mouse.down();
    await page.waitForTimeout(30);
    await page.mouse.move(sx + dx / 2, sy + dy / 2, { steps });
    await page.waitForTimeout(30);
    await page.mouse.move(sx + dx, sy + dy, { steps });
    await page.waitForTimeout(30);
    await page.mouse.up();
    await page.waitForTimeout(150);
    const after = await locator.boundingBox();
    if (after && (Math.abs(after.x - box.x) > 4 || Math.abs(after.y - box.y) > 4)) return;
  }
  throw new Error('dragBy: element never moved after 3 attempts (pointerdown kept missing its shown===0 && !running() window)');
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
      // plate ids embed a filename (a literal dot), so #id would read as a class selector
      const dx = (rand() - 0.5) * 1600, dy = (rand() - 0.5) * 1200;
      await dragBy(page, page.locator(`[id="${id}"]`), dx, dy);
      // Checked here, before the scene-3 card is ever touched: the card
      // drop's own call site is a second, later chance to union this plate
      // back in, which would hide a broken plate call site entirely.
      assertContained(await readPrintRect(page), await readPlateRects(page), engineName, `after dragging plate ${id}`);
    }
    await gotoScene(page, 2);
    await page.waitForFunction(() => document.getElementById('stage').classList.contains('s3-ready'), null, { timeout: 15000 });
    const cdx = (rand() - 0.5) * 1800, cdy = (rand() - 0.5) * 1400;
    await dragBy(page, page.locator('#sib-nb'), cdx, cdy, 8);
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
// -- because watchScrollDesktop keeps computing target from the live scroll
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
    // review-phases-2-4.md: sampling window.__landing.prints here caught a
    // transient -- retakeShown() (called unconditionally after every plate
    // drop) and the warmer refill the array asynchronously, so a read soon
    // after the drag could land mid-refill regardless of whether the rect
    // ever actually changed. printGeneration is the ground truth instead:
    // applyPrintRect bumps it exactly once, exactly when setPrintRect's own
    // sheets.fill(null) runs, so a before/after comparison across the drag
    // proves the invalidation path fired without caring what refills next.
    // Scattered plates can overlap (scatterPlates() has no seed): '.plate'
    // first() picks by DOM order, not by what a real pointerdown at its own
    // centre would actually hit. Measured directly, 1 in 8 fresh loads:
    // elementFromPoint at the first plate's own centre resolved to a
    // *different*, overlapping plate, so mouse.down() there dragged that
    // one instead -- silent when it happened to also cross outsideBase, a
    // real ~10-30% flake (3/10 in the required proof run) when it didn't.
    // Picking the one plate actually hit-testable at its own centre is what
    // a real drag gesture would find anyway.
    const plateId = await page.evaluate(() => {
      for (const pl of document.querySelectorAll('.plate')) {
        const r = pl.getBoundingClientRect();
        const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)?.closest('.plate');
        if (hit === pl) return pl.id;
      }
      return null;
    });
    assert(plateId, `I-fuzz-invalidated ${engineName}: no plate is hit-testable at its own centre (every plate fully covered)`);
    const genBefore = await page.evaluate(() => window.__landing.printGeneration());
    await dragBy(page, page.locator(`[id="${plateId}"]`), 1400, 900);
    const genAfter = await page.evaluate(() => window.__landing.printGeneration());
    assert(genAfter > genBefore, `I-fuzz-invalidated ${engineName}: printGeneration did not advance (${genBefore} -> ${genAfter}), so the drag did not actually clear held prints`);
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
    return { id: pl.id, boxAspect: box.w / box.h, natAspect: nat.w / nat.h };
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
    }
    console.log(`${engineName}: I-plate all ${rows.length} plates render at their image's natural aspect (<=1% off)`);
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

// I-header-scrim (site owner, 2026-09-20; rewritten unit0a for the owner's
// actual complaint: "the current dark background behind site name and
// language picker in scene 5 on mobile is ugly; it should use at least a
// band as the first 4 scene"). The previous shape gave .brand and
// .language-picker each their own rgba(8,10,9,...) box -- two separate dark
// boxes, exactly what read as ugly. Fixed (site/index.html) by deleting
// that rule and letting the ONE shared header band (body::before, already
// used by scenes 1-4) carry colour through scene 5 too, interpolating from
// --bg to the closing film's own #080a09 as --xf rises instead of fading
// the band's own opacity to zero -- same geometry, same solid-then-fade
// gradient shape, so scene 5's header reads as the same band, only dark.
// This invariant asserts that requirement directly: no background of
// .brand/.language-picker's own, the band's own box identical between
// scene 1 and scene 5 (no new element, no layout shift), and contrast
// against the band's own colour at rest (xf=1) -- the band is opaque for
// its first 34px, where the header sits, so there is no translucent frame
// behind it left to composite against the way the old two-box rule needed.
// RED (site/index.html's .brand,.language-picker background rule restored,
// body::before's colour interpolation reverted to the old opacity fade):
// the "no background of its own" assertion fails immediately -- .brand
// carries its own rgba(8,10,9,...) box again. GREEN restored.
//
// Fixed (a follow-up, same day): a visitor can stop the scroll anywhere in
// the crossfade, so the dip found first -- sampling contrast through the
// actual crossfade via a real slow touch-scroll (CDP, Chromium only --
// Playwright cannot synthesize this in mobile WebKit, same limit check-
// landing-mobile.mjs already lives with) showed a genuine collapse to
// ~1.1:1 near xf=0.5 -- was a real defect, not inherent. Root cause: the
// header text colour and the band colour were both linear interpolations
// through the same grey gamut in opposite directions (text: black to
// white; band: near-white to near-black), crossing paths near the
// midpoint by construction, not from a wiring bug -- no non-linear easing
// of that same blend avoids it either (checked: even a steep linear ramp
// between the two colour formulas still touches ~1:1 partway through its
// own ramp, for the identical reason). site/index.html's fix instead
// flips text between the two existing formulas with a near-instant step
// (--hdr, a clamp() ramp 0.5% of the crossfade wide, so a real touch-
// scroll never renders inside it) at xf=0.548, the crossover where
// solving contrast(black, band) = contrast(white, band) lands -- both
// sides 4.58:1 there, this function's own floor below. RED (site/
// index.html's --hdr definition and every rule using it reverted to
// var(--xf) directly): minimum crossfade contrast collapses back to
// ~1:1. GREEN restored, minimum measured at 4.68:1.
function srgbToLinear(c) { c /= 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; }
function relativeLuminance([r, g, b]) { return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b); }
function contrastRatio(rgb1, rgb2) { const [a, b] = [relativeLuminance(rgb1), relativeLuminance(rgb2)].sort((x, y) => y - x); return (a + 0.05) / (b + 0.05); }
function parseRGBA(str) { const m = str.match(/[\d.]+/g).map(Number); return { r: m[0], g: m[1], b: m[2], a: m[3] ?? 1 }; }
function readBandRgb(bgImageStr) {
  const m = bgImageStr.match(/rgb\(([\d.]+),\s*([\d.]+),\s*([\d.]+)\)/);
  return m ? [1, 2, 3].map((i) => Number(m[i])) : null;
}
async function iHeaderScrim(browsers) {
  for (const [engineName, browser] of Object.entries(browsers)) {
    const page = await browser.newPage(PRESETS.phone);
    await ready(page);
    const geomAt = () => page.evaluate(() => {
      const cs = getComputedStyle(document.body, '::before');
      return { top: cs.top, left: cs.left, right: cs.right, width: cs.width, height: cs.height, zIndex: cs.zIndex, display: cs.display };
    });
    const geomScene1 = await geomAt();
    await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    await page.waitForFunction(() => window.__landing.state().xf === 1 && document.getElementById('five').classList.contains('on'), null, { timeout: 20000 });
    const geomScene5 = await geomAt();
    assert(JSON.stringify(geomScene1) === JSON.stringify(geomScene5), `I-header-scrim ${engineName}: header band's own box differs between scene 1 (${JSON.stringify(geomScene1)}) and scene 5 (${JSON.stringify(geomScene5)})`);
    const style = await page.evaluate(() => ({
      brandBg: getComputedStyle(document.querySelector('.brand')).backgroundColor,
      brandFg: getComputedStyle(document.querySelector('.brand .moss-wordmark')).color,
      langBg: getComputedStyle(document.querySelector('.language-picker')).backgroundColor,
      langFg: getComputedStyle(document.querySelector('.language-picker .nav-lang-current')).color,
      bandImage: getComputedStyle(document.body, '::before').backgroundImage,
    }));
    for (const [label, bgStr] of [['brand', style.brandBg], ['language-picker', style.langBg]]) {
      assert(parseRGBA(bgStr).a === 0, `I-header-scrim ${engineName}: .${label} has a background of its own (${bgStr}) -- one shared band was the ask, not per-element boxes`);
    }
    const bandRgb = readBandRgb(style.bandImage);
    assert(bandRgb, `I-header-scrim ${engineName}: could not read the shared band's own colour from body::before`);
    for (const [label, fgStr] of [['brand', style.brandFg], ['language-picker', style.langFg]]) {
      const fg = parseRGBA(fgStr);
      const cr = contrastRatio([fg.r, fg.g, fg.b], bandRgb);
      assert(cr >= 4.5, `I-header-scrim ${engineName}: .${label} contrast ${cr.toFixed(2)} < 4.5 against the band's own colour (fg ${fgStr}, band ${JSON.stringify(bandRgb)})`);
      console.log(`${engineName}: I-header-scrim .${label} ${cr.toFixed(2)}:1 against the shared band, at rest (xf=1)`);
    }
    await page.close();
  }
  // Chromium only: mobile WebKit cannot be driven by synthesized touch
  // events via Playwright/CDP (check-landing-mobile.mjs's own limit).
  const page = await browsers.chromium.newPage(PRESETS.phone);
  await ready(page);
  const y3 = await page.evaluate(() => window.__landing.restY(3));
  await page.evaluate((y) => scrollTo(0, y), y3);
  await page.waitForFunction((s) => window.__landing.state().shown === s && !window.__landing.state().running, 3, { timeout: 20000 });
  const cdp = await page.context().newCDPSession(page);
  const samples = [];
  let touchY = 700;
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: 195, y: touchY }] });
  for (let i = 0; i < 220; i++) {
    touchY -= 6;
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: 195, y: touchY }] });
    await page.waitForTimeout(15);
    const raw = await page.evaluate(() => ({ xf: window.__landing.state().xf, bandImage: getComputedStyle(document.body, '::before').backgroundImage, brandFg: getComputedStyle(document.querySelector('.brand .moss-wordmark')).color }));
    const bandRgb = readBandRgb(raw.bandImage);
    if (!bandRgb) continue;
    const fg = parseRGBA(raw.brandFg);
    samples.push({ xf: raw.xf, cr: contrastRatio([fg.r, fg.g, fg.b], bandRgb) });
  }
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  assert(samples.length > 20, `I-header-scrim chromium: too few crossfade samples (${samples.length}) to judge the mid-transit minimum`);
  for (const target of [0, 0.25, 0.5, 0.75, 1]) {
    const nearest = samples.reduce((best, s) => Math.abs(s.xf - target) < Math.abs(best.xf - target) ? s : best);
    console.log(`I-header-scrim chromium: xf~=${target} (actual ${nearest.xf.toFixed(2)}): contrast ${nearest.cr.toFixed(2)}:1`);
  }
  const min = Math.min(...samples.map((s) => s.cr));
  const at = samples.find((s) => s.cr === min);
  console.log(`I-header-scrim chromium: MINIMUM contrast through the crossfade is ${min.toFixed(2)}:1 at xf=${at.xf.toFixed(2)} (${samples.length} samples, real touch-scroll)`);
  // A visitor can stop scrolling anywhere in the crossfade, so this is a
  // real assertion now, not just a reported finding -- site/index.html's
  // --hdr flip (this function's header comment has the derivation) is what
  // makes 4.5 reachable at all; a continuous linear blend between the two
  // text-colour formulas cannot clear it (measured: ~1:1 at the old
  // build's own crossover, unavoidable by construction -- both curves pass
  // through the same grey at the same moment).
  assert(min >= 4.5, `I-header-scrim chromium: minimum crossfade contrast ${min.toFixed(2)} < 4.5 at xf=${at.xf.toFixed(2)} (${samples.length} samples)`);
  await page.close();
}

// I-default: every check script's own default navigation loads the
// unflagged URL. A carry= baked into a literal .goto() call site was exactly
// the historical bug (design doc: "the fixed driver was never made the
// default URL and no test loaded the unflagged page"). review-phases-2-4.md:
// the ?carry= flag itself is gone as of phase 4, so grepping for that one
// literal no longer catches "the same shape of mistake under a different
// name" -- generalized to any query string inside a quoted literal on a
// .goto() line. A script that legitimately needs one is named in ALLOWLIST,
// the one sanctioned opt-out; none does today.
const I_DEFAULT_ALLOWLIST = new Set();
async function iDefault() {
  const entries = await readdir(HERE);
  const files = entries.filter((f) => /^check-(landing|site-preview|docs|favicon)/.test(f) && f.endsWith('.mjs') && f !== 'check-landing-invariants.mjs' && f !== 'check-landing-all.mjs' && f !== 'check-landing-structure.mjs' && !I_DEFAULT_ALLOWLIST.has(f));
  const offenders = [];
  for (const file of files) {
    const src = await readFile(`${HERE}${file}`, 'utf8');
    for (const line of src.split('\n')) {
      if (!line.includes('.goto(')) continue;
      // A query string inside a quoted literal on this line: `?key=`
      // between matching quote characters, not merely a `?` anywhere on the
      // line (a URL constructor call's own punctuation, or a `?.` optional
      // chain, could contain one without this being a flagged default).
      if (/['"`][^'"`]*\?[^'"`=]*=[^'"`]*['"`]/.test(line)) offenders.push(`${file}: ${line.trim()}`);
    }
  }
  assert(offenders.length === 0, `I-default: a script's own .goto() call bakes in a query string: ${offenders.join('; ')}`);
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
