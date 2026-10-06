/**
 * search.ts — client runtime for moss's full-text site search ("Quiet Palette").
 *
 * Ships only on pages where the build enabled search (`LayoutConfig.search` —
 * `[site].search` ∧ preview-features ∧ deployed), the same gate that emits the
 * `.nav-search-btn` in `build/components/nav.rs` and the Pagefind index under
 * `/_moss/pagefind/`.
 *
 * Shape of the interaction:
 *   trigger (nav pill, `/`, ⌘K/Ctrl+K)
 *     → top-anchored overlay at 18vh, translucent backdrop, fixed 18px radius
 *     → debounced Pagefind query (never mid-IME-composition)
 *     → flat hairline-separated rows, ↑/↓ with wraparound, Enter navigates
 *     → Esc closes and returns focus to the trigger
 *
 * Three invariants worth keeping:
 *
 * 1. **Zero network cost until first invoke.** `pagefind.js` is `import()`ed on
 *    the first open, never at load. The import specifier is built at runtime so
 *    esbuild leaves it alone instead of trying to bundle a file that only
 *    exists in the published output.
 * 2. **No query fires during IME composition.** A naive `input` listener sends
 *    a garbage pinyin fragment on every keystroke; `compositionstart` /
 *    `compositionend` gate the debounce so CJK authors get one query per word.
 * 3. **The trigger is bound by delegation, not by reference.** The preview's
 *    idiomorph pass replaces nav nodes and strips JS-appended ones, so a stored
 *    button reference goes stale. `moss-morph-patched` also drops the overlay
 *    reference if the morph removed it.
 */

// Rust-owned constants (bundle mount URL + palette strings) arrive via the
// generated module — `generate-artifacts -- search-runtime` — never hand-mirrored.
import { BUNDLE_PATH, STRINGS, type SearchStrings } from "./search.generated";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** Input debounce. Long enough to skip intermediate keystrokes, short enough to feel live. */
const DEBOUNCE_MS = 150;

/** Delay before the progress hairline appears, so fast queries never flash it. */
const PROGRESS_DELAY_MS = 200;

/** Result rows rendered per query. */
const MAX_RESULTS = 10;

// ---------------------------------------------------------------------------
// Pagefind's runtime API (v1.5.x) — declared locally; the module is loaded at
// runtime from the published bundle, so there is nothing to import types from.
// ---------------------------------------------------------------------------

interface PagefindResultData {
  url: string;
  excerpt: string;
  meta?: { title?: string } & Record<string, string | undefined>;
}

interface PagefindResult {
  id: string;
  data: () => Promise<PagefindResultData>;
}

interface PagefindApi {
  options: (opts: { baseUrl?: string }) => Promise<void>;
  init: () => Promise<void>;
  search: (query: string) => Promise<{ results: PagefindResult[] }>;
}

/** Resolve `<html lang>` the same way `Language::from_bcp47` does: any zh-* by region. */
function resolveStrings(): SearchStrings {
  const tag = (document.documentElement.lang || "en").toLowerCase();
  if (!tag.startsWith("zh")) return STRINGS.en;
  if (/hant|tw|hk|mo/.test(tag)) return STRINGS["zh-hant"];
  return STRINGS["zh-hans"];
}

const S = resolveStrings();

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

let overlay: HTMLElement | null = null;
let input: HTMLInputElement | null = null;
let statusEl: HTMLElement | null = null;
let listEl: HTMLElement | null = null;
let progressEl: HTMLElement | null = null;

let isOpen = false;
let isComposing = false;
let selectedIndex = -1;
let rows: HTMLAnchorElement[] = [];

let debounceTimer: number | undefined;
let progressTimer: number | undefined;
/** Monotonic query id — a slow query that resolves after a newer one is discarded. */
let queryToken = 0;

let pagefind: PagefindApi | null = null;
let pagefindLoad: Promise<PagefindApi | null> | null = null;
/** Bumped on every morph; cache-busts the `pagefind.js` import after a reindex. */
let indexVersion = 0;

/** Element focus returns to on close (the trigger that opened the panel). */
let returnFocusTo: HTMLElement | null = null;
/** Saved inline `overflow` so close restores exactly what the page had. */
let savedOverflow: string | null = null;

// ---------------------------------------------------------------------------
// Pagefind loading
// ---------------------------------------------------------------------------

/**
 * Load (and memoize) the Pagefind runtime.
 *
 * **A failed load is never memoized.** In preview the index can appear *after*
 * the page is already live — the author enables `[site].search`, or the
 * background reindex lands a moment after the rebuild — so the first open may
 * legitimately 404. Caching that `null` would leave search dead for the rest of
 * the preview session even once a perfectly good index exists. Only a
 * successful load is remembered; a failure clears the memo so the next open
 * retries.
 *
 * The cache-busting query string matters for the same reason: a browser that
 * negative-cached `pagefind.js` (or cached the pre-reindex copy, whose entry
 * manifest names content-hashed chunks that no longer exist) would otherwise
 * keep serving it back. `indexVersion` bumps on every morph — see the
 * `moss-morph-patched` handler.
 */
function loadPagefind(): Promise<PagefindApi | null> {
  if (pagefindLoad) return pagefindLoad;
  const attempt = indexVersion;
  // Runtime-computed specifier: keeps esbuild from resolving (and failing on)
  // a module that only exists in the published build output.
  const specifier = new URL(BUNDLE_PATH + "pagefind.js", location.href).href;
  const url = attempt > 0 ? `${specifier}?v=${attempt}` : specifier;
  pagefindLoad = (async () => {
    try {
      const mod = (await import(/* @vite-ignore */ url)) as unknown as PagefindApi;
      // Without this, every result URL grows a phantom `/_moss/` prefix:
      // Pagefind guesses the site base from where its own bundle loaded, via
      // `^(.*\/)_?pagefind` — matched against moss's mount `/_moss/pagefind/`
      // the greedy group yields `/_moss/`. moss sites are built root-relative
      // (BUNDLE_PATH is root-absolute for the same reason), so pin it.
      await mod.options({ baseUrl: "/" });
      await mod.init();
      // Guarded for the same reason as the failure branch: a reindex may have
      // landed while this import was in flight, and this module's entry
      // manifest now names chunk files that were renamed. Publishing it would
      // wire search to dead chunks with no failure to retry from.
      if (attempt === indexVersion) pagefind = mod;
      return mod;
    } catch (err) {
      console.warn("[moss] search index unavailable", url, err);
      // Retryable: drop the memo so a later open re-imports. Guarded on
      // `attempt` so a stale failure can't clear a newer in-flight load.
      if (attempt === indexVersion) {
        pagefindLoad = null;
        pagefind = null;
      }
      return null;
    }
  })();
  return pagefindLoad;
}

/**
 * Forget the loaded index so the next open re-imports it.
 *
 * Pagefind's entry manifest names content-hashed chunk files; a reindex writes
 * new names, so a long-lived preview page holding the old module requests
 * chunks that no longer exist. Dropping the module reference is the whole fix —
 * the next `loadPagefind()` re-imports under a fresh `?v=` and re-inits.
 */
function resetPagefind(): void {
  indexVersion += 1;
  pagefind = null;
  pagefindLoad = null;
}

// ---------------------------------------------------------------------------
// DOM construction (lazy — nothing is added to the page until the first open)
// ---------------------------------------------------------------------------

function buildOverlay(): HTMLElement {
  const root = document.createElement("div");
  root.className = "moss-search";
  root.id = "moss-search";
  root.hidden = true;

  const backdrop = document.createElement("div");
  backdrop.className = "moss-search__backdrop";
  root.appendChild(backdrop);

  const dialog = document.createElement("div");
  dialog.className = "moss-search__panel";
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");
  dialog.setAttribute("aria-label", S.label);

  const field = document.createElement("div");
  field.className = "moss-search__field";
  field.innerHTML =
    '<svg class="moss-search__field-icon" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" ' +
    'width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2.4" ' +
    'stroke-linecap="round" viewBox="0 0 32 32"><circle cx="14" cy="14" r="8.5"/>' +
    '<path d="m20.2 20.2 6.3 6.3"/></svg>';

  const field_input = document.createElement("input");
  field_input.className = "moss-search__input";
  field_input.type = "text";
  field_input.autocomplete = "off";
  field_input.spellcheck = false;
  field_input.placeholder = S.placeholder;
  // No data-tooltip inside a floating overlay (moss UI convention) — aria only.
  field_input.setAttribute("aria-label", S.label);
  field_input.setAttribute("role", "combobox");
  field_input.setAttribute("aria-expanded", "false");
  field_input.setAttribute("aria-controls", "moss-search-results");
  field_input.setAttribute("aria-autocomplete", "list");
  field.appendChild(field_input);
  dialog.appendChild(field);

  const progress = document.createElement("div");
  progress.className = "moss-search__progress";
  progress.hidden = true;
  dialog.appendChild(progress);

  const seam = document.createElement("div");
  seam.className = "moss-search__seam";
  dialog.appendChild(seam);

  const body = document.createElement("div");
  body.className = "moss-search__body";

  // Idle state shows nothing — the placeholder in the input is the prompt.
  const status = document.createElement("p");
  status.className = "moss-search__status";
  status.setAttribute("role", "status");
  status.hidden = true;
  body.appendChild(status);

  const list = document.createElement("ul");
  list.className = "moss-search__results";
  list.id = "moss-search-results";
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-label", S.resultsLabel);
  body.appendChild(list);

  dialog.appendChild(body);
  root.appendChild(dialog);

  // Backdrop click (and any click that lands outside the panel) closes.
  root.addEventListener("mousedown", (e) => {
    if (!dialog.contains(e.target as Node)) close();
  });

  field_input.addEventListener("compositionstart", () => {
    isComposing = true;
  });
  field_input.addEventListener("compositionend", () => {
    isComposing = false;
    scheduleQuery();
  });
  field_input.addEventListener("input", () => {
    // Mid-composition `input` events carry pinyin fragments, not a query.
    if (isComposing) return;
    scheduleQuery();
  });

  root.addEventListener("keydown", onPanelKeydown);

  overlay = root;
  input = field_input;
  statusEl = status;
  listEl = list;
  progressEl = progress;

  document.body.appendChild(root);
  return root;
}

function ensureOverlay(): HTMLElement {
  if (overlay && overlay.isConnected) return overlay;
  return buildOverlay();
}

// ---------------------------------------------------------------------------
// Open / close
// ---------------------------------------------------------------------------

function open(trigger: HTMLElement | null): void {
  if (isOpen) return;
  const root = ensureOverlay();
  returnFocusTo = trigger;
  isOpen = true;
  root.hidden = false;
  savedOverflow = document.documentElement.style.overflow;
  document.documentElement.style.overflow = "hidden";
  input?.focus();
  input?.select();
  // Warm the index while the user is still reaching for the first key.
  void loadPagefind();
}

function close(): void {
  if (!isOpen) return;
  isOpen = false;
  if (overlay) overlay.hidden = true;
  document.documentElement.style.overflow = savedOverflow ?? "";
  savedOverflow = null;
  clearProgress();
  window.clearTimeout(debounceTimer);
  // Closing mid-composition (Esc, backdrop click) doesn't reliably fire
  // compositionend — without this the next open's `input` handler stays gated
  // forever, since nothing else clears the flag.
  isComposing = false;
  returnFocusTo?.focus();
  returnFocusTo = null;
}

// ---------------------------------------------------------------------------
// Query pipeline
// ---------------------------------------------------------------------------

function scheduleQuery(): void {
  window.clearTimeout(debounceTimer);
  debounceTimer = window.setTimeout(runQuery, DEBOUNCE_MS);
}

function runQuery(): void {
  const query = (input?.value ?? "").trim();
  const token = ++queryToken;

  if (!query) {
    clearProgress();
    renderIdle();
    return;
  }

  // Only genuine chunk-fetch latency shows the hairline, and only past 200ms.
  window.clearTimeout(progressTimer);
  progressTimer = window.setTimeout(() => {
    if (token === queryToken && progressEl) progressEl.hidden = false;
  }, PROGRESS_DELAY_MS);

  void (async () => {
    const api = pagefind ?? (await loadPagefind());
    if (token !== queryToken) return;
    if (!api) {
      clearProgress();
      renderEmpty(query);
      return;
    }
    let data: PagefindResultData[];
    try {
      const search = await api.search(query);
      data = await Promise.all(search.results.slice(0, MAX_RESULTS).map((r) => r.data()));
    } catch (err) {
      console.warn("[moss] search failed", err);
      if (token === queryToken) {
        clearProgress();
        renderEmpty(query);
      }
      return;
    }
    if (token !== queryToken) return;
    clearProgress();
    if (data.length === 0) {
      renderEmpty(query);
      return;
    }
    renderResults(data);
  })();
}

function clearProgress(): void {
  window.clearTimeout(progressTimer);
  if (progressEl) progressEl.hidden = true;
}

// ---------------------------------------------------------------------------
// Rendering — idle / empty / results share the same vertical slot so the panel
// never jumps between states.
// ---------------------------------------------------------------------------

function resetList(): void {
  rows = [];
  selectedIndex = -1;
  if (listEl) listEl.replaceChildren();
  input?.removeAttribute("aria-activedescendant");
  input?.setAttribute("aria-expanded", "false");
}

function renderIdle(): void {
  resetList();
  if (statusEl) {
    statusEl.hidden = true;
    statusEl.textContent = "";
  }
}

function renderEmpty(query: string): void {
  resetList();
  if (statusEl) {
    statusEl.hidden = false;
    statusEl.textContent = S.empty.replace("{query}", query);
  }
}

/** The Han ranges the indexer segments — mirrors `is_han` in build/feeds/search.rs. */
const HAN =
  "\\u{3400}-\\u{4DBF}\\u{4E00}-\\u{9FFF}\\u{F900}-\\u{FAFF}" +
  "\\u{20000}-\\u{2A6DF}\\u{2A700}-\\u{2EBEF}\\u{2F800}-\\u{2FA1F}";

/**
 * What may sit on the *other* side of a removable space: Han itself, CJK
 * punctuation (`。`/`「`/`（`…, fullwidth punctuation only — NOT fullwidth
 * alphanumerics ＡＢＣ/１２３, which carry authored spaces like Latin does,
 * and NOT U+3000, which is itself whitespace), and kana including the
 * halfwidth-katakana block (a kanji↔kana boundary space is never authored
 * Japanese).
 */
const HAN_NEIGHBOR =
  HAN +
  "\\u{3001}-\\u{303F}\\u{3040}-\\u{30FF}\\u{31F0}-\\u{31FF}" +
  "\\u{FF01}-\\u{FF0F}\\u{FF1A}-\\u{FF20}\\u{FF3B}-\\u{FF40}\\u{FF5B}-\\u{FF60}\\u{FF66}-\\u{FF9F}\\u{FFE0}-\\u{FFE6}";

/**
 * A removable space needs Han on at least one side — `segment_han_runs` only
 * ever inserts spaces at Han-run boundaries, so that is the exact inverse.
 * Two passes (Han on the left; Han on the right), each looking through
 * Pagefind's `<mark>`/`</mark>` wrappers. No lookbehind (Safari <16.4 fails at
 * parse time, taking the whole script with it): the left context is consumed
 * and restored via `$1`; consecutive joins still chain because the right
 * character is matched by lookahead only.
 */
const SPACE_AFTER_HAN = new RegExp(
  `([${HAN}](?:</?mark>)*) (?=(?:</?mark>)*[${HAN_NEIGHBOR}])`,
  "gu",
);
const SPACE_BEFORE_HAN = new RegExp(
  `([${HAN_NEIGHBOR}](?:</?mark>)*) (?=(?:</?mark>)*[${HAN}])`,
  "gu",
);

/**
 * Undo the indexer's jieba word segmentation for display.
 *
 * The index is built from a copy of each page whose Han runs were re-joined
 * with spaces (`segment_han_runs`) so the non-`extended` Pagefind build gets
 * CJK word boundaries. Pagefind stores that segmented text as the page's
 * content, so excerpts — and titles, captured from the segmented copy's
 * `<h1>` — arrive reading "這 是 一段 文字". Segmentation only ever *inserted*
 * single spaces at Han-run boundaries, and Pagefind whitespace-normalizes the
 * digest, so removing a space with Han on one side and a Han/kana/CJK-punct
 * neighbour on the other is the exact inverse. Spaces between Han and
 * Latin/digits/ASCII punctuation stay: the boundary rule inserts those too,
 * but authored text legitimately has them ("河流 river", "文字 (note)")
 * and mixed-script spacing is the conventional CJK typography anyway.
 */
export function joinSegmentedHan(text: string): string {
  return text.replace(SPACE_AFTER_HAN, "$1").replace(SPACE_BEFORE_HAN, "$1");
}

/**
 * Pagefind's excerpt is plain indexed text (HTML-entity-decoded at index time — see
 * pagefind's `fossick`) with `<mark>…</mark>` spliced in around matched words by its own
 * client JS. It is NOT safe HTML: any literal "<"/">" that was visible text on the source
 * page (a code sample, a stray "<3") survives into the excerpt unescaped. Never innerHTML
 * this — split on the exact `<mark>…</mark>` wrapper and route everything else through
 * textContent so the only element ever created from indexed content is <mark>, and its
 * contents are always text, never parsed markup.
 */
export function renderExcerpt(container: HTMLElement, excerpt: string): void {
  const parts = joinSegmentedHan(excerpt).split(/(<mark>[\s\S]*?<\/mark>)/g);
  for (const part of parts) {
    const match = /^<mark>([\s\S]*)<\/mark>$/.exec(part);
    if (match) {
      const mark = document.createElement("mark");
      mark.textContent = match[1];
      container.appendChild(mark);
    } else if (part) {
      container.appendChild(document.createTextNode(part));
    }
  }
}

function renderResults(data: PagefindResultData[]): void {
  resetList();
  if (statusEl) statusEl.hidden = true;
  if (!listEl) return;

  const frag = document.createDocumentFragment();
  data.forEach((item, i) => {
    const li = document.createElement("li");
    li.className = "moss-search__row";
    li.setAttribute("role", "presentation");

    const a = document.createElement("a");
    a.className = "moss-search__link";
    a.id = `moss-search-result-${i}`;
    a.href = item.url;
    a.setAttribute("role", "option");
    a.setAttribute("aria-selected", "false");

    const title = document.createElement("span");
    title.className = "moss-search__title";
    title.textContent = joinSegmentedHan(item.meta?.title || item.url);
    a.appendChild(title);

    const excerpt = document.createElement("span");
    excerpt.className = "moss-search__excerpt";
    renderExcerpt(excerpt, item.excerpt ?? "");
    a.appendChild(excerpt);

    a.addEventListener("mousemove", () => select(i, false));
    li.appendChild(a);
    frag.appendChild(li);
    rows.push(a);
  });
  listEl.appendChild(frag);
  input?.setAttribute("aria-expanded", "true");
  select(0, false);
}

function select(index: number, scroll = true): void {
  if (rows.length === 0) return;
  const next = ((index % rows.length) + rows.length) % rows.length;
  if (next === selectedIndex) return;
  rows[selectedIndex]?.classList.remove("is-selected");
  rows[selectedIndex]?.setAttribute("aria-selected", "false");
  selectedIndex = next;
  const el = rows[selectedIndex];
  el.classList.add("is-selected");
  el.setAttribute("aria-selected", "true");
  input?.setAttribute("aria-activedescendant", el.id);
  if (scroll) el.scrollIntoView({ block: "nearest" });
}

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------

function onPanelKeydown(e: KeyboardEvent): void {
  // During IME composition (candidate window open), Escape/↑/↓/Enter are the
  // candidate picker's keys, not the palette's — WebKit and Chrome both still
  // dispatch keydown for them (isComposing: true). Let the IME have them, or
  // a pinyin candidate pick doubles as "close the palette" / "move selection".
  if ((isComposing || e.isComposing) && e.key !== "Tab") return;
  switch (e.key) {
    case "Escape":
      e.preventDefault();
      close();
      return;
    case "ArrowDown":
      if (rows.length === 0) return;
      e.preventDefault();
      select(selectedIndex + 1);
      return;
    case "ArrowUp":
      if (rows.length === 0) return;
      e.preventDefault();
      select(selectedIndex - 1);
      return;
    case "Enter": {
      const target = rows[selectedIndex];
      if (!target) return;
      e.preventDefault();
      location.href = target.href;
      return;
    }
    case "Tab": {
      // Focus trap: the panel's focusables are the input plus the result links.
      const focusables: HTMLElement[] = input ? [input, ...rows] : [...rows];
      if (focusables.length === 0) return;
      const current = focusables.indexOf(document.activeElement as HTMLElement);
      const next = e.shiftKey
        ? (current <= 0 ? focusables.length - 1 : current - 1)
        : (current === focusables.length - 1 ? 0 : current + 1);
      e.preventDefault();
      focusables[next].focus();
      return;
    }
  }
}

/** True when the keystroke belongs to whatever the user is typing into. */
function isTypingContext(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    el.isContentEditable === true
  );
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

// Delegated: the preview's morph pass swaps nav nodes, so binding the button
// directly would go stale after the first rebuild.
document.addEventListener("click", (e) => {
  const trigger = (e.target as HTMLElement | null)?.closest?.(".nav-search-btn");
  if (!trigger) return;
  e.preventDefault();
  open(trigger as HTMLElement);
});

document.addEventListener("keydown", (e) => {
  if (isOpen) return;
  if (isTypingContext(e.target)) return;
  const isCmdK = (e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K");
  const isSlash = e.key === "/" && !e.metaKey && !e.ctrlKey && !e.altKey;
  if (!isCmdK && !isSlash) return;
  if (!document.querySelector(".nav-search-btn")) return;
  e.preventDefault();
  open(document.querySelector<HTMLElement>(".nav-search-btn"));
});

// idiomorph strips JS-appended nodes on a preview rebuild. Drop the stale
// references so the next open rebuilds the overlay from scratch.
document.addEventListener("moss-morph-patched", () => {
  // Unconditional, and deliberately outside the overlay guard below: a morph
  // means the site rebuilt, which means the index may have been rewritten
  // under us. The overlay may well still be connected — a reindex is not a
  // DOM change — but the loaded module is stale either way.
  resetPagefind();
  if (overlay && !overlay.isConnected) {
    if (isOpen) {
      isOpen = false;
      document.documentElement.style.overflow = savedOverflow ?? "";
      savedOverflow = null;
      returnFocusTo = null;
    }
    // Same gate close() clears: a morph mid-composition doesn't fire
    // compositionend, so without this the next open's `input` handler
    // stays gated forever.
    isComposing = false;
    overlay = null;
    input = null;
    statusEl = null;
    listEl = null;
    progressEl = null;
    rows = [];
    selectedIndex = -1;
  }
});
