#!/usr/bin/env node
// Records an author's own clicks / right-clicks / double-clicks / typed-and-committed input
// fields against a surface's real document, mapping each interaction to a named target by
// reverse lookup in the surface adapter (site/ui/demo/surfaces/*.js) — the same lookup a person
// hand-authoring a scene would do, automated. An interaction on an element with no named target
// prints a clear message naming the element, so the author can add a `data-action` or a TARGETS
// entry instead of the recording silently dropping it. On window close (or Ctrl-C) it writes
// site/ui/demo/scenes/<name>.json and prints the link to paste (site/ui/demo/README.md,
// "Add a demo").
//
// Usage:
//   node scripts/record-scene.mjs <name> [--lang en|zh-hant|zh-hans] [--surface editor]
//
// Test-only hook, for exercising the recorder itself without a person at the keyboard:
//   node scripts/record-scene.mjs <name> --headless --script <file>
// <file>'s default export is `async (page) => { ... }`, called with the Playwright Page already
// navigated to the surface document; Playwright's own input simulation dispatches trusted DOM
// events, so this drives the exact listeners a real author's clicks would. The recorder closes
// the browser (and so writes its scene) once the script's promise resolves.

import { writeFile } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolve, relative } from 'node:path';
import { resolveBaseURL, loadPlaywright } from './site-check-harness.mjs';

const ROOT = resolve(fileURLToPath(new URL('.', import.meta.url)), '..');
const SITE_UI_DIR = resolve(ROOT, 'site/ui');
const SCENES_DIR = resolve(SITE_UI_DIR, 'demo/scenes');

const LOCALES = new Set(['en', 'zh-hant', 'zh-hans']);
const DEFAULT_SURFACE = 'editor';
const DEFAULT_REVEALS = 2; // driver.js PACE.defaultReveals

// Which document a surface records against, and where its adapter module lives — both paths
// relative to SITE_UI_DIR (the server root below). Kept as data here, alongside SURFACES rather
// than re-derived from surfaces/editor.js's own EDITOR_URL, because that URL is built from
// `import.meta.url` for the BROWSER's benefit (site/ui/demo/README.md, "Surfaces") and this
// script needs an http:// path before any page exists to resolve it against.
const SURFACE_DOCUMENTS = {
  editor: { doc: 'app-editor.html', adapter: '/demo/surfaces/editor.js' },
};

function parseArgs(argv) {
  const args = { lang: 'en', surface: DEFAULT_SURFACE, headless: false, script: null, name: null };
  const positionals = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--lang') args.lang = argv[++i];
    else if (a === '--surface') args.surface = argv[++i];
    else if (a === '--headless') args.headless = true;
    else if (a === '--script') args.script = argv[++i];
    else positionals.push(a);
  }
  args.name = positionals[0] ?? null;
  return args;
}

const args = parseArgs(process.argv.slice(2));
if (!args.name || !/^[a-z0-9-]+$/.test(args.name)) {
  console.error('Usage: node scripts/record-scene.mjs <name> [--lang en|zh-hant|zh-hans] [--surface editor]');
  console.error('  <name> is a scene filename: lowercase letters, digits, hyphens.');
  process.exit(2);
}
const surfaceDoc = SURFACE_DOCUMENTS[args.surface];
if (!surfaceDoc) {
  console.error(`Unknown --surface "${args.surface}" (known: ${Object.keys(SURFACE_DOCUMENTS).join(', ')})`);
  process.exit(2);
}
if (!LOCALES.has(args.lang)) {
  console.error(`Unknown --lang "${args.lang}" (known: ${[...LOCALES].join(', ')})`);
  process.exit(2);
}

// The in-page half, injected before any page script runs (page.addInitScript, so it survives the
// surface document's own reloads too — every Play in the real system is a fresh load, and a
// recording session should be able to reload without losing its listeners). A classic, non-module
// script: `import()` the operator works in either, and this avoids needing the adapter module
// itself to finish loading before the page's own boot does.
function recorderInitScript(adapterHref) {
  return `(() => {
    // Resolved once, ahead of any real interaction — the adapter module loads in milliseconds at
    // page boot, well before a person (or a --script driver) can click anything, so every handler
    // below reads it synchronously rather than awaiting the import each time. That matters: a
    // click on a target like the breadcrumb triggers the app's OWN re-render (expanding the tree),
    // which can replace the clicked element's whole ancestor chain — resolving the name must
    // happen in THIS same synchronous capture-phase turn, before that re-render runs, or the
    // walk below finds nothing where the browser plainly showed something clickable.
    let editorModule = null;
    import(${JSON.stringify(adapterHref)}).then((mod) => { editorModule = mod; });
    let pendingClick = null;
    function describe(el) {
      if (!el || !el.tagName) return '(no element)';
      const id = el.id ? '#' + el.id : '';
      const cls = (typeof el.className === 'string' && el.className.trim()) ? '.' + el.className.trim().split(/\\s+/).join('.') : '';
      const text = (el.textContent || '').trim().slice(0, 30);
      return el.tagName.toLowerCase() + id + cls + (text ? ' "' + text + '"' : '');
    }
    // Synchronous: walks up from the element, asking every named target whether it IS this ancestor —
    // the same reverse lookup a person authoring a scene by hand would do.
    function resolveName(el) {
      if (!editorModule) return null; // interaction landed before the adapter finished loading
      const { findTarget, TARGET_NAMES } = editorModule;
      for (let node = el; node && node !== document.documentElement; node = node.parentElement) {
        for (const name of TARGET_NAMES) {
          if (findTarget(document, name) === node) return name;
        }
      }
      return null;
    }
    function report(kind, name, el) {
      if (name) window.__mossRecordStep({ kind, name });
      else window.__mossRecordMiss(kind + ' on ' + describe(el) + ' has no named target — add a data-action, or a TARGETS entry in surfaces/' + ${JSON.stringify(args.surface)} + '.js');
    }
    // A genuine double-click fires click(detail=1), click(detail=2), dblclick in that order — the
    // two click events are the same physical gesture a scripted dblclick step (driver.js) never
    // sends, so recording them as their own steps would double what one gesture actually was. A
    // click's NAME is resolved immediately (see resolveName's own comment) and held for
    // DBLCLICK_WINDOW_MS in case a dblclick follows; a SECOND click landing on the SAME element
    // inside that window is the double-click's own first half (cancelled here, dblclick below
    // reports it) — a second click on a DIFFERENT element is just the next single click and
    // flushes the pending one immediately rather than losing it.
    const DBLCLICK_WINDOW_MS = 350;
    document.addEventListener('click', (e) => {
      if (!e.isTrusted) return;
      const el = e.target;
      const name = resolveName(el);
      if (pendingClick) {
        clearTimeout(pendingClick.timer);
        const previous = pendingClick;
        pendingClick = null;
        if (previous.el === el) return; // first half of a double-click; dblclick reports it
        report('click', previous.name, previous.el);
      }
      pendingClick = { el, name, timer: setTimeout(() => { pendingClick = null; report('click', name, el); }, DBLCLICK_WINDOW_MS) };
    }, { capture: true });
    document.addEventListener('dblclick', (e) => {
      if (!e.isTrusted) return;
      if (pendingClick) { clearTimeout(pendingClick.timer); pendingClick = null; }
      report('dblclick', resolveName(e.target), e.target);
    }, { capture: true });
    document.addEventListener('contextmenu', (e) => {
      if (!e.isTrusted) return;
      report('context', resolveName(e.target), e.target);
    }, { capture: true });
    // typeInto (site/ui/demo/README.md: "there is no bare type verb") — only a COMMITTED value,
    // on Enter, matches what the real driver plays back.
    document.addEventListener('keydown', (e) => {
      if (!e.isTrusted || e.key !== 'Enter') return;
      const el = e.target;
      if (!el || el.tagName !== 'INPUT') return;
      const name = resolveName(el);
      if (name) window.__mossRecordStep({ kind: 'typeInto', name, text: el.value ?? '' });
      else window.__mossRecordMiss('typed-and-committed input on ' + describe(el) + ' has no named target — add a data-action, or a TARGETS entry in surfaces/' + ${JSON.stringify(args.surface)} + '.js');
    }, { capture: true });
  })();`;
}

function stepFromRecord(record) {
  if (record.kind === 'typeInto') return { typeInto: { target: record.name, text: record.text } };
  return { [record.kind]: record.name, reveals: DEFAULT_REVEALS };
}

async function main() {
  const playwright = await loadPlaywright();
  const { baseURL, close: closeServer } = await resolveBaseURL(undefined, { root: SITE_UI_DIR });

  const browser = await playwright.chromium.launch({ headless: args.headless });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });

  const recorded = [];
  await page.exposeFunction('__mossRecordStep', (record) => {
    recorded.push(record);
    console.log(`[record-scene] recorded ${record.kind} -> "${record.name}"${record.kind === 'typeInto' ? `: "${record.text}"` : ''}`);
  });
  await page.exposeFunction('__mossRecordMiss', (message) => {
    console.log(`[record-scene] MISS: ${message}`);
  });
  page.on('pageerror', (error) => console.error('[record-scene] page error:', error.message));
  page.on('console', (msg) => { if (process.env.MOSS_RECORD_DEBUG) console.error('[record-scene] console:', msg.type(), msg.text()); });
  await page.addInitScript({ content: recorderInitScript(surfaceDoc.adapter) });

  let finished = false;
  async function finish() {
    if (finished) return;
    finished = true;
    const steps = recorded.map(stepFromRecord);
    const scene = args.surface === DEFAULT_SURFACE ? { steps } : { surface: args.surface, steps };
    const scenePath = resolve(SCENES_DIR, `${args.name}.json`);
    await writeFile(scenePath, `${JSON.stringify(scene, null, 2)}\n`);
    console.log(`\nWrote ${relative(ROOT, scenePath)} (${steps.length} step(s))`);
    console.log(`Paste this link where the scene belongs: [▶](#scene=${args.name})`);
    await closeServer();
  }
  page.on('close', finish);
  browser.on('disconnected', finish);
  process.on('SIGINT', async () => { await finish(); process.exit(0); });

  await page.goto(`${baseURL}${surfaceDoc.doc}?lang=${args.lang}`, { waitUntil: 'load' });

  if (args.script) {
    const scriptPath = resolve(process.cwd(), args.script);
    const { default: run } = await import(pathToFileURL(scriptPath).href);
    await run(page);
    // The click/dblclick debounce above (DBLCLICK_WINDOW_MS) can still be pending when the
    // driver script returns — a real author's own pause before closing the window always clears
    // it, but a script that clicks and returns immediately would otherwise lose its last click.
    await page.waitForTimeout(500);
    await browser.close(); // triggers 'disconnected' -> finish()
  } else if (args.headless) {
    console.log('[record-scene] --headless with no --script has nothing to drive; waiting for Ctrl-C.');
  } else {
    console.log(`[record-scene] recording against ${baseURL}${surfaceDoc.doc}?lang=${args.lang}`);
    console.log('[record-scene] click, right-click, double-click, or type-and-press-Enter in the window; close it (or Ctrl-C here) when done.');
  }
}

await main();
