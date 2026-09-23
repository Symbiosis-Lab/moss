#!/usr/bin/env node
// Verifies the <moss-editor-demo>/<moss-demo-marker> pages (site/ui/demo) in a real browser, in
// both Chromium and WebKit. Usage:
//   PLAYWRIGHT_MODULE=/path/to/node_modules/playwright/index.mjs node scripts/check-docs-demo.mjs <preview-url>

import { loadPlaywright } from './site-check-harness.mjs';
import { travelDurationMs, holdAfterResultMs } from '../site/ui/demo/driver.js';
import { findMarkdownFiles, findSceneLinks, SITE_DIR } from './check-demo-scenes.mjs';

const input = process.argv[2];
if (!input) {
  console.error('Usage: node scripts/check-docs-demo.mjs <preview-url>');
  process.exit(2);
}
const base = new URL(input);
const playwright = await loadPlaywright();

const assert = (condition, message) => { if (!condition) throw new Error(message); };

// This checker's own policy — which built URLs to exercise in a real browser, at which source
// file — locale/viewport, not the walk itself. zh-hans is skipped here to keep the per-engine run
// short: scene names are locale-independent (site/ui/demo/README.md), so en and zh-hant already
// exercise the same scenes zh-hans would.
const CHECKED_PAGES = [
  { path: '/get-started/editor/', source: 'site/Get Started/Meet the editor.md' },
  { path: '/zh-hant/開始使用/editor/', source: 'site/zh-hant/開始使用/認識編輯器.md' },
  { path: '/get-started/', source: 'site/Get Started/Get Started.md' },
  { path: '/zh-hant/開始使用/', source: 'site/zh-hant/開始使用/開始使用.md' },
];

// The markers each page carries (in source order) come from the same page→scene-link walk
// check-demo-scenes.mjs uses, so this list can't drift from what the Markdown actually links.
async function loadDemoPages() {
  const files = await findMarkdownFiles(SITE_DIR);
  const links = await findSceneLinks(files);
  const markersBySource = new Map();
  for (const { page, name } of links) {
    if (name === '') continue; // "just open the surface" — no marker click to script here
    if (!markersBySource.has(page)) markersBySource.set(page, []);
    markersBySource.get(page).push(name);
  }
  return CHECKED_PAGES.map(({ path, source }) => {
    const markers = markersBySource.get(source);
    assert(markers?.length > 0, `check-docs-demo: no scene links found for ${source} (CHECKED_PAGES source path stale?)`);
    return { path, markers };
  });
}
const DEMO_PAGES = await loadDemoPages();

const sceneCache = new Map();
async function sceneData(name) {
  let cached = sceneCache.get(name);
  if (!cached) {
    cached = fetch(new URL(`ui/demo/scenes/${name}.json`, base)).then((res) => res.json());
    sceneCache.set(name, cached);
  }
  return cached;
}

// A generous upper bound on how long a scene can take to reach 'done' (site/ui/demo/README.md, "Pace"):
// each of its OWN steps at the slowest travel/dwell/hold the pace table allows, plus a flat
// allowance for typing and for fetch/settle latency. An "after" chain's ancestors play instantly
// (player.js's playChain), so they cost fetch latency only, already covered by the flat slack.
const DWELL_MAX_MS = 600; // site/ui/demo/README.md, "Pace": a right-click's dwell, the longer of the two
const PRESS_MS = 150;
async function paceBoundMs(name) {
  const scene = await sceneData(name);
  let total = 15000; // fetch latency, "after"-chain resolution, and a fresh iframe reload of the
  // ~1.7MB harvested document + its own editor-ready bootstrap before ANY step plays (every Play
  // is a fresh load — site/ui/demo/README.md) — slower in a cold headless browser than the README's own
  // "about a fifth of a second" figure for a warm one, and slower again under WebKit.
  for (const step of scene.steps) {
    if (step.typeInto) { total += 2500; continue; }
    total += travelDurationMs(2000) + DWELL_MAX_MS + PRESS_MS + holdAfterResultMs(step.reveals);
  }
  return total;
}

// Navigates and waits for the demo frame's iframe to exist — nothing more. The frame starts
// loading the default fixture as soon as it connects (see site/ui/demo/README.md, "the frame is
// never empty"), but that load is asynchronous, so this helper still returns before it settles.
// Callers that need editor content ready wait for that themselves, via waitEditorReady.
async function gotoDemo(browser, path, viewport) {
  const page = await browser.newPage({ viewport });
  await page.goto(new URL(path, base).href, { waitUntil: 'load' });
  await page.waitForSelector('moss-editor-demo iframe');
  return page;
}

// Pairs with gotoDemo for the common shape every check below follows: open a page at a
// path and viewport, run assertions against it, close it — even if an assertion throws.
async function withPage(browser, path, viewport, fn) {
  const page = await gotoDemo(browser, path, viewport);
  try {
    await fn(page);
  } finally {
    await page.close();
  }
}

// For a page that carries no <moss-editor-demo> at all (the divider-width check's control case).
async function withPlainPage(browser, path, viewport, fn) {
  const page = await browser.newPage({ viewport });
  await page.goto(new URL(path, base).href, { waitUntil: 'load' });
  try {
    await fn(page);
  } finally {
    await page.close();
  }
}

async function waitEditorReady(page, timeout = 15000) {
  await page.waitForFunction(() => {
    const frame = document.querySelector('moss-editor-demo iframe');
    return Boolean(frame?.contentWindow?.__editor) && frame.contentDocument?.documentElement.dataset.ready === '1';
  }, null, { timeout });
}

const editorText = (page) => page.evaluate(() => document.querySelector('moss-editor-demo iframe').contentWindow.__editor.text);

// A marker's button holds an aria-hidden glyph (▶/■) plus a `.moss-demo-marker__verb-text` span
// carrying the bare "Play"/"Stop" (moss-demo-marker.js), visually hidden but readable off that class.
// The `verb ?? button` fallback is defensive only; every marker has the span today. Every check
// below that cares about Play/Stop reuses this instead of each reimplementing it.
const markerState = (page, name) => page.evaluate((n) => {
  const marker = document.querySelector(`moss-demo-marker[name="${n}"]`);
  const button = marker.querySelector('button');
  const verb = button.querySelector('.moss-demo-marker__verb-text');
  return {
    button: (verb ?? button).textContent,
    status: marker.querySelector('.moss-demo-marker__status').textContent,
  };
}, name);

async function clickMarker(page, name) {
  await page.evaluate((n) => {
    document.querySelector(`moss-demo-marker[name="${n}"]`).scrollIntoView({ block: 'center' });
  }, name);
  await page.waitForTimeout(150);
  await page.evaluate((n) => {
    document.querySelector(`moss-demo-marker[name="${n}"] button`).click();
  }, name);
}

async function pressKeyInEditor(page) {
  const frame = page.frameLocator('moss-editor-demo iframe');
  await frame.locator('.cm-content').click();
  await page.keyboard.type('x');
}

// a. At rest (no scroll, no click) at 1440px: exactly one iframe, the editor is ready, nothing is
// playing (every marker reads Play), and the demo frame itself contains no button — the bar is gone;
// the text is both the list of scenes and the way to start them (site/ui/demo/README.md).
async function checkRestState(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    const counts = await page.evaluate(() => ({
      frames: document.querySelectorAll('moss-editor-demo').length,
      iframes: document.querySelectorAll('moss-editor-demo iframe').length,
      frameButtons: document.querySelectorAll('moss-editor-demo button').length,
    }));
    assert(counts.frames === 1 && counts.iframes === 1, `[${engine}] expected exactly one moss-editor-demo/iframe, got ${JSON.stringify(counts)}`);
    assert(counts.frameButtons === 0, `[${engine}] the demo frame itself carries a button; it must have no controls of its own: ${counts.frameButtons}`);

    const tree = await markerState(page, 'tree');
    const properties = await markerState(page, 'properties');
    assert(tree.button === 'Play' && properties.button === 'Play', `[${engine}] a marker reads something other than Play at rest: ${JSON.stringify({ tree, properties })}`);
  });
}

// d. A key press inside the editor during playback stops it, the text is kept, and the button
// returns to Play. "tree" is a two-click scene — its first step's own 500-1000ms pointer glide
// alone (driver.js's PACE.travel*, site/ui/demo/README.md "Pace") is already long enough to interrupt
// reliably without slowing anything down or adding a production knob; 200ms lands the keypress
// mid-glide with margin either side, before the dwell/press/hold beats that follow it even start.
async function checkInterruptStopsPlayback(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    await clickMarker(page, 'tree');
    await page.waitForFunction(() => document.querySelector('moss-demo-marker[name="tree"] .moss-demo-marker__verb-text').textContent === 'Stop', null, { timeout: 5000 });
    await page.waitForTimeout(200);
    await pressKeyInEditor(page);

    await page.waitForFunction(() => document.querySelector('moss-demo-marker[name="tree"] .moss-demo-marker__verb-text').textContent === 'Play', null, { timeout: 5000 });
    const textAfter = await editorText(page);
    assert(textAfter.includes('x'), `[${engine}] the interrupting keypress was not recorded: ${JSON.stringify(textAfter)}`);

    await page.waitForTimeout(300);
    const settled = await editorText(page);
    assert(settled === textAfter, `[${engine}] text kept changing after the interrupting keypress (playback did not stop): ${JSON.stringify({ textAfter, settled })}`);
  });
}

// The framed editor's own UI strings follow the page's locale (surfaces/editor.js's `lang` passthrough,
// site/ui/demo/README.md item 2): the context menu's "Versions…" row reads in English on the English
// guide and in the app's own zh-hant translation on the zh-hant guide. Found by `data-action`
// (ctx-menu.ts renders each row's own i18n key as this attribute — gestures.md), not by counting
// from the end of the menu, so a menu-order change cannot silently break this. Reads the English
// page's own row text as ground truth rather than typing a translation, and only requires the
// zh-hant row to differ from it and contain CJK characters — so a future retranslation cannot make
// this assert a specific string the app's own dictionary no longer says.
async function readVersionsMenuLabel(browser, path) {
  let label;
  await withPage(browser, path, { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    label = await page.evaluate(() => {
      const doc = document.querySelector('moss-editor-demo iframe').contentDocument;
      const seg = doc.querySelector('.seg.leaf.bc');
      const r = seg.getBoundingClientRect();
      seg.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, button: 2 }));
      return doc.querySelector('.ctx-menu [data-action="versions"]')?.textContent ?? '';
    });
  });
  return label;
}

async function checkContextMenuFollowsLocale(browser, engine) {
  const en = await readVersionsMenuLabel(browser, '/get-started/editor/');
  const zhHant = await readVersionsMenuLabel(browser, '/zh-hant/開始使用/editor/');
  assert(en === 'Versions…', `[${engine}] English guide's context-menu versions row reads "${en}", not "Versions…"`);
  assert(zhHant.length > 0 && zhHant !== en && /[一-鿿]/.test(zhHant),
    `[${engine}] zh-hant guide's context-menu versions row did not localize: en="${en}" zh-hant="${zhHant}"`);
}

// en frames a William Blake project, zh-hant a 朱耷 project (task: "different fixture per locale")
// — compared structurally (the open file's own breadcrumb path, non-empty, different between the
// two pages, Latin on en and CJK on zh-hant) rather than against a typed "Blake"/"朱耷" literal, so
// a future retitling of either fixture cannot make this assert stale prose.
async function checkFixturesFrameDifferentProjects(browser, engine) {
  const leafPath = async (path) => {
    let text;
    await withPage(browser, path, { width: 1440, height: 900 }, async (page) => {
      await waitEditorReady(page);
      text = await page.evaluate(() => document.querySelector('moss-editor-demo iframe').contentDocument.querySelector('.seg.leaf.bc')?.dataset.path ?? '');
    });
    return text;
  };
  const en = await leafPath('/get-started/editor/');
  const zhHant = await leafPath('/zh-hant/開始使用/editor/');
  assert(en.length > 0 && zhHant.length > 0, `[${engine}] one of the two fixtures framed no open file: en="${en}" zh-hant="${zhHant}"`);
  assert(en !== zhHant, `[${engine}] en and zh-hant framed the same open file: "${en}"`);
  assert(!/[一-鿿]/.test(en), `[${engine}] en fixture's open-file path contains CJK characters: "${en}"`);
  assert(/[一-鿿]/.test(zhHant), `[${engine}] zh-hant fixture's open-file path has no CJK characters: "${zhHant}"`);
}

// The "versions" scene's own diff view — reached mid-playback, at its "open a version" step —
// shows the OPEN piece's real text, not a fabricated string: captures the live document's text
// before playing, then asserts the version-detail view (mid-scene) contains a snippet of it.
async function checkVersionsDiffShowsOwnText(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    const liveText = await editorText(page);
    await clickMarker(page, 'versions');
    await page.waitForFunction(() => {
      const doc = document.querySelector('moss-editor-demo iframe').contentDocument;
      return (doc?.querySelector('.moss-versions__source')?.textContent ?? '').trim().length > 0;
    }, null, { timeout: await paceBoundMs('versions') });
    const diffText = await page.evaluate(() => document.querySelector('moss-editor-demo iframe').contentDocument.querySelector('.moss-versions__source').textContent);
    assert(diffText.includes(liveText.slice(0, 40)), `[${engine}] the versions scene's diff did not show the open piece's own text: diff="${diffText.slice(0, 80)}" live="${liveText.slice(0, 80)}"`);
    // Let the scene finish so a later check never inherits an in-flight "versions" playback.
    await page.waitForFunction(() => document.querySelector('moss-demo-marker[name="versions"] .moss-demo-marker__verb-text').textContent === 'Play', null, { timeout: await paceBoundMs('versions') });
  });
}

// The right-click look (site/ui/demo/README.md, "The pointer"): the pointer dot must read solid through
// the whole glide and dwell that precede a right-click's press, and switch to the hollow dotted
// look only once the press itself lands — never earlier, and never not at all. Regression guard
// for a bug where `press()`'s own animation cleanup cancelled the CSS `moss-demo-hollow` keyframe
// the instant it started (it and the dip/ring Web Animations API animations all showed up in the
// same `element.getAnimations()` sweep), so the hollow look never rendered a single frame — found
// by sampling this same computed style every ~30ms against a real build. "save-as-template"'s
// first step is a `context` (right-click) with a 600ms dwell and a long (reveals: 10) hold, wide
// enough to sample several points on each side of the press reliably.
async function checkRightClickHollowTiming(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    const samples = await page.evaluate(() => new Promise((resolve) => {
      const dot = document.querySelector('.moss-demo__pointer-dot');
      const pointer = document.querySelector('.moss-demo__pointer');
      const out = [];
      const timer = setInterval(() => {
        out.push({ button: pointer.dataset.button ?? null, borderStyle: getComputedStyle(dot).borderStyle });
      }, 30);
      document.querySelector('moss-demo-marker[name="save-as-template"] button').click();
      setTimeout(() => { clearInterval(timer); resolve(out); }, 2200);
    }));
    const pressIndex = samples.findIndex((s) => s.button === 'right');
    assert(pressIndex > 0, `[${engine}] never observed the right-click press (data-button="right") while sampling: ${JSON.stringify(samples)}`);
    const beforePress = samples.slice(0, pressIndex);
    assert(beforePress.every((s) => s.borderStyle === 'solid'),
      `[${engine}] the hollow look appeared before the press, during glide/dwell: ${JSON.stringify(beforePress)}`);
    const atOrAfterPress = samples.slice(pressIndex);
    assert(atOrAfterPress.some((s) => s.borderStyle === 'dotted'),
      `[${engine}] the hollow look never appeared after the right-click press: ${JSON.stringify(atOrAfterPress)}`);
  });
}

// The collapse-tree scene's pointer must visibly land ON the tree's bottom border (#divider), not
// merely dispatch a functionally-correct dblclick at it — the two had drifted apart: the real
// event's coordinates come from a fresh centerOf(el) at press time (driver.js), so it always hit
// #divider regardless, while the pointer's own glide target was a ONE-TIME reading of #divider's
// rect taken while the preceding "tree" scene's rows were still mounting (a CSS-driven height
// change measured at ~140ms), so the dot would glide to and visibly rest at a stale position well
// above the settled border. Regression guard: once the pointer arrives and stops moving, convert
// its own on-screen center to the harvested iframe's local coordinate space and assert
// elementFromPoint there is #divider itself.
async function checkCollapseTreePointerOnBorder(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    await page.evaluate(() => document.querySelector('moss-demo-marker[name="collapse-tree"] button').click());

    // Wait for the pointer to arrive and hold still (two consecutive polls reading the same
    // transform) — glideTo commits its final inline transform only once the glide animation
    // finishes, and it stays put through dwell/press/hold.
    await page.evaluate(() => { window.__lastPointerT = undefined; });
    await page.waitForFunction(() => {
      const pointer = document.querySelector('.moss-demo__pointer');
      if (pointer.hidden) return false;
      const t = pointer.style.transform;
      if (!t) return false;
      const stable = window.__lastPointerT === t;
      window.__lastPointerT = t;
      return stable;
    }, null, { timeout: await paceBoundMs('collapse-tree') });

    const hit = await page.evaluate(() => {
      const pointerRect = document.querySelector('.moss-demo__pointer').getBoundingClientRect();
      const cx = pointerRect.left + pointerRect.width / 2;
      const cy = pointerRect.top + pointerRect.height / 2;
      const iframe = document.querySelector('moss-editor-demo iframe');
      const iframeRect = iframe.getBoundingClientRect();
      const doc = iframe.contentDocument;
      const el = doc.elementFromPoint(cx - iframeRect.left, cy - iframeRect.top);
      return { id: el?.id ?? null, tag: el?.tagName ?? null };
    });
    assert(hit.id === 'divider', `[${engine}] the collapse-tree pointer's final position is not on #divider: elementFromPoint found ${JSON.stringify(hit)}`);

    // Let the scene actually finish, and confirm the double-click it dispatched really collapsed
    // the tree — the pointer landing correctly is not itself proof the gesture worked.
    await page.waitForFunction(() => document.querySelector('moss-demo-marker[name="collapse-tree"] .moss-demo-marker__verb-text').textContent === 'Play', null, { timeout: await paceBoundMs('collapse-tree') });
  });
}

// e. A load whose own scene-JSON fetch is still pending when a second, different scene is
// explicitly requested must not be able to claw back state once that stale fetch resolves — nor
// navigate the shared iframe again after a newer load has already finished (site/ui/demo/README.md, "One
// load at a time"). Every Play now navigates afresh regardless of fixture (site/ui/demo/README.md,
// "Play"), and app-editor.html reads no fixture-selecting query params at all (surfaces/editor.js), so the
// iframe's own `src` is identical across every load and can no longer serve as the "did it
// navigate again" signal. A real navigation always replaces `contentWindow` with a fresh object
// regardless of whether the URL changed, so this stamps a marker on the window once "tree" settles
// and checks it is still there.
async function checkSecondLoadDoesNotNavigate(browser, engine) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  try {
    await page.goto(new URL('/get-started/editor/', base).href, { waitUntil: 'load' });
    await page.waitForSelector('moss-editor-demo iframe');
    await waitEditorReady(page); // let the connect-time default-fixture load settle first

    let releaseA;
    const aHeld = new Promise((resolve) => { releaseA = resolve; });
    await page.route('**/ui/demo/scenes/properties.json', async (route) => {
      await aHeld;
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({ open: '', steps: [{ click: 'properties.add' }, { typeInto: { target: 'properties.search', text: 'cover' } }] }),
      });
    });

    await clickMarker(page, 'properties');
    await page.waitForFunction(() => {
      const demo = document.querySelector('moss-editor-demo');
      return demo.phase === 'loading' && demo.load?.name === 'properties';
    }, null, { timeout: 5000 });

    // "tree" is a second, explicit request while "properties"'s own fetch is still held — it
    // supersedes "properties" and plays out on its own fresh navigation before that stale fetch
    // ever resolves.
    await clickMarker(page, 'tree');
    await page.waitForFunction(() => document.querySelector('moss-editor-demo').phase === 'done', null, { timeout: await paceBoundMs('tree') });
    await page.evaluate(() => { document.querySelector('moss-editor-demo iframe').contentWindow.__mossNoRenavigateProbe = true; });

    releaseA();
    // Let the released, superseded fetch and whatever it does to the demo frame's state actually
    // finish before asserting — this is the window in which an unguarded continuation clobbers
    // state the newer load already settled.
    await page.waitForTimeout(1500);

    const stillSameWindow = await page.evaluate(() => document.querySelector('moss-editor-demo iframe').contentWindow.__mossNoRenavigateProbe === true);
    assert(stillSameWindow, `[${engine}] the superseded "properties" load navigated the shared iframe again once released`);

    const load = await page.evaluate(() => {
      const demo = document.querySelector('moss-editor-demo');
      const step = demo.load?.scene?.steps?.[0];
      return { name: demo.load?.name, phase: demo.phase, step: step?.click ?? step?.context };
    });
    assert(load.name === 'tree' && load.step === 'tree.breadcrumb', `[${engine}] the superseded "properties" load clobbered the current load's scene data once it resolved: ${JSON.stringify(load)}`);
    const treeState = await markerState(page, 'tree');
    assert(treeState.button === 'Play', `[${engine}] "tree" did not settle back to Play: ${JSON.stringify(treeState)}`);
  } finally {
    await page.close();
  }
}

// f. A failed scene load shows the failure line on its own marker and leaves the button reading
// Play so the reader can retry; retrying actually refetches (loadSceneData evicts a rejected
// fetch from its cache instead of caching the failure forever) rather than replaying the same
// rejection.
async function checkLoadFailureRetries(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    let requestCount = 0;
    await page.route('**/ui/demo/scenes/properties.json', (route) => {
      requestCount += 1;
      if (requestCount === 1) route.fulfill({ status: 500, contentType: 'text/plain', body: 'boom' });
      else route.continue();
    });

    await clickMarker(page, 'properties');
    await page.waitForFunction(() => document.querySelector('moss-demo-marker[name="properties"] .moss-demo-marker__status')?.textContent?.length > 0, null, { timeout: 15000 });
    const failed = await markerState(page, 'properties');
    assert(failed.status === 'The interactive editor could not load', `[${engine}] failed load did not show the failure line: ${JSON.stringify(failed)}`);
    assert(failed.button === 'Play', `[${engine}] error state does not read Play: ${JSON.stringify(failed)}`);

    await page.evaluate(() => document.querySelector('moss-demo-marker[name="properties"] button').click());
    await page.waitForFunction(() => document.querySelector('moss-demo-marker[name="properties"] .moss-demo-marker__status')?.textContent === '', null, { timeout: 15000 });
    // A full iframe reload (the retry's fixture wasn't applied on the failed attempt) plus the
    // animated steps' own pace — a real machine running other sessions' browsers alongside this
    // one needs real margin over a tight ceiling.
    await page.waitForFunction(() => document.querySelector('moss-demo-marker[name="properties"] .moss-demo-marker__verb-text').textContent === 'Play', null, { timeout: await paceBoundMs('properties') });
    assert(requestCount === 2, `[${engine}] retry did not issue a fresh request (evicted-cache fix): saw ${requestCount} request(s)`);
  });
}

// g. Theme follows the page; toolbar appears on focus and fits; sticky never overlaps prose;
// pinned fits a 1440×800 viewport; at 390px no horizontal overflow and the demo frame follows
// the first paragraph.

async function checkStickyLayout(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    const scrollHeight = await page.evaluate(() => document.querySelector('article').getBoundingClientRect().height + document.querySelector('article').getBoundingClientRect().top + window.scrollY);
    const tops = [];
    for (const frac of [0, 0.1, 0.25, 0.4, 0.55, 0.7]) {
      await page.evaluate((y) => window.scrollTo(0, y), Math.floor(scrollHeight * frac));
      await page.waitForTimeout(80);
      const rect = await page.evaluate(() => document.querySelector('moss-editor-demo').getBoundingClientRect());
      assert(rect.right <= 1440 + 1, `[${engine}] demo frame overflowed the viewport at ${frac}: ${JSON.stringify(rect)}`);
      assert(rect.top >= -1 && rect.bottom <= 900 + 1, `[${engine}] demo frame left the viewport vertically at ${frac} (sticky not holding): ${JSON.stringify(rect)}`);
      tops.push(rect.top);
      const overlap = await page.evaluate((demoRect) => [...document.querySelectorAll('article > :not(moss-editor-demo)')].some((el) => {
        const r = el.getBoundingClientRect();
        return r.left < demoRect.right && r.right > demoRect.left && r.top < demoRect.bottom && r.bottom > demoRect.top;
      }), rect);
      assert(!overlap, `[${engine}] prose overlaps the demo frame at scroll fraction ${frac}`);
    }
    // Sticky, not merely present: once past the initial offset, top should clamp to the same
    // value across multiple scroll positions rather than tracking the scroll linearly.
    const stuckTops = tops.slice(2, 6);
    const spread = Math.max(...stuckTops) - Math.min(...stuckTops);
    assert(spread < 1, `[${engine}] demo frame top did not stay clamped while scrolling (not actually sticky): ${JSON.stringify(stuckTops)}`);
  });

  // A shorter viewport (1440×800): the pinned demo frame must still fit, top offset present and
  // bottom not cut off, not just the taller 900px case above. Scrolls by a fraction of the page's
  // own scrollable range, the same way the 900px case above does, rather than a hard-coded pixel
  // offset — a fixed 2000px landed past the article's sticky range once the page got shorter (the
  // demo frame's top read -46 in WebKit), so this recomputes the fraction against whatever height the
  // page actually has. 0.4 sits in the middle of the 900px case's own confirmed "stuck" range
  // (0.25-0.7), which is unambiguously pinned in both engines.
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 800 }, async (page) => {
    const scrollHeight = await page.evaluate(() => document.querySelector('article').getBoundingClientRect().height + document.querySelector('article').getBoundingClientRect().top + window.scrollY);
    await page.evaluate((y) => window.scrollTo(0, y), Math.floor(scrollHeight * 0.4));
    await page.waitForTimeout(150);
    const rect = await page.evaluate(() => document.querySelector('moss-editor-demo').getBoundingClientRect());
    assert(rect.top > 0, `[${engine}] pinned demo frame has no top offset at 1440×800: ${JSON.stringify(rect)}`);
    assert(rect.bottom <= 800 + 1, `[${engine}] pinned demo frame's bottom is cut off at 1440×800: ${JSON.stringify(rect)}`);
  });
}

function luminance(rgbString) {
  const [r, g, b] = rgbString.match(/[\d.]+/g).map(Number);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

async function checkThemeFollowsPage(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);

    // A fresh session can start on 'sunlight' (this site's own script.js, "Sunlight is the
    // default first-visit experience") rather than 'light'/'dark', and window.toggleTheme()
    // (crates/moss-build/src/js-src/site/theme.ts) only ever flips exactly between those two —
    // from 'sunlight' one call lands on 'dark', not 'light'. Loop rather than assume a single
    // call suffices, so this still converges regardless of the page's starting theme.
    await page.evaluate(async () => {
      for (let guard = 0; document.documentElement.dataset.theme !== 'light' && guard < 3; guard++) window.toggleTheme();
    });
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'light');
    await page.waitForFunction(() => document.querySelector('moss-editor-demo iframe').contentDocument.documentElement.dataset.theme === 'light');
    const lightBg = await page.evaluate(() => getComputedStyle(document.querySelector('moss-editor-demo iframe').contentDocument.body).backgroundColor);

    await page.evaluate(() => window.toggleTheme());
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
    await page.waitForFunction(() => document.querySelector('moss-editor-demo iframe').contentDocument.documentElement.dataset.theme === 'dark');
    const darkState = await page.evaluate(() => {
      const doc = document.querySelector('moss-editor-demo iframe').contentDocument;
      return { theme: doc.documentElement.dataset.theme, chromeTheme: doc.documentElement.dataset.chromeTheme, bg: getComputedStyle(doc.body).backgroundColor };
    });
    assert(darkState.theme === 'dark' && darkState.chromeTheme === 'dark', `[${engine}] iframe did not pick up the page's dark theme: ${JSON.stringify(darkState)}`);
    assert(luminance(darkState.bg) < luminance(lightBg), `[${engine}] iframe body did not darken: light=${lightBg} dark=${darkState.bg}`);

    await page.evaluate(() => window.toggleTheme());
    await page.waitForFunction(() => document.querySelector('moss-editor-demo iframe').contentDocument.documentElement.dataset.theme === 'light');
    const backToLightBg = await page.evaluate(() => getComputedStyle(document.querySelector('moss-editor-demo iframe').contentDocument.body).backgroundColor);
    assert(luminance(backToLightBg) > luminance(darkState.bg), `[${engine}] iframe did not flip back to light: dark=${darkState.bg} backToLight=${backToLightBg}`);
  });
}

// Real behavior, not a sizing bug — the harvest's own insert-bar shows only once the editor view
// has focus (editor.html's InsertBar#sync reads view.hasFocus). Once shown, it must not be
// clipped by the demo frame's box.
async function checkToolbarShowsOnFocusAndFits(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    const frame = page.frameLocator('moss-editor-demo iframe');
    const beforeFocus = await page.evaluate(() => document.querySelector('moss-editor-demo iframe').contentDocument.querySelector('.insert-bar')?.classList.contains('visible'));
    assert(!beforeFocus, `[${engine}] toolbar is already visible before the editor has focus — the trigger is no longer focus`);

    await frame.locator('.cm-content').click();
    await page.waitForFunction(() => document.querySelector('moss-editor-demo iframe').contentDocument.querySelector('.insert-bar.visible'), null, { timeout: 5000 });

    const iframeRect = await page.evaluate(() => document.querySelector('moss-editor-demo iframe').getBoundingClientRect());
    const barRect = await page.evaluate(() => {
      const r = document.querySelector('moss-editor-demo iframe').contentDocument.querySelector('.insert-bar').getBoundingClientRect();
      return { top: r.top, bottom: r.bottom, left: r.left, right: r.right };
    });
    assert(
      barRect.top >= 0 && barRect.bottom <= iframeRect.height + 1 && barRect.left >= 0 && barRect.right <= iframeRect.width + 1,
      `[${engine}] toolbar is clipped by the demo frame's box: iframe=${JSON.stringify(iframeRect)} bar(iframe-local)=${JSON.stringify(barRect)}`,
    );
  });
}

async function checkNarrow(browser, engine) {
  await withPage(browser, '/get-started/editor/', { width: 390, height: 844 }, async (page) => {
    const result = await page.evaluate(() => {
      const demo = document.querySelector('moss-editor-demo');
      const firstParagraph = document.querySelector('article p');
      return {
        overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
        position: getComputedStyle(demo).position,
        demoTop: demo.getBoundingClientRect().top,
        pTop: firstParagraph ? firstParagraph.getBoundingClientRect().top : null,
        pText: firstParagraph ? firstParagraph.textContent.trim() : '',
        pBottom: firstParagraph ? firstParagraph.getBoundingClientRect().bottom : null,
        viewportHeight: window.innerHeight,
      };
    });
    assert(result.overflow <= 0, `[${engine}] page overflows horizontally at 390px: ${result.overflow}px`);
    assert(result.position !== 'sticky', `[${engine}] demo frame is sticky at 390px, expected inline: ${result.position}`);
    assert(result.pTop !== null && result.pTop < result.demoTop, `[${engine}] the opening paragraph does not precede the demo frame at 390px: ${JSON.stringify(result)}`);
    assert(result.pText.length > 0 && result.pBottom > 0 && result.pTop < result.viewportHeight,
      `[${engine}] the first viewport at 390px shows no prose before the demo frame: ${JSON.stringify(result)}`);
  });
}

// h. At 1440px, the header divider's, the footer divider's, and the series-nav divider's
// left/right edges match the article's outer edges within 1px, on "Meet the editor" (a demo
// page) and on a docs page with no demo frame alike. moss's `.container` reads --moss-site-max-width,
// but the nav, the article, the footer, and the series-nav each carry a MORE specific site.css
// rule of their own that reads --moss-content-width instead (see site/.moss/theme/style.css) —
// this exercises all four, not just the variable.
async function checkDividerWidthsMatchArticle(browser, engine) {
  const widths = (page) => page.evaluate(() => {
    const rect = (el) => { const r = el.getBoundingClientRect(); return { left: r.left, right: r.right, width: r.width }; };
    // Custom properties come back as authored text (e.g. "72ch"), not a resolved pixel number —
    // laying out a real probe element is what forces the browser to resolve var()/calc() to px.
    const pxOf = (cssWidth) => {
      const probe = document.createElement('div');
      probe.style.cssText = `position:absolute; visibility:hidden; inline-size:${cssWidth};`;
      document.body.appendChild(probe);
      const px = probe.getBoundingClientRect().width;
      probe.remove();
      return px;
    };
    // The same formula site/.moss/theme/style.css sets --moss-site-max-width to, recomputed here
    // from the page's own resolved custom properties — not a copied constant — so this catches
    // the rule being missing even though moss's own --moss-site-max-width default (1200px) is
    // ALSO wider than a plain page's content width, which a mere "wider than X" bound would miss.
    const expectedWideWidth = Math.min(
      pxOf('calc(var(--moss-content-width) + var(--moss-space-lg) + var(--moss-demo-width))'),
      window.innerWidth - 2 * pxOf('var(--moss-container-padding)'),
    );
    return {
      nav: rect(document.querySelector('nav.main-nav')),
      article: rect(document.querySelector('article')),
      footer: rect(document.querySelector('footer.container')),
      seriesNav: rect(document.querySelector('nav.moss-series-nav')),
      expectedWideWidth,
    };
  });
  const assertMatch = (w, label) => {
    assert(Math.abs(w.nav.left - w.article.left) <= 1 && Math.abs(w.nav.right - w.article.right) <= 1,
      `[${engine}] header divider does not match the article's width ${label}: ${JSON.stringify(w)}`);
    assert(Math.abs(w.footer.left - w.article.left) <= 1 && Math.abs(w.footer.right - w.article.right) <= 1,
      `[${engine}] footer divider does not match the article's width ${label}: ${JSON.stringify(w)}`);
    assert(Math.abs(w.seriesNav.left - w.article.left) <= 1 && Math.abs(w.seriesNav.right - w.article.right) <= 1,
      `[${engine}] series-nav divider does not match the article's width ${label}: ${JSON.stringify(w)}`);
  };
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    const w = await widths(page);
    assertMatch(w, 'on a page with a demo frame');
    assert(Math.abs(w.article.width - w.expectedWideWidth) <= 2,
      `[${engine}] a page with a demo frame is not the wide formula's width (moss's own --moss-site-max-width default would ALSO be wider than a plain page, so this checks the exact value, not just "wider"): got=${w.article.width} expected=${w.expectedWideWidth}`);
  });
  await withPlainPage(browser, '/docs/writing/structure/', { width: 1440, height: 900 }, async (page) => {
    assertMatch(await widths(page), 'on a page with no demo frame');
  });
}

// j. "Pressing Play moves nothing" (site/ui/demo/README.md) on the wide layout: scrollY, the opening
// paragraph, the demo frame, and the pressed button's own rect must not move by more than 1px at any
// point across the WHOLE scene, not just before/after — a mid-scene jump (a superseded
// scrollIntoView, a reflow from a result appearing) would otherwise slip between two snapshots.
// Sampled once every 100ms from the moment Play is pressed for `durationMs`, comfortably past the
// scene's own worst-case length under driver.js's Pace table.
function measureStillness(page, name, durationMs) {
  return page.evaluate(({ n, duration }) => new Promise((resolve) => {
    function snap() {
      const marker = document.querySelector(`moss-demo-marker[name="${n}"]`);
      const button = marker.querySelector('button');
      const firstP = document.querySelector('article p');
      const demo = document.querySelector('moss-editor-demo');
      const pos = (el) => { const r = el.getBoundingClientRect(); return { top: r.top, left: r.left }; };
      return { scrollY: window.scrollY, firstP: pos(firstP), demo: pos(demo), button: pos(button) };
    }
    const base = snap();
    const samples = [base];
    const start = performance.now();
    document.querySelector(`moss-demo-marker[name="${n}"] button`).click();
    const interval = setInterval(() => {
      samples.push(snap());
      if (performance.now() - start > duration) {
        clearInterval(interval);
        // Largest top/left drift from the baseline snapshot, across every sample — a rect "moving"
        // means either coordinate changing, so both are checked and the worse one kept.
        const maxDelta = (pick) => Math.max(...samples.map((s) => {
          const a = pick(s);
          const b = pick(base);
          return Math.max(Math.abs(a.top - b.top), Math.abs(a.left - b.left));
        }));
        resolve({
          maxScrollDelta: Math.max(...samples.map((s) => Math.abs(s.scrollY - base.scrollY))),
          maxFirstPDelta: maxDelta((s) => s.firstP),
          maxDemoDelta: maxDelta((s) => s.demo),
          maxButtonDelta: maxDelta((s) => s.button),
          sampleCount: samples.length,
        });
      }
    }, 100);
  }), { n: name, duration: durationMs });
}

async function checkPlayDoesNotShiftPage(browser, engine) {
  // Marker "tree", at the top of the page: a two-step scene, worst case a little over 4s.
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    const tree = await measureStillness(page, 'tree', await paceBoundMs('tree'));
    assert(
      tree.maxScrollDelta <= 1 && tree.maxFirstPDelta <= 1 && tree.maxDemoDelta <= 1 && tree.maxButtonDelta <= 1,
      `[${engine}] Play shifted the page for marker "tree": ${JSON.stringify(tree)}`,
    );
  });

  // Marker "versions", scrolled down into view first (so the demo frame is pinned/sticky — the exact
  // condition that reproduced the ~60px scrollIntoView-vs-sticky jump this check guards against):
  // the longest scene on the page, six steps.
  await withPage(browser, '/get-started/editor/', { width: 1440, height: 900 }, async (page) => {
    await waitEditorReady(page);
    await page.evaluate(() => document.querySelector('moss-demo-marker[name="versions"]').scrollIntoView({ block: 'center' }));
    await page.waitForTimeout(150);
    const v = await measureStillness(page, 'versions', await paceBoundMs('versions'));
    assert(
      v.maxScrollDelta <= 1 && v.maxFirstPDelta <= 1 && v.maxDemoDelta <= 1 && v.maxButtonDelta <= 1,
      `[${engine}] Play shifted the page for marker "versions": ${JSON.stringify(v)}`,
    );
  });
}

// The consolidated playback check: every marker on every page in DEMO_PAGES plays to 'done' with
// no load error (its own status line stays empty) and no page error (an uncaught exception on the
// host page), within a pace-derived bound for its own scene. Replaces per-scene assertions about
// what a scene's steps specifically leave behind — site/ui/demo/README.md's own module boundary already
// keeps a scene's *effect* out of player.js/demo-frame.js's concern, so this checks only what the
// demo frame contract promises for every scene: it plays, and it settles back to Play.
async function checkEveryMarkerPlaysToDone(browser, engine) {
  for (const { path, markers } of DEMO_PAGES) {
    await withPage(browser, path, { width: 1440, height: 900 }, async (page) => {
      const pageErrors = [];
      page.on('pageerror', (e) => pageErrors.push(e.message));
      await waitEditorReady(page);
      for (const name of markers) {
        await clickMarker(page, name);
        // The marker's own Play/Stop text is localized per page (strings.js) — zh-hant reads
        // "播放"/"停止", not "Play"/"Stop" — so this reads moss-editor-demo's own `phase` property
        // instead of button text: locale-independent, and it is what the button's text already
        // derives from. Waits for 'playing' first, proving the click actually took effect, before
        // waiting for it to leave that phase — the same two-phase shape as the button-text version
        // this replaces, so a click that never registers still fails loudly instead of the first
        // wait passing trivially (the button/phase reads exactly its pre-click state until then).
        await page.waitForFunction((n) => {
          const demo = document.querySelector('moss-editor-demo');
          return demo.load?.name === n && demo.phase === 'playing';
        }, name, { timeout: 5000 });
        const bound = await paceBoundMs(name);
        await page.waitForFunction((n) => {
          const demo = document.querySelector('moss-editor-demo');
          return demo.load?.name === n && demo.phase !== 'playing' && demo.phase !== 'loading';
        }, name, { timeout: bound });
        const phase = await page.evaluate(() => document.querySelector('moss-editor-demo').phase);
        const status = await page.evaluate((n) => document.querySelector(`moss-demo-marker[name="${n}"] .moss-demo-marker__status`)?.textContent ?? '', name);
        assert(phase === 'done', `[${engine}] ${path}: marker "${name}" ended in phase "${phase}", not done`);
        assert(status === '', `[${engine}] ${path}: marker "${name}" reported a load error: "${status}"`);
      }
      assert(pageErrors.length === 0, `[${engine}] ${path}: page error(s) during marker playback: ${JSON.stringify(pageErrors)}`);
    });
  }
}

async function withBrowser(launcher, fn) {
  const browser = await launcher.launch({ headless: true });
  try {
    await fn(browser);
  } finally {
    await browser.close();
  }
}

const engines = [['chromium', playwright.chromium], ['webkit', playwright.webkit]];
for (const [engine, launcher] of engines) {
  await withBrowser(launcher, (browser) => checkRestState(browser, engine));
  await withBrowser(launcher, (browser) => checkEveryMarkerPlaysToDone(browser, engine));
  await withBrowser(launcher, (browser) => checkInterruptStopsPlayback(browser, engine));
  await withBrowser(launcher, (browser) => checkContextMenuFollowsLocale(browser, engine));
  await withBrowser(launcher, (browser) => checkFixturesFrameDifferentProjects(browser, engine));
  await withBrowser(launcher, (browser) => checkVersionsDiffShowsOwnText(browser, engine));
  await withBrowser(launcher, (browser) => checkRightClickHollowTiming(browser, engine));
  await withBrowser(launcher, (browser) => checkCollapseTreePointerOnBorder(browser, engine));
  await withBrowser(launcher, (browser) => checkSecondLoadDoesNotNavigate(browser, engine));
  await withBrowser(launcher, (browser) => checkLoadFailureRetries(browser, engine));
  await withBrowser(launcher, (browser) => checkStickyLayout(browser, engine));
  await withBrowser(launcher, (browser) => checkThemeFollowsPage(browser, engine));
  await withBrowser(launcher, (browser) => checkToolbarShowsOnFocusAndFits(browser, engine));
  await withBrowser(launcher, (browser) => checkNarrow(browser, engine));
  await withBrowser(launcher, (browser) => checkDividerWidthsMatchArticle(browser, engine));
  await withBrowser(launcher, (browser) => checkPlayDoesNotShiftPage(browser, engine));
  console.log(`[${engine}] all checks passed`);
}
