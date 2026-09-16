/**
 * Pure guards shared by the iframe-bridge morph handler and its unit tests.
 *
 * The bridge is an un-importable IIFE, so its morph tests normally MIRROR its
 * logic in a local harness. This module lets the bridge and the tests exercise
 * the SAME code for the script-change check instead — esbuild inlines it into
 * the bridge bundle, and the tests import it directly.
 */

// A `<script>` only needs a RELOAD to take effect if the browser executes it as
// JavaScript. `type="application/ld+json"` (moss emits it per page, carrying the
// title/description/date), `application/json`, `importmap`, `speculationrules`,
// `text/template`, etc. are DATA blocks — idiomorph morphs their text in place
// and nothing has to re-run, so they must not count as a script change (else a
// plain title/description edit would force a reload on every keystroke).
const EXECUTABLE_TYPES = new Set([
  "",
  "module",
  "text/javascript",
  "application/javascript",
  "text/ecmascript",
  "application/ecmascript",
]);

function isExecutableJs(s: HTMLScriptElement): boolean {
  return EXECUTABLE_TYPES.has((s.getAttribute("type") ?? "").trim().toLowerCase());
}

/**
 * Stable identity for a `<script>`: its `src` ATTRIBUTE (external) or its inline
 * text (inline). We read the raw `src` attribute, NOT the resolved `.src` URL —
 * a `DOMParser` document has no base URI, so `.src` would resolve differently
 * from the live document and manufacture false diffs.
 *
 * Caveat: an external script at a STABLE (non-content-hashed) path whose bytes
 * change without a URL change is not detected — the key is unchanged, so the
 * edit is missed until a manual reload. moss's own scripts are content-hashed
 * (`.../script.{hash}.js`); this only affects author raw-HTML `<script src>`s.
 */
function scriptKey(s: HTMLScriptElement): string {
  const src = s.getAttribute("src");
  return src !== null ? `src:${src}` : `inline:${s.textContent ?? ""}`;
}

/**
 * Keys of the site's EXECUTABLE scripts — excluding moss-managed permanent
 * scripts (the bridge is `[data-moss-permanent]`, injected consistently into
 * both the live document and every served page) and non-JS data blocks.
 */
function siteScriptKeys(root: ParentNode): string[] {
  return Array.from(root.querySelectorAll("script"))
    .filter((s) => isExecutableJs(s) && !s.closest("[data-moss-permanent]"))
    .map(scriptKey);
}

/**
 * True when the freshly-built page (`next`) introduces an executable script the
 * live document (`live`) has never run — a changed content-hash `src`, or a
 * genuinely new script. That is the only case a morph cannot handle: it reuses
 * matched `<script>` nodes and never re-executes them (the bridge keystone), so
 * a changed script would silently NOT take effect. When true the bridge asks the
 * shell to fall back to a full reload, which re-runs every script.
 *
 * We check "a `next` key absent from `live`", NOT ordered equality, on purpose:
 * a site/theme may inject `<script>`s at runtime (analytics, embeds) that live
 * in `live` but never in the served bytes. Ordered/length comparison would then
 * diff on EVERY morph → a reload on every keystroke, destroying the scroll/focus
 * the morph exists to preserve. Ignoring live-only scripts also means a removed
 * served script is not caught — acceptable: its already-run code is harmless and
 * clears on the next real reload.
 */
export function scriptsDiffer(live: ParentNode, next: ParentNode): boolean {
  return differingScriptKeys(live, next).length > 0;
}

/**
 * The keys behind a [`scriptsDiffer`] verdict — every executable-script key the
 * freshly-built page has that the live document lacks, in document order.
 *
 * Exists because "script-changed → reload fallback" was undiagnosable: the log
 * named the verdict but never the script, so a reload firing on every rebuild
 * of an unchanged page could not be told apart from a real theme edit without
 * hand-diffing two generations of HTML. `scriptsDiffer` is this predicate's
 * emptiness check, so the two can never disagree about what tripped.
 */
export function differingScriptKeys(live: ParentNode, next: ParentNode): string[] {
  const liveKeys = new Set(siteScriptKeys(live));
  return siteScriptKeys(next).filter((key) => !liveKeys.has(key));
}

/**
 * A [`scriptKey`] rendered short enough to log.
 *
 * An external key is already a path and is passed through. An INLINE key is the
 * script's entire body — moss emits a ~5 KB subscribe bundle inline — so it is
 * reported as its byte length plus a whitespace-collapsed head. The length is
 * what distinguishes two inline scripts that share an opening line, which is
 * the case the head alone cannot resolve.
 */
export function describeScriptKey(key: string): string {
  const INLINE = "inline:";
  if (!key.startsWith(INLINE)) return key;
  const body = key.slice(INLINE.length);
  const head = body.replace(/\s+/g, " ").trim().slice(0, 80);
  return `inline[${body.length}B]:${head}`;
}

/**
 * Loggable descriptors for the scripts that force a reload, or `null` when a
 * morph can proceed. One call so the bridge's hot path stays a single branch —
 * the two 1400-line call sites the preview morph runs through are held flat by
 * the `prod_lines_per_file` ratchet, and this is where the logic belongs anyway.
 */
export function describeScriptChange(live: ParentNode, next: ParentNode): string[] | null {
  const keys = differingScriptKeys(live, next);
  return keys.length > 0 ? keys.map(describeScriptKey) : null;
}

/**
 * The attribute names `<body>` last carried FROM THE SERVER.
 *
 * Seeded by `captureServerBody()` at bridge INIT — not lazily on the first
 * morph. At init the document is still exactly the server's bytes; by the first
 * morph, site scripts have run and any class they added would be misread as the
 * server's and stripped.
 *
 * This is the whole trick. An `innerHTML` morph never touches its target's own
 * attributes, so `<body>`'s set was frozen at page load and the editor's Width
 * and Typesetting chips (`data-content-width` / `data-typesetting`, emitted by
 * shell.html from frontmatter) changed content but not layout until a reload.
 * Syncing them means stale ones have to be REMOVED — turning a chip off drops
 * the attribute entirely rather than changing its value.
 *
 * A blind "remove anything the new document lacks" removes runtime mutations
 * too. `frontend/site/fullscreen.ts` sets `body.style.overflow = "hidden"` to
 * lock background scroll while the lightbox is open, and
 * `frontend/site/immersive-mode.ts` adds `immersive-fs-active`; the server emits
 * neither, so a morph mid-lightbox would unlock scrolling under an open overlay.
 * Carrying a hand-written list of those forward is the include-list shape that
 * rots silently — the next runtime mutation anyone adds is a bug nobody sees.
 *
 * So: only unset what we previously set. Anything a runtime mutation introduced
 * was never in this set and is therefore never removed, without naming it.
 */
let serverBodyAttrs: Set<string> | null = null;
/** The classes `<body>` last carried from the server, same rationale. */
let serverBodyClasses: Set<string> | null = null;

const classesOf = (el: Element): string[] =>
  (el.getAttribute("class") ?? "").split(/\s+/).filter(Boolean);

/**
 * Reconcile `<body>`'s own attributes toward the freshly-fetched document.
 *
 * `class` is merged rather than overwritten: it is the one attribute the server
 * and the runtime both write, so classes the runtime added — those the previous
 * SERVER value did not contain — are carried on top of the new server value.
 */
export function syncBodyAttributes(next: HTMLElement, live: HTMLElement = document.body): void {
  // A missing memory means `captureServerBody()` never ran. Removing nothing is
  // the safe read: a stale attribute is a cosmetic miss, stripping a runtime one
  // breaks an open overlay.
  const prevAttrs = serverBodyAttrs ?? new Set<string>();
  const prevClasses = serverBodyClasses ?? new Set(classesOf(next));

  for (const name of prevAttrs) {
    if (name !== "class" && !next.hasAttribute(name)) live.removeAttribute(name);
  }
  for (const attr of Array.from(next.attributes)) {
    if (attr.name === "class") continue;
    if (live.getAttribute(attr.name) !== attr.value) live.setAttribute(attr.name, attr.value);
  }

  const nextClasses = classesOf(next);
  const runtimeClasses = Array.from(live.classList).filter((c) => !prevClasses.has(c));
  const merged = Array.from(new Set([...nextClasses, ...runtimeClasses]));
  if (merged.length) live.setAttribute("class", merged.join(" "));
  else live.removeAttribute("class");

  serverBodyAttrs = new Set(Array.from(next.attributes, (a) => a.name));
  serverBodyClasses = new Set(nextClasses);
}

/**
 * Record what the server put on `<body>`. Call once at bridge init, before any
 * site script has had a chance to mutate it.
 */
export function captureServerBody(live: HTMLElement = document.body): void {
  serverBodyAttrs = new Set(Array.from(live.attributes, (a) => a.name));
  serverBodyClasses = new Set(classesOf(live));
}

/** Test seam: forget the remembered server attribute set. */
export function resetBodyAttributeMemory(): void {
  serverBodyAttrs = null;
  serverBodyClasses = null;
}
