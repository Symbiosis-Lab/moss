/**
 * Folder-mode card handlers — the onboarding cards.
 *
 *
 * The handlers here run in the editor webview when the user clicks a
 * folder-mode card. They live as a separate module so editor-main.ts
 * stays focused on the file-mode editor lifecycle; folder-mode card
 * actions are a parallel concern with its own surface.
 *
 * Wiring:
 *   - "Start writing" → `onboarding_start_writing` Tauri command creates
 *     `<folder>/index.md` (idempotent), then we open the file via the
 *     provided `loadFile` callback. The editor lifecycle unmounts folder
 *     mode at the top of `loadFile` so the transition is bookkeeping-free
 *     from this side.
 *   - "Try an example" → opens the host's `starterPicker` slot over the cards.
 *   - "URL" → opens an inline URL input; on submit, calls
 *     `start_native_import_task` which spawns a (ActionPanel, Import,
 *     Ambient) PanelTask and drives scrape_to_folder — progress routes to
 *     the breadcrumb hairline. Zero-result is inline on the card.
 *   - "Plugin" → invokes the `open_plugins_view` Tauri command, which
 *     emits `MossEvent::OpenPluginsView` to the **main webview only**
 *     (via `emit_to("main", …)`). The app shell's listener opens the
 *     settings modal at the channels section. We can't use a
 *     `CustomEvent` — Tauri webviews are isolated `Window`s, so a
 *     `window.dispatchEvent` in the editor webview never reaches the
 *     main webview. Always `emit_to()`, never `app.emit()` broadcast.
 *
 * Save badges: every transition spawns a PanelTask whose `(scope, tone,
 * kind)` matches the action — the task renderers paint those automatically.
 * This module renders no badges itself; it just produces the events.
 */

import { t } from "../../i18n/index";
import { showToast, showAck } from "../../shared/ui/toast/toast-manager";
import { runAction, BUSY_LABEL_ATTR } from "../../shared/ui/run-action";
import type { FolderCardKind } from "./folder-mode";
import { mountFolderMode, onFolderModeUnmount } from "./folder-mode";
import { host } from "../../host";
import { engine } from "../../engine";

// ── Public surface ─────────────────────────────────────────────────────

/**
 * Dependencies the card handler set needs from `editor-main.ts`. Passing
 * these through avoids `folder-mode-handlers` reaching back into
 * editor-main module state (which keeps testing tractable).
 */
export interface CardHandlerDeps {
  /** Absolute path of the open folder. */
  folderPath: string;
  /** Display name of the folder (the basename of `folderPath`). */
  folderName: string;
  /** Uses the folder recipe for a nested folder inside the open vault. */
  createHome?: () => Promise<void>;
  /** Reveal and open created/imported content through the shared new-entry action. */
  loadFile: (relativePath: string) => Promise<unknown>;
  /** Host element rendering folder mode (passed back to mount on rollback). */
  host: HTMLElement;
}

/**
 * Build the `onCardClick` callback to pass into `mountFolderMode`. The
 * returned function routes the kind to the matching async handler.
 */
export function makeCardClickHandler(
  deps: CardHandlerDeps,
): (kind: FolderCardKind) => void {
  return (kind: FolderCardKind) => {
    // Fire-and-forget; `dispatchCard` owns the busy paint and the failure
    // toast for every card. We don't want to make `onCardClick` async
    // (it's a synchronous event-bus signal in the folder-mode contract), so
    // we wrap each click in `void`.
    void dispatchCard(kind, deps);
  };
}

/** The work behind each card, keyed by the kind the click carries. */
function cardWork(
  kind: FolderCardKind,
  deps: CardHandlerDeps,
): () => Promise<void> {
  switch (kind) {
    case "start-writing":
      return () => handleStartWriting(deps);
    case "try-example":
      return () => handleTryExample(deps);
    case "import-url":
      return () => handleImportUrl(deps);
    case "connect-plugin":
      return () => handleConnectPlugin(deps);
  }
}

/**
 * Run a card's work under the instant-feedback rule.
 * The clicked card goes
 * busy in the click's own frame, before any I/O, and every exit path — success,
 * error result, throw — leaves the user either on a new screen or looking at a
 * message. This is the only place the cards need it, so it lives here
 * rather than in each handler.
 *
 * The visible latency this covers is real: "Start writing" creates the self-named root home
 * on disk and mounts CM6 over it, and on an iCloud-backed or cold folder that
 * is seconds on the first screen a new user ever sees.
 */
export async function dispatchCard(
  kind: FolderCardKind,
  deps: CardHandlerDeps,
): Promise<void> {
  const card = deps.host.querySelector<HTMLElement>(
    `[data-folder-mode-card="${kind}"]`,
  );
  // The URL card is the one card whose click effect IS its paint:
  // `handleImportUrl` expands it in place, synchronously, in the click's own
  // frame. `.moss-is-busy` dims the whole card, and `opacity` applies to
  // descendants — so the busy paint would grey out the input the click just
  // revealed. That card takes the guard and skips the affordance.
  const trigger = kind === "import-url" ? null
    : card ?? (kind === "start-writing" ? deps.host.querySelector<HTMLElement>("[data-pane-state-cta]") : null);
  // A card is `<button><span title><span desc></button>` — two element
  // children, so `runAction` has no unambiguous text node to swap. Marking the
  // title is what turns "Start writing" into "Creating…" for the round trip.
  // The attribute is inert; the card markup itself belongs to folder-mode.ts.
  trigger
    ?.querySelector(".moss-folder-mode__card-title")
    ?.setAttribute(BUSY_LABEL_ATTR, "");

  await runAction({
    trigger,
    // Guards the two non-element entry points too (and the URL card, which
    // passes no trigger), so an impatient double-click can't run the work
    // twice.
    key: `folder-card:${kind}`,
    busyLabel:
      kind === "start-writing"
        ? t('onboard.flow.card_creating')
        : t('status.working'),
    run: async () => {
      try {
        await cardWork(kind, deps)();
      } catch (err) {
        // Logged here, at the throw site, and rethrown so the toast below is
        // still the user's version of the same event. A log is not feedback.
        console.error(`[folder-mode-handlers] ${kind} failed:`, err);
        throw err;
      }
    },
    // Success is self-evidencing on every card: a file opens, the picker takes
    // over the grid, the URL form expands, the plugins view acks itself.
    failMessage: (err) => t('onboard.flow.error_could_not_complete', { err }),
    // Nothing to restore once the card is off the DOM — folder mode unmounts
    // under `loadFile`, and the starter picker replaces the whole grid.
    stale: () => trigger !== null && !trigger.isConnected,
  });
}

// ── Start writing ──────────────────────────────────────────────────────

export async function handleStartWriting(deps: CardHandlerDeps): Promise<void> {
  if (deps.createHome) { await deps.createHome(); return; }
  if (!host.startWriting) throw new Error("this host cannot start a page here");
  const result = await host.startWriting(deps.folderPath);
  if (result.status === "error") {
    throw new Error(result.error);
  }
  const absolute = result.data;
  // editor-main.ts uses project-root-relative paths in its registry; pass
  // the path as-is and let `loadFile`'s `toRelative` adapt. The relative
  // path is identical to the filename when the file sits at the root.
  const relative = relativeTo(deps.folderPath, absolute);
  await deps.loadFile(relative);
}

// ── Starter ────────────────────────────────────────────────────────────
//
// Try an example opens the host's `starterPicker` slot; a host without one
// shows no card for it.

export async function handleTryExample(deps: CardHandlerDeps): Promise<void> {
  await host.starterPicker?.open({
    folderPath: deps.folderPath,
    folderName: deps.folderName,
    host: deps.host,
    loadHome: (absolute) => deps.loadFile(relativeTo(deps.folderPath, absolute)),
    back: () => mountFolderMode(deps.host, {
      folderPath: deps.folderPath,
      folderName: deps.folderName,
      isEmpty: true,
      onCardClick: makeCardClickHandler(deps),
    }),
  });
}

// ── URL import ────────────────────────────────────────────────────────

const URL_HOST_ATTR = "data-folder-mode-url";

/**
 * Lifecycle of the inline-URL card's transient listeners (input/submit/
 * keydown + the document-level capture-phase click-outside). All are
 * bound to this controller's `signal`; aborting it removes every one
 * deterministically in a single call.
 *
 * Why a single module-level controller (not a per-expand local): the
 * card can be re-expanded (zero-result re-render) and collapsed via
 * three independent paths (Esc, click-outside, folder-mode unmount).
 * A module-level handle lets every collapse/teardown path abort the
 * one live set without threading the controller through call sites.
 * Each `wireUrlCard` aborts any prior controller before installing a
 * fresh one, so expand→Esc→expand→Esc cycles never stack listeners.
 */
let urlCardAbort: AbortController | null = null;
/** Disposer that unregisters the unmount teardown for the live card. */
let urlCardUnmountDisposer: (() => void) | null = null;

/**
 * Tear down the inline-URL card's listeners. Safe to call repeatedly and
 * from any path (Esc, click-outside, unmount, re-expand). After this no
 * URL-card listener remains on the document or the (possibly detached)
 * card node.
 */
function teardownUrlCardListeners(): void {
  if (urlCardAbort) {
    urlCardAbort.abort();
    urlCardAbort = null;
  }
  if (urlCardUnmountDisposer) {
    urlCardUnmountDisposer();
    urlCardUnmountDisposer = null;
  }
}

/**
 * URL import — inline-expand interaction.
 *
 * Ported from the (removed) preview empty-state overlay's inline-URL form
 * (`overlay-templates.ts` `#url-import-row` / `#scrape-form` +
 * `preview-manager.ts` `setupEmptySiteFormHandler`), adapted to
 * folder-mode's BEM class system. The URL card morphs in place:
 *
 *   - label (title + description) ↔ input crossfade
 *   - arrow (›) ↔ "Import" button morph
 *   - Esc collapses; click-outside collapses
 *   - inline `.moss-folder-mode__url-error` row for an invalid URL
 *
 * The other three cards stay visible the whole time (no full-grid swap),
 * matching A's expand-in-place behavior. KEEPS B's backend path:
 * `importFromUrl` calls `start_native_import_task` (routes progress to the import panel)
 * + the zero-result ghost-text re-render + `pickLandingFile` (NOT A's
 * `startScraping`, which lacks ghost-text / landing-file behavior).
 *
 * Data attributes (`-input`, `-submit`, `-back`) are preserved so the
 * onboarding e2e suite can drive the form.
 */
export async function handleImportUrl(deps: CardHandlerDeps): Promise<void> {
  const card = deps.host.querySelector<HTMLElement>(
    '[data-folder-mode-card="import-url"]',
  );
  if (!card) return;
  // Already expanded? A second card-level click (e.g. bubbling up from
  // the input/submit) must NOT re-render and wipe the user's input.
  if (card.classList.contains("moss-folder-mode__card--url-expanded")) return;
  expandUrlCard(card, "", deps);
}

/**
 * Replace the URL card's contents with the expandable inline-form markup
 * and wire its interactions. `ghostText` seeds the input placeholder
 * (used by the zero-result re-render to show the failed URL as ghost
 * text per spec § "URL import — zero-result case").
 */
function expandUrlCard(
  card: HTMLElement,
  ghostText: string,
  deps: CardHandlerDeps,
): void {
  card.classList.add("moss-folder-mode__card--url-expanded");
  card.setAttribute("aria-expanded", "true");
  card.innerHTML = renderUrlCardInner(ghostText);
  wireUrlCard(card, deps);
}

/**
 * Inner markup for the expanded URL card. The label and the inline form
 * are stacked (absolutely positioned) so they crossfade; the arrow and
 * the Import button share the trailing slot and morph between each other
 * via the `--url-expanded` modifier (CSS in folder-mode.css).
 */
function renderUrlCardInner(ghostText: string): string {
  const placeholder = ghostText || "https://example.com/post";
  return `
    <div class="moss-folder-mode__url-row">
      <div class="moss-folder-mode__url-middle">
        <div class="moss-folder-mode__url-label" aria-hidden="true">
          <span class="moss-folder-mode__card-title">${t('onboard.card_url_title')}</span>
          <span class="moss-folder-mode__card-description">${t('onboard.card_url_desc')}</span>
        </div>
        <input
          type="url"
          ${URL_HOST_ATTR}-input
          class="moss-folder-mode__url-input"
          placeholder="${escapeAttr(placeholder)}"
          aria-label="${t('onboard.flow.url_input_aria')}"
          autocomplete="off"
          spellcheck="false" />
      </div>
      <span class="moss-folder-mode__url-right">
        <span class="moss-folder-mode__url-arrow" aria-hidden="true">&#x203A;</span>
        <button
          type="button"
          class="moss-folder-mode__url-submit"
          ${URL_HOST_ATTR}-submit>${t('onboard.flow.import_btn')}</button>
      </span>
    </div>
    <div class="moss-folder-mode__url-error" ${URL_HOST_ATTR}-error hidden></div>
  `;
}

/**
 * Collapse the URL card back to its plain button state (the four-card
 * grid is intact, so this just restores the card's button face).
 */
function collapseUrlCard(card: HTMLElement): void {
  // Deterministically remove every listener (input/submit/keydown +
  // document-level click-outside) for this card. Without this, the
  // click-outside listener self-removed only on the NEXT document click,
  // so rapid expand→Esc→expand→Esc cycles stacked capture-phase
  // listeners on the owner document.
  teardownUrlCardListeners();
  card.classList.remove("moss-folder-mode__card--url-expanded");
  card.setAttribute("aria-expanded", "false");
  card.innerHTML = `
    <span class="moss-folder-mode__card-title">${t('onboard.card_url_title')}</span>
    <span class="moss-folder-mode__card-description">${t('onboard.card_url_desc')}</span>
  `;
}

function wireUrlCard(card: HTMLElement, deps: CardHandlerDeps): void {
  const input = card.querySelector<HTMLInputElement>(`[${URL_HOST_ATTR}-input]`);
  const submit = card.querySelector<HTMLButtonElement>(`[${URL_HOST_ATTR}-submit]`);
  const errorEl = card.querySelector<HTMLElement>(`[${URL_HOST_ATTR}-error]`);
  if (!input || !submit) return;

  // Fresh controller for this expand. Abort any prior one first so a
  // re-expand (e.g. the zero-result re-render) never leaves the previous
  // card's listeners live. All listeners below carry `signal`, and the
  // unmount teardown aborts it, so an expand→unmount tears everything
  // down even without a collapse.
  teardownUrlCardListeners();
  const abort = new AbortController();
  urlCardAbort = abort;
  const { signal } = abort;
  urlCardUnmountDisposer = onFolderModeUnmount(() => {
    abort.abort();
    urlCardAbort = null;
    urlCardUnmountDisposer = null;
  });

  const showError = (msg: string) => {
    if (!errorEl) return;
    errorEl.textContent = msg;
    errorEl.hidden = false;
  };
  const clearError = () => {
    if (errorEl) errorEl.hidden = true;
  };

  const fire = async () => {
    const url = input.value.trim();
    if (!url) {
      // `focus()` alone on the already-focused input changes nothing on
      // screen, so the click reads as a dead button. Say what's missing, the
      // same way the invalid-URL branch below does.
      showError(t('onboard.flow.url_empty'));
      input.focus();
      return;
    }
    // Inline validation (ported from the overlay's form-submit handler).
    try {
      new URL(url);
    } catch {
      showError(t('onboard.flow.url_invalid'));
      input.focus();
      return;
    }
    clearError();
    input.disabled = true;
    submit.disabled = true;
    submit.textContent = t('onboard.flow.url_starting');
    try {
      await importFromUrl(url, deps);
    } catch {
      // One event, one home: import runs through a native PanelTask
      // (start_native_import_task). A scrape failure fires
      // handle.failed(e, /*recoverable=*/false) → a single persistent Blocking
      // error toast via task-toast-subscriber. Do NOT toast again here, or the same
      // failure double-signals. We only restore the card's input UI.
      input.disabled = false;
      submit.disabled = false;
      submit.textContent = t('onboard.flow.import_btn');
    }
  };

  submit.addEventListener("click", (e) => {
    // Don't let the click bubble to the card's own `data-folder-mode-card`
    // handler (which would re-run handleImportUrl).
    e.stopPropagation();
    void fire();
  }, { signal });
  // Clicks inside the expanded card (on the input) must not bubble to the
  // card-level handler either.
  input.addEventListener("click", (e) => e.stopPropagation(), { signal });
  input.addEventListener("input", clearError, { signal });
  input.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") {
      ev.preventDefault();
      void fire();
    } else if (ev.key === "Escape") {
      ev.preventDefault();
      // collapseUrlCard aborts `signal`, removing this and every other
      // URL-card listener (including the document click-outside). focus()
      // runs before the teardown's effects matter — it's not a listener.
      collapseUrlCard(card);
      card.focus();
    }
  }, { signal });

  // Click-outside collapses (ported from the overlay's document-click
  // handler). Bound on the card's owner document with the same `signal`,
  // so collapse / re-expand / unmount all remove it deterministically —
  // it no longer needs to self-remove on the next document click.
  const onDocClick = (e: MouseEvent) => {
    if (!card.classList.contains("moss-folder-mode__card--url-expanded")) {
      return;
    }
    if (!card.contains(e.target as Node)) {
      collapseUrlCard(card);
    }
  };
  // Capture phase so the collapse fires even if an inner element stops
  // propagation; defer registration a tick so the originating card-click
  // that triggered the expand doesn't immediately collapse it. The
  // `{ capture, signal }` options object removes the listener when the
  // controller aborts — no manual self-removal. Guard the deferred add
  // against an abort that happened during the tick.
  setTimeout(() => {
    if (!signal.aborted) {
      card.ownerDocument.addEventListener("click", onDocClick, {
        capture: true,
        signal,
      });
    }
  }, 0);

  // Auto-focus the input once the crossfade has started.
  setTimeout(() => input.focus(), 0);
}

/**
 * Calls `start_native_import_task` which drives `scrape_to_folder`
 * and routes progress to the import panel via a native PanelTask on
 * (ActionPanel, Import, Ambient). Zero-result is inline on the card.
 * On completion, opens the most-recently-dated imported post; if zero
 * files materialized, re-expands the URL card with ghost-text.
 */
async function importFromUrl(url: string, deps: CardHandlerDeps): Promise<void> {
  const hostname = (() => {
    try {
      return new URL(url).hostname;
    } catch {
      return url;
    }
  })();

  // Import progress routes to the import panel via a native PanelTask on
  // (ActionPanel, Import, Ambient) — the breadcrumb hairline is the
  // home for project work.
  // start_native_import_task spawns the task, drives scrape_to_folder,
  // and emits Running→Succeeded/Failed lifecycle events that the hairline
  // subscriber already handles.
  if (!host.importSite) throw new Error("this host cannot import a site");
  const result = await host.importSite(url, deps.folderPath);
  if (result.status === "error") {
    throw new Error(result.error);
  }
  if (!result.data.success) {
    throw new Error(result.data.error ?? "import failed");
  }
  const total = result.data.total_pages;
  if (total === 0) {
    // Zero-result variant: feedback is inline on the
    // re-expanded URL card, not a toast. The card re-expands with the failed
    // URL as ghost-text (placeholder) so the user can retype or click elsewhere.
    const card = deps.host.querySelector<HTMLElement>(
      '[data-folder-mode-card="import-url"]',
    );
    if (card) {
      expandUrlCard(card, url, deps);
      // Inject an inline status label below the input on the card.
      let statusEl = card.querySelector<HTMLElement>("[data-folder-mode-url-status]");
      if (!statusEl) {
        statusEl = document.createElement("div");
        statusEl.setAttribute("data-folder-mode-url-status", "");
        statusEl.className = "moss-folder-mode__url-status";
        card.appendChild(statusEl);
      }
      statusEl.textContent = t('onboard.flow.url_nothing_to_import', { hostname });
    }
    return;
  }
  // Success: pick the home file.
  const pick = await pickLandingFile(deps.folderPath);
  if (pick) {
    await deps.loadFile(pick);
  } else {
    showAck(t('onboard.flow.url_imported_pages', { total: String(total) }));
  }
}

/**
 * Pick the file to land on after an import:
 *   1. The most-recently-dated post (via `pickRecentFile`).
 *   2. The home file via `detect_home_file_in_folder` precedence
 *      (`index.md`, etc.) — surfaced through `listTree`.
 *   3. Alphabetically first markdown file.
 *
 * Returns a project-root-relative path or null when no file is found.
 *
 * Exported with the `_` prefix so the unit-test suite can exercise the
 * precedence chain in isolation (the production call site is private
 * to `importFromUrl` inside this module).
 */
export async function _pickLandingFile(folderPath: string): Promise<string | null> {
  return pickLandingFile(folderPath);
}

async function pickLandingFile(folderPath: string): Promise<string | null> {
  const tree = await engine.listTree(false);
  if (tree.status !== "ok") return null;
  const md: string[] = [];
  // The root node's `name` is the basename of `folderPath` ("site"). We
  // want paths relative to the folder, so walk the root's children with
  // an empty prefix rather than the root itself (which would prepend
  // "site/" to every entry and defeat the `"index.md"` step-2 match).
  const root = tree.data as unknown as TreeNodeShape;
  for (const c of root.children ?? []) {
    walkTree(c, "", md);
  }
  if (md.length === 0) return null;
  // Most-recently-dated post (via frontmatter `date`). The
  // editor exposes `parseFrontmatter` per file; do this lazily and
  // bail out if any single read fails.
  const dated: Array<{ path: string; date: string }> = [];
  for (const p of md) {
    try {
      const abs = `${folderPath}/${p}`;
      const fm = await engine.parseFrontmatter(abs);
      if (fm.status === "ok") {
        const dateRaw = (fm.data.frontmatter as Record<string, unknown>)?.date;
        if (typeof dateRaw === "string" && dateRaw.length > 0) {
          dated.push({ path: p, date: dateRaw });
        }
      }
    } catch {
      // Ignore — file may not be parseable yet (still being written).
    }
  }
  if (dated.length > 0) {
    dated.sort((a, b) => b.date.localeCompare(a.date));
    return dated[0].path;
  }
  // Home-file precedence — `index.md` first.
  const index = md.find((p) => p.toLowerCase() === "index.md");
  if (index) return index;
  for (const stem of ["readme.md", "_index.md", "main.md"]) {
    const hit = md.find((p) => p.toLowerCase() === stem);
    if (hit) return hit;
  }
  // Alphabetical fallback.
  md.sort();
  return md[0] ?? null;
}

interface TreeNodeShape {
  name: string;
  is_dir: boolean;
  path?: string;
  children?: TreeNodeShape[];
}

function walkTree(node: TreeNodeShape, prefix: string, out: string[]): void {
  if (!node.is_dir) {
    // Only `.md` files matter for landing-file detection.
    if (node.name.toLowerCase().endsWith(".md")) {
      out.push(prefix ? `${prefix}/${node.name}` : node.name);
    }
    return;
  }
  // Skip hidden dirs in the surface walk; the build pipeline does too.
  if (node.name.startsWith(".")) return;
  const next = prefix ? `${prefix}/${node.name}` : node.name;
  for (const c of node.children ?? []) {
    walkTree(c, next, out);
  }
}

// ── Plugin drill-down ─────────────────────────────────────────────────

export async function handleConnectPlugin(deps: CardHandlerDeps): Promise<void> {
  // Cross-webview routing. The settings/plugins surface lives in the
  // main webview, not the action panel — `window.dispatchEvent` does NOT
  // cross Tauri's `Window`-per-webview isolation, so we hop through Rust:
  //   editor webview JS → open_plugins_view IPC → app.emit_to("main", …)
  //                    → MossEvent::OpenPluginsView → app.ts listener
  //                    → settingsModal.open("channels")
  //
  // Per-plugin drill-down (e.g., Matters → triggers `process` hook) is
  // wired by the existing `installPlugin` path (see
  // the host plugin catalog): after install,
  // moss runs the bundled `process` hook in the background, which is
  // exactly what the import flow needs. This handler's responsibility
  // ends at "open the plugins view"; the plugin install flow takes over
  // and PanelTask events arrive via `report_plugin_task_lifecycle`.
  if (!host.openPluginsView) throw new Error("this host has no plugins view");
  const result = await host.openPluginsView("import", deps.folderPath);
  if (result.status === "error") {
    // Backstop: the IPC call should always succeed when moss is up. If
    // the emit_to fails (e.g., main webview labeled differently in a
    // future refactor), surface a toast so the click isn't silent.
    console.warn("[folder-mode-handlers] open_plugins_view failed:", result.error);
    showToast({ what: t('onboard.flow.plugins_open_failed'), severity: "NeedsAction" });
    return;
  }
  // Confirmation toast — the modal opens in the main webview and the
  // user's eyes haven't followed yet. The toast itself is rendered by
  // the main webview's toast manager (which is where toasts always land).
  showAck(t('onboard.flow.plugins_opening'));
}

// ── Helpers ───────────────────────────────────────────────────────────

function relativeTo(folder: string, absolute: string): string {
  // POSIX-style; the registry uses forward slashes regardless of platform.
  const norm = absolute.replace(/\\/g, "/");
  const root = folder.replace(/\\/g, "/").replace(/\/+$/, "");
  if (norm.startsWith(root + "/")) {
    return norm.slice(root.length + 1);
  }
  if (norm === root) {
    return "";
  }
  return norm;
}

function escapeText(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function escapeAttr(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
