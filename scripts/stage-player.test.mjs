// Unit tests for site/ui/stage/player.js, against a fake driver (no browser, no iframe), plus the
// two pure Pace formulas from site/ui/stage/driver.js (stage/README.md, "Pace") — they take no DOM
// and no timers, so they belong in the same no-browser suite rather than a browser-only check.
// Run: node --test scripts/stage-player.test.mjs

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { play, playChain, resolveLocale } from '../site/ui/stage/player.js';
import { travelDurationMs, holdAfterResultMs } from '../site/ui/stage/driver.js';

// A driver whose animated verbs resolve true on the next microtask, as if each finished instantly.
function fastDriver() {
  const calls = [];
  return {
    calls,
    async click(name, { signal } = {}) {
      calls.push(['click', name]);
      await Promise.resolve();
      return !signal?.aborted;
    },
    clickInstant(name) {
      calls.push(['clickInstant', name]);
    },
    async context(name, { signal } = {}) {
      calls.push(['context', name]);
      await Promise.resolve();
      return !signal?.aborted;
    },
    contextInstant(name) {
      calls.push(['contextInstant', name]);
    },
    async dblclick(name, { signal } = {}) {
      calls.push(['dblclick', name]);
      await Promise.resolve();
      return !signal?.aborted;
    },
    dblclickInstant(name) {
      calls.push(['dblclickInstant', name]);
    },
  };
}

// A driver whose `click()` never resolves on its own — only when the caller aborts. Used to prove
// that an abort mid-step stops the scene rather than letting it silently finish.
function hangingDriver() {
  const calls = [];
  return {
    calls,
    click(name, { signal }) {
      calls.push(['click', name]);
      return new Promise((resolve) => {
        signal?.addEventListener('abort', () => resolve(false), { once: true });
      });
    },
    clickInstant(name) {
      calls.push(['clickInstant', name]);
    },
  };
}

test('play runs steps in order and ends done', async () => {
  const driver = fastDriver();
  const scene = { steps: [{ click: 'tree.breadcrumb' }, { context: 'versions.currentFile' }] };
  const outcome = await play(scene, driver, { locale: 'en' });
  assert.equal(outcome, 'done');
  assert.deepEqual(driver.calls, [['click', 'tree.breadcrumb'], ['context', 'versions.currentFile']]);
});

test('pause mid-click cancels and later steps do not run', async () => {
  const driver = hangingDriver();
  const scene = { steps: [{ click: 'first' }, { click: 'second' }] };
  const controller = new AbortController();
  const outcomePromise = play(scene, driver, { locale: 'en', signal: controller.signal });
  controller.abort();
  const outcome = await outcomePromise;
  assert.equal(outcome, 'paused');
  assert.deepEqual(driver.calls, [['click', 'first']]);
});

test('instant mode calls only the instant variants, never the animated ones', async () => {
  const driver = fastDriver();
  const scene = { steps: [{ click: 'tree.breadcrumb' }, { context: 'versions.currentFile' }] };
  const outcome = await play(scene, driver, { locale: 'en', instant: true });
  assert.equal(outcome, 'done');
  assert.deepEqual(driver.calls, [['clickInstant', 'tree.breadcrumb'], ['contextInstant', 'versions.currentFile']]);
  assert.ok(!driver.calls.some(([verb]) => verb === 'click' || verb === 'context'));
});

test('play runs a context (right-click) step like click, animated and instant', async () => {
  const driver = fastDriver();
  const scene = { steps: [{ context: 'versions.currentFile' }] };
  const outcome = await play(scene, driver, { locale: 'en' });
  assert.equal(outcome, 'done');
  assert.deepEqual(driver.calls, [['context', 'versions.currentFile']]);

  const instantDriver = fastDriver();
  const instantOutcome = await play(scene, instantDriver, { locale: 'en', instant: true });
  assert.equal(instantOutcome, 'done');
  assert.deepEqual(instantDriver.calls, [['contextInstant', 'versions.currentFile']]);
});

test('play runs a dblclick step like click, animated and instant', async () => {
  const driver = fastDriver();
  const scene = { steps: [{ dblclick: 'tree.divider' }] };
  const outcome = await play(scene, driver, { locale: 'en' });
  assert.equal(outcome, 'done');
  assert.deepEqual(driver.calls, [['dblclick', 'tree.divider']]);

  const instantDriver = fastDriver();
  const instantOutcome = await play(scene, instantDriver, { locale: 'en', instant: true });
  assert.equal(instantOutcome, 'done');
  assert.deepEqual(instantDriver.calls, [['dblclickInstant', 'tree.divider']]);
});

test('playChain runs every prerequisite instant and only the target animated', async () => {
  const driver = fastDriver();
  const chain = [
    { name: 'open-versions', scene: { steps: [{ context: 'versions.currentFile' }] } },
    { name: 'save-version', scene: { steps: [{ click: 'versions.saveToggle' }] } },
    { name: 'restore-version', scene: { steps: [{ click: 'versions.firstRow' }, { click: 'versions.restore' }] } },
  ];
  const outcome = await playChain(chain, driver, { locale: 'en' });
  assert.equal(outcome, 'done');
  assert.deepEqual(driver.calls, [
    ['contextInstant', 'versions.currentFile'],
    ['clickInstant', 'versions.saveToggle'],
    ['click', 'versions.firstRow'],
    ['click', 'versions.restore'],
  ]);
});

test('playChain plays the target instantly too under reduced motion', async () => {
  const driver = fastDriver();
  const chain = [
    { name: 'open-versions', scene: { steps: [{ context: 'versions.currentFile' }] } },
    { name: 'restore-version', scene: { steps: [{ click: 'versions.restore' }] } },
  ];
  const outcome = await playChain(chain, driver, { locale: 'en', instant: true });
  assert.equal(outcome, 'done');
  assert.deepEqual(driver.calls, [['contextInstant', 'versions.currentFile'], ['clickInstant', 'versions.restore']]);
});

test('playChain with no prerequisites behaves like play() alone', async () => {
  const driver = fastDriver();
  const chain = [{ name: 'tree', scene: { steps: [{ click: 'tree.breadcrumb' }] } }];
  const outcome = await playChain(chain, driver, { locale: 'en' });
  assert.equal(outcome, 'done');
  assert.deepEqual(driver.calls, [['click', 'tree.breadcrumb']]);
});

test('playChain rejects a chain that repeats a scene name (a cycle)', async () => {
  const driver = fastDriver();
  const chain = [
    { name: 'save-version', scene: { steps: [] } },
    { name: 'open-versions', scene: { steps: [] } },
    { name: 'save-version', scene: { steps: [] } },
  ];
  await assert.rejects(() => playChain(chain, driver, { locale: 'en' }), /cycle/);
});

test('a per-locale string resolves by locale with en fallback', () => {
  const value = { en: 'Hello', 'zh-hant': '你好' };
  assert.equal(resolveLocale(value, 'zh-hant'), '你好');
  assert.equal(resolveLocale(value, 'zh-hans'), 'Hello');
  assert.equal(resolveLocale('plain string', 'zh-hant'), 'plain string');
});

test('a per-locale string with neither the locale nor en falls back to empty', () => {
  const value = { 'zh-hans': '你好' };
  assert.equal(resolveLocale(value, 'zh-hant'), '');
});

test('an unknown verb fails loudly', async () => {
  const driver = fastDriver();
  const scene = { steps: [{ teleport: 'nowhere' }] };
  await assert.rejects(() => play(scene, driver, { locale: 'en' }), /Unknown scene verb/);
});

test('a step with zero or two verbs is rejected', async () => {
  const driver = fastDriver();
  await assert.rejects(
    () => play({ steps: [{}] }, driver, { locale: 'en' }),
    /exactly one verb/,
  );
  await assert.rejects(
    () => play({ steps: [{ click: 'a', context: 'b' }] }, driver, { locale: 'en' }),
    /exactly one verb/,
  );
});

test('a step\'s "reveals" rides alongside its one verb without counting as a second one', async () => {
  const driver = fastDriver();
  const scene = { steps: [{ context: 'versions.currentFile', reveals: 10 }] };
  const outcome = await play(scene, driver, { locale: 'en' });
  assert.equal(outcome, 'done');
  assert.deepEqual(driver.calls, [['context', 'versions.currentFile']]);
});

test('play forwards a step\'s "reveals" to the driver untouched (stage/README.md, "Pace")', async () => {
  const seen = [];
  const driver = {
    async click(name, opts) { seen.push(['click', name, opts.reveals]); return true; },
    async context(name, opts) { seen.push(['context', name, opts.reveals]); return true; },
    async dblclick(name, opts) { seen.push(['dblclick', name, opts.reveals]); return true; },
    clickInstant() {},
    contextInstant() {},
    dblclickInstant() {},
  };
  const scene = { steps: [{ click: 'a', reveals: 7 }, { context: 'b' }, { dblclick: 'c', reveals: 1 }] };
  await play(scene, driver, { locale: 'en' });
  assert.deepEqual(seen, [['click', 'a', 7], ['context', 'b', undefined], ['dblclick', 'c', 1]]);
});

test('travelDurationMs clamps to [500, 1000]ms and scales with distance between', () => {
  assert.equal(travelDurationMs(0), 500, 'below the floor (450ms base) clamps up to the 500ms minimum');
  assert.equal(travelDurationMs(100), 500, 'the boundary where the raw formula first reaches 500ms');
  assert.equal(travelDurationMs(300), 600, '450 + 0.5*300 = 600, inside the clamp');
  assert.equal(travelDurationMs(1100), 1000, '450 + 0.5*1100 = 1000, the boundary at the cap');
  assert.equal(travelDurationMs(5000), 1000, 'far past the boundary still clamps to the 1000ms maximum');
});

test('holdAfterResultMs caps reveals at 5 and defaults to 2 when omitted', () => {
  assert.equal(holdAfterResultMs(0), 400, '0 reveals: just the base hold');
  assert.equal(holdAfterResultMs(1), 600);
  assert.equal(holdAfterResultMs(5), 1400, 'the boundary at the cap');
  assert.equal(holdAfterResultMs(6), 1400, 'past the cap holds no longer than exactly at it');
  assert.equal(holdAfterResultMs(19), 1400, 'a real count far past the cap (properties.add) is still just the cap');
  assert.equal(holdAfterResultMs(undefined), holdAfterResultMs(2), 'omitted reveals defaults to 2');
});
