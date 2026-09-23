// The "editor" surface adapter: everything demo-frame.js and driver.js need to know about the
// harvested editor document (site/ui/app-editor.html) to drive it — its named click/context
// targets, its theme attributes, its ready check, and how to frame it. This is the adapter
// interface every surface implements (site/ui/demo/README.md, "Surfaces"):
//
//   DEFAULT_FIXTURE                      the fixture used when a scene names none
//   findTarget(doc, name) -> Element|null      resolves a named target, or null if not yet rendered
//   setTheme(doc, theme) -> void                applies the page's light/dark theme to the framed document
//   frameUrl(fixture, locale) -> string         the URL that opens the framed document
//   waitForReady(win, doc) -> Promise<handle>   resolves once the framed document is interactive
//
// Moves into the desktop app repository's scripts/landing/ once the driver gains a `load(fixture)` method (see
// site/ui/demo/README.md, "The harvested document") — it belongs with the harvest, not the
// reader-facing demo frame.

const EDITOR_URL = new URL('../../app-editor.html', import.meta.url);

// app-editor.html's own supported locales (its `LA` array) — the same three strings.js keys its
// UI strings are localized to.
const LOCALES = new Set(['en', 'zh-hant', 'zh-hans']);

// app-editor.html's project and first file are chosen by its own mocked Tauri seams
// (the app's boot parameters, scripts/landing/app-editor-boot-params.ts) from the `?lang=` query string alone —
// `frameUrl` below is the one place that sets it (en: a William Blake project; zh-hant/zh-hans: a
// 朱耷 project, traditional vs. simplified). Kept as an exported empty object, rather than removed,
// so demo-frame.js's `scene.fixture ?? DEFAULT_FIXTURE` shape survives unchanged; frameUrl ignores
// whatever it is handed beyond the locale.
export const DEFAULT_FIXTURE = {};

const TARGETS = {
  // The root breadcrumb segment ("Click the site root in the breadcrumb to expand the file tree"
  // — Meet the editor, "Find and create pages"). `[data-path=""]` is the project root's own path,
  // present at boot alongside the folder and leaf segments already on the frozen anchor's spine
  // (gestures.md: the open file sits one level inside a folder, so the spine is root → folder →
  // leaf). Clicking it mounts the root's own non-spine children into `.nav-body` — for this
  // fixture, only its home file (verified interactively; the spine's own folder segment is not
  // duplicated as a row here).
  'tree.breadcrumb': (doc) => doc.querySelector('.seg.bc[data-path=""]'),

  // The tree's bottom border (gestures.md, "Collapse the tree") — a 12px drag handle that is also
  // a double-click target: dblclick collapses/reopens `#nav-surface` without touching `.nav-body`'s
  // mounted rows.
  'tree.divider': (doc) => doc.querySelector('#divider'),

  // The first markdown row mounted into the tree body, whatever it is — used both to "click a page
  // to open it" once the root is expanded (Meet the editor, "Find and create pages") and, after a
  // right-click elsewhere reveals sibling rows, generically as "a page". `[data-path$=".md"]`
  // is locale-independent (the extension, not the filename, is what marks a row as a page) and
  // never matches the currently open file or an on-spine folder — both are excluded from `.nav-body`
  // by the frozen anchor (verified interactively).
  'tree.pageRow': (doc) => doc.querySelector('.nav-body [data-path$=".md"]'),

  // The titlebar's ➕ (gestures.md, "Create a page"): creates a page in the currently browsed
  // folder immediately and opens it, caret on the placeholder heading — no tree expansion needed
  // first (verified interactively; its target is the create-flow's own anchor, not a tree
  // selection).
  'create.newPageButton': (doc) => doc.querySelector('#editor-create-btn'),
  // The ▾ beside it (gestures.md, "New from template"): opens the "New Page options" menu (New
  // Page, New Folder, one row per saved template, Manage Templates… once any exist).
  'create.caret': (doc) => doc.querySelector('#editor-create-caret'),
  // The caret menu's own "New Folder" row (`id: "new-folder"` in the menu's own item list, so
  // `data-action="new-folder"` — same ctx-menu.ts convention as every other row below).
  'create.newFolderAction': (doc) => doc.querySelector('.ctx-menu [data-action="new-folder"]'),
  // The inline rename/create `<input>` a plain "New Page"/"New Folder" context-menu action opens
  // in the tree (gestures.md, "Input-typing events") — REPLACES the placeholder row rather than
  // sitting beside it, so this is the only element to find once it appears.
  'tree.renameInput': (doc) => doc.querySelector('input.moss-tree-rename-input'),

  // Every ctx-menu row now carries `data-action`, the row's own i18n key rendered as a DOM
  // attribute (gestures.md) — stable across locales, unlike matching a row's translated text or
  // its position among sibling rows.
  'templates.saveAsTemplateAction': (doc) => doc.querySelector('.ctx-menu [data-action="save-as-template"]'),
  // The name prompt save-as-template opens (gestures.md, "Input-typing events"): a real `<input>`,
  // reused below as the template's own rename/delete prompts would use the same class — only one
  // `.input-popover` is ever open at a time, so the generic selector is unambiguous.
  'templates.nameInput': (doc) => doc.querySelector('.input-popover .pop-input'),
  // A saved template's row in the caret menu (gestures.md, "New from template"): `id` is
  // `template:<id>`, matched by prefix rather than hardcoding the id template storage assigns.
  'templates.fromTemplateRow': (doc) => doc.querySelector('.ctx-menu [data-action^="template:"]'),

  // "Click **+** to add a property" (Meet the editor, "Set page properties"). The add-property
  // menu's search box filters by the field's raw, untranslated key as well as its translated label
  // (gestures.md, "Add a Date property"), so typing a field's key is the one way to reach a
  // specific field that works in every locale.
  'properties.add': (doc) => doc.querySelector('.chip-unified-btn'),
  'properties.search': (doc) => doc.querySelector('.omenu-search'),

  // The current file's breadcrumb segment — right-clicking it reaches the same file context menu
  // as right-clicking its row (verified interactively: both list `save-as-template` and
  // `versions`), so it is the shortest path to either and needs no tree expansion first. The only
  // `.seg` carrying both `leaf` and `bc`, always present at boot.
  'versions.currentFile': (doc) => doc.querySelector('.seg.leaf.bc'),
  // The "Versions…" row of the context menu `versions.currentFile` opens, by `data-action` rather
  // than by counting from the end of a fixed-looking row order (gestures.md).
  'versions.open': (doc) => doc.querySelector('.ctx-menu [data-action="versions"]'),
  // The ✚ toggle that reveals the save-a-version field (gestures.md step 4).
  'versions.saveToggle': (doc) => doc.querySelector('.moss-versions__save-toggle'),
  // The confirm button. The name field beside it is optional (a placeholder, not a required
  // value), so the versions scene never types into it before confirming.
  'versions.saveConfirm': (doc) => doc.querySelector('.moss-versions__save-confirm'),
  // The version just saved, prepended to the top of the list.
  'versions.firstRow': (doc) => doc.querySelector('.moss-versions__row:first-child'),
  // Restores the open version. For page scope this needs no confirm modal, and it exits Versions
  // mode on its own once it finishes (gestures.md step 8) — no further step is needed to leave it.
  'versions.restore': (doc) => doc.querySelector('.moss-versions__restore'),
};

/** Resolves a named target against the harvested document, or `null` if it is not (yet) present —
 * the timing-tolerant half `driver.js` polls with while it waits for a step's target to render.
 * Still throws for an unknown name: that is a scene-authoring bug, not a timing issue. */
export function findTarget(doc, name) {
  const find = TARGETS[name];
  if (!find) throw new Error(`Unknown click target: "${name}"`);
  return find(doc) ?? null;
}

/** Every target name this surface's scenes may use — the set check-demo-scenes.mjs (site/ui/demo/
 * README.md, "Static check") validates a scene's step targets against, without loading a browser. */
export const TARGET_NAMES = new Set(Object.keys(TARGETS));

// The harvested document's own dark/light switch (see its `[data-theme]` and
// `[data-chrome-theme]` CSS blocks): `data-theme` covers the app surface, `data-chrome-theme`
// the window-chrome tokens layered on top. Both take the same value — the harvest never mixes
// them — which is why demo-frame.js hands this one theme string rather than two.
export function setTheme(doc, theme) {
  doc.documentElement.dataset.theme = theme;
  doc.documentElement.dataset.chromeTheme = theme;
}

/** The URL that opens the harvested document. `fixture` is accepted only to keep the frame's one
 * call shape stable — app-editor.html reads no fixture-selecting query params at all (see
 * DEFAULT_FIXTURE above), so every field of it is ignored today. The old hand-assembled
 * editor.html's `root`/`file`/`date` params, and the one line of demo-frame.js that varied this
 * URL by them, are both gone; this module is the only place that knew about them, and the only
 * place that needed to change.
 *
 * `locale` frames the editor in the page's own locale: app-editor.html reads `?lang=en|zh-hant|
 * zh-hans`, renders its own UI strings (menus, labels) in that locale before first render, AND
 * picks which in-memory project opens — a William Blake project for `en`, a 朱耷 project
 * (traditional characters for `zh-hant`, simplified for `zh-hans`) otherwise. demo-frame.js is the
 * one place that resolves a page's locale (for strings.js); this function is
 * the one place that turns it into a query param — no other module reads or writes the query
 * string at all. A falsy or unrecognized locale leaves the URL unchanged; app-editor.html already
 * ignores an unsupported `lang` value on its own, but checking here keeps a plain call shape
 * (`frameUrl(fixture)`, no locale) from silently appending `lang=undefined`. */
export function frameUrl(_fixture, locale) {
  if (!LOCALES.has(locale)) return EDITOR_URL.href;
  const url = new URL(EDITOR_URL);
  url.searchParams.set('lang', locale);
  return url.href;
}

const READY_TIMEOUT_MS = 10000;

/** Resolves once `win.__editor` exists and the harvested document marks itself ready — the
 * adapter interface's `waitForReady`. Its resolved value becomes the frame's `getHandle()`, which
 * driver.js's `setText`/`setTextInto` verbs call `.setDoc()` on. */
export function waitForReady(win, doc) {
  if (doc.documentElement.dataset.ready === '1' && win.__editor) return Promise.resolve(win.__editor);
  return new Promise((resolve, reject) => {
    const observer = new MutationObserver(check);
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error('Editor did not become ready in time'));
    }, READY_TIMEOUT_MS);
    function check() {
      if (doc.documentElement.dataset.ready === '1' && win.__editor) {
        cleanup();
        resolve(win.__editor);
      }
    }
    function cleanup() {
      observer.disconnect();
      clearTimeout(timer);
    }
    observer.observe(doc.documentElement, { attributes: true, attributeFilter: ['data-ready'] });
    check();
  });
}
