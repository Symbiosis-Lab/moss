#!/usr/bin/env node
// Node-only unit tests for the pure scroll gesture model in
// site/landing-gesture.js -- table-driven sequences of timestamped events,
// no browser, no DOM, no server. Run: node scripts/gesture-model.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const { createLandingGesture } = createRequire(import.meta.url)('../site/landing-gesture.js');

// The model itself carries no HOLD_GAP -- that constant, and the decision
// of when a wheel tick counts as a push vs. a momentum tick, stay in site/
// landing.js's wheel event handler, outside what moved. These helpers mimic
// the exact sequences that handler and the settle spring run against the
// model's own primitives, so a test reads like the real caller discipline
// rather than reaching into private state.
const HOLD_GAP = 100;
// Mirrors the wheel handler's push branch (site/landing.js): cancel
// whatever run is in flight, then extend the hold deadline.
function wheelPush(model, now) { model.cancelRun(); model.contact.wheelUntil = now + HOLD_GAP; }
// Mirrors its momentum branch: the deadline drops to now instead of lapsing
// on its own, so a classified coast tick can't leave a stale window open.
function wheelCoast(model) { model.contact.wheelUntil = 0; }
// Mirrors settleAtRest's one call site (site/landing.js): consuming a rest
// and starting the run that carries to it happen together, gated on the
// model reporting 'coasting' -- never on a held or already-settling model.
function fire(model, now) {
  if (model.gestureKind(now) !== 'coasting') return null;
  model.restOwed = false;
  const token = { cancel: false };
  model.run = token;
  return token;
}
// Mirrors settleTo's own convergence write (site/landing.js): clears run
// only if it is still the same run this token started.
function convergeRun(model, token) { if (model.run === token) model.run = null; }

test('idle at rest with no contact', () => {
  const m = createLandingGesture();
  assert.equal(m.gestureKind(0), 'idle');
});

test('a wheel push reads held while the deadline has not lapsed', () => {
  const m = createLandingGesture();
  wheelPush(m, 0);
  assert.equal(m.gestureKind(50), 'held');
  assert.equal(m.gestureKind(99), 'held');
});

test('a hold expires when wheel ticks stop, in every drive role', () => {
  // "Every drive role" because the model has no notion of one: watchScrollDesktop,
  // watchScrollNative and watchScrollReduced all call tickGesture(now) from the
  // one shared watchScroll loop before dispatching to any of them (site/
  // landing.js), so a hold expiring is just tickGesture seeing a `now` past
  // the deadline -- nothing here depends on who is asking.
  const m = createLandingGesture();
  wheelPush(m, 0);
  m.tickGesture(10);                        // observed held once
  assert.equal(m.gestureKind(10), 'held');
  m.tickGesture(10 + HOLD_GAP + 1);         // observed not-held: the release edge
  assert.equal(m.gestureKind(10 + HOLD_GAP + 1), 'coasting');
  assert.equal(m.restOwed, true);
  assert.equal(m.gesture.reason, 'release');
});

test('a wheel momentum tick during a touch hold does not drop the hold', () => {
  const m = createLandingGesture();
  m.holdDirect();                           // touch/scrollbar hold begins
  m.tickGesture(0);
  assert.equal(m.gestureKind(0), 'held');
  // A coast tick from the wheel channel touches only contact.wheelUntil,
  // never contact.direct -- the fault this guards (M4) was a shared scalar
  // `via` that let one channel overwrite what the other meant.
  wheelCoast(m);
  m.tickGesture(5);
  assert.equal(m.gestureKind(5), 'held');
  assert.equal(m.restOwed, false);
  m.holdOff();
  m.tickGesture(10);
  assert.equal(m.gestureKind(10), 'coasting');
  assert.equal(m.restOwed, true);
});

test('a converged settle returns to idle', () => {
  const m = createLandingGesture();
  const token = { cancel: false };
  m.run = token;                            // settleTo's own opening write
  assert.equal(m.gestureKind(0), 'settling');
  convergeRun(m, token);                    // settleTo's own convergence write
  assert.equal(m.gestureKind(0), 'idle');
});

test('an edge that opens and closes between two ticks still records a rest owed', () => {
  // tickGesture's own edge detection depends only on contact.wasHeld, the
  // value IT wrote on its own last call -- never on what some other reader
  // (gestureKind, called by state() or similar with its own `now`) saw in
  // between. This drives a hold open (observed by one tickGesture call),
  // lets an unrelated gestureKind() read happen after the deadline has
  // already lapsed -- exactly what a production caller like landing.state()
  // does on some other frame -- and proves the NEXT tickGesture call still
  // fires the release edge, undisturbed by that intervening read.
  const m = createLandingGesture();
  wheelPush(m, 0);
  m.tickGesture(10);                        // open, observed
  assert.equal(m.gestureKind(10), 'held');
  assert.equal(m.gestureKind(10 + HOLD_GAP + 50), 'idle');   // an intervening read, no tick
  assert.equal(m.restOwed, false);          // that read alone records nothing
  m.tickGesture(10 + HOLD_GAP + 60);        // close, observed -- the real edge
  assert.equal(m.restOwed, true);
  assert.equal(m.gesture.reason, 'release');
});

test('settle start: armSettle arms a rest only when nothing is held or running', () => {
  const m = createLandingGesture();
  m.armSettle('scroll');
  assert.equal(m.restOwed, true);
  assert.equal(m.gesture.reason, 'scroll');
});

test('settle start: armSettle is a no-op while held', () => {
  const m = createLandingGesture();
  m.holdDirect();
  m.armSettle('scroll');
  assert.equal(m.restOwed, false);
});

test('settle start: armSettle is a no-op while a run is in flight', () => {
  const m = createLandingGesture();
  m.run = { cancel: false };
  m.armSettle('scroll');
  assert.equal(m.restOwed, false);
});

test('a rest owed is cleared only where it is consumed (fire)', () => {
  const m = createLandingGesture();
  m.armSettle('scroll');
  assert.equal(m.restOwed, true);
  const token = fire(m, 0);
  assert.ok(token, 'fire should have consumed the owed rest and started a run');
  assert.equal(m.restOwed, false);
});

function mulberry32(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6D2B79F5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

test('fuzz: never held and settling at once; restOwed set only by a release edge or an arm, cleared only by fire', () => {
  const rand = mulberry32(20260921);
  const m = createLandingGesture();
  let now = 0, runToken = null;
  const actions = ['tick', 'wheelPush', 'wheelCoast', 'holdDirect', 'holdOff', 'armSettle', 'fire', 'convergeRun'];
  for (let step = 0; step < 5000; step++) {
    now += Math.floor(rand() * 40);
    const action = actions[Math.floor(rand() * actions.length)];
    const prevRestOwed = m.restOwed;
    const prevWasHeld = m.contact.wasHeld;
    let firedRun = false;
    switch (action) {
      case 'tick': m.tickGesture(now); break;
      case 'wheelPush': wheelPush(m, now); break;
      case 'wheelCoast': wheelCoast(m); break;
      case 'holdDirect': m.holdDirect(); break;
      case 'holdOff': m.holdOff(); break;
      case 'armSettle': m.armSettle('fuzz'); break;
      case 'fire': { const t = fire(m, now); if (t) { runToken = t; firedRun = true; } break; }
      case 'convergeRun': convergeRun(m, runToken); break;
    }
    // Invariant: never held and settling at once. holdDirect/wheelPush/fire
    // each cancel any run or refuse to fire while held (their own transition
    // rules), so this must hold after every step, not just by construction
    // of gestureKind's own ternary.
    assert.ok(!(m.gestureHeld(now) && m.run), `step ${step} (${action}): held and settling at once (contact=${JSON.stringify(m.contact)}, run=${JSON.stringify(m.run)})`);
    // Invariant: restOwed only ever turns on via a real release edge
    // (tickGesture, with the hold observed open on a prior call) or armSettle.
    if (!prevRestOwed && m.restOwed) {
      const releaseEdge = action === 'tick' && prevWasHeld && !m.gestureHeld(now);
      assert.ok(releaseEdge || action === 'armSettle', `step ${step} (${action}): restOwed set true outside a release edge or an arm`);
    }
    // Invariant: restOwed only ever turns off at its one consumption site.
    if (prevRestOwed && !m.restOwed) {
      assert.ok(action === 'fire' && firedRun, `step ${step} (${action}): restOwed cleared outside fire()`);
    }
  }
});
