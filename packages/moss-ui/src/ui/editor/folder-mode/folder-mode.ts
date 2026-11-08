/**
 * FolderMode — the editor view's "folder root" presentation.
 *
 * Per the onboarding spec:
 *
 *   Folder mode = file mode + (in the populated state) a site-header band.
 *
 * Two states, deliberately different chrome:
 *
 *   - Empty folder (`articles.is_empty()`): bare — just the cards
 *     (Start writing, and those the host provides of Try an example /
 *     Import from URL / Connect a service) on the blueprint-grid canvas. No site-header band.
 *   - Populated folder: a site-header band sitting above the existing CM6 on
 *     the home file (per `home.rs` precedence).
 *
 * The site-header band carries the "your folder name is your site name"
 * teaching moment and the Light/Dark toggle. It renders in the POPULATED
 * state only; keeping the empty state bare avoids competing controls. The
 * microcopy text is spec-verbatim — `FOLDER_MICROCOPY` is the authoritative
 * string and the populated-state tests assert against it.
 *
 * DOM contract: render into a host element (typically the
 * `.moss-editor-cm` slot when no file is selected). The component takes
 * the slot, replaces its contents, and provides an `unmount()` to put
 * the slot back to a clean state before CM6 takes over.
 *
 * @module folder-mode
 */

import { showToast } from "../../shared/ui/toast/toast-manager";
import { host as uiHost } from "../../host";
import { startBlueprintGrid } from "@symbiosis-lab/moss-blueprint-grid";
import { t } from "../../i18n/index";
import type { UiStringMap } from "../../i18n/index";

// ── Public types ────────────────────────────────────────────────────────

/**
 * The card kinds shown when the folder is empty.
 */
export type FolderCardKind =
  | "start-writing"
  | "try-example"
  | "import-url"
  | "connect-plugin";

export interface FolderModeOptions {
  /** Absolute path of the folder the user opened. Used for "Open in Finder". */
  folderPath: string;
  /** Display name of the folder (the basename of `folderPath`). */
  folderName: string;
  /**
   * Whether the folder has any markdown content (matches the
   * `articles.is_empty()` predicate per the spec). When `true`, the
   * card grid is rendered. When `false`, the populated body slot
   * is exposed via `getPopulatedBodyHost()` so the caller can mount
   * CM6 on the home file beneath the divider.
   */
  isEmpty: boolean;
  /**
   * Card click callback. A stub (`stubCardHandler`) fills this in;
   * the caller replaces it with the real handlers. Decoupling the callback keeps
   * the UI shell unaware of the file-creation flow.
   */
  onCardClick?: (card: FolderCardKind) => void;
  /**
   * Optional Tauri command override for tests. Defaults to
   * `uiHost.reveal`. Tests pass a spy so the unit test can run
   * in jsdom without Tauri runtime.
   */
  revealEntry?: (path: string) => Promise<unknown>;
  /**
   * Optional toast override for tests. Defaults to the real
   * `showToast` from `shared/ui/toast/toast-manager.ts`.
   */
  showToast?: typeof showToast;
}

// ── Public constants ────────────────────────────────────────────────────

/**
 * The microcopy under the folder-name heading. Verbatim from the spec
 * (line 224: "The folder name is your site name." + line 225: "Rename
 * in Finder to change it.") joined into one paragraph.
 *
 * **Do not paraphrase.** The dispatch plan explicitly requires this
 * string to match the spec; the test
 * `folder-mode.test.ts: microcopy matches spec verbatim` asserts on it.
 */
export const FOLDER_MICROCOPY =
  "The folder name is your site name. Rename in Finder to change it.";

/**
 * The cards in spec-mockup order. `start-writing` renders large on top; the
 * rest the host provides render as a row beneath the "or start with" divider.
 *
 * `titleKey` and `descKey` are keys into `UiStringMap`; they are resolved
 * via `t()` at render time in `buildCardsHtml()` so locale switches work
 * correctly. DO NOT call `t()` here — this is a module-level const and
 * would freeze the locale at import time.
 */
export const FOLDER_CARDS: ReadonlyArray<{
  kind: FolderCardKind;
  titleKey: keyof UiStringMap;
  descKey: keyof UiStringMap;
  /** Whether the host provides the feature behind the card; absent means always. */
  available?: () => boolean;
}> = [
  {
    kind: "start-writing",
    titleKey: "onboard.card_write_title",
    descKey: "onboard.card_write_desc",
  },
  {
    kind: "try-example",
    titleKey: "onboard.card_example_title",
    descKey: "onboard.card_example_desc",
    available: () => uiHost.starterPicker !== undefined,
  },
  {
    kind: "import-url",
    titleKey: "onboard.card_url_title",
    descKey: "onboard.card_url_desc",
  },
  {
    kind: "connect-plugin",
    titleKey: "onboard.card_plugin_title",
    descKey: "onboard.card_plugin_desc",
  },
];

// ── Pure helpers (unit-tested) ──────────────────────────────────────────

/**
 * Pure function: build the site-header band's HTML structure. Exposed so
 * the test can assert on the structure without going through the full
 * mount flow.
 */
export function buildSiteHeaderHtml(folderName: string): string {
  // Why an inline SVG rather than an icon font:
  //   - moss has no icon font today; existing icons live as inline SVG.
  //   - The hover-reveal "Open in Finder" affordance needs to be a
  //     button (focusable, accessible), so we render the icon inside
  //     the button.
  //
  // The data-tooltip attribute hooks into moss's portal tooltip system
  // (see CLAUDE.md "Tooltips" rule). NEVER use native `title=`.
  const safeName = escapeHtml(folderName);
  return `
    <div class="moss-folder-mode__header">
      <div class="moss-folder-mode__heading-row">
        <h1 class="moss-folder-mode__heading">${safeName}</h1>
        <button
          type="button"
          class="moss-folder-mode__reveal"
          data-folder-mode-reveal
          data-tooltip-position="bottom"
          data-tooltip="${escapeHtml(t('onboard.reveal_finder_tooltip'))}"
          aria-label="${escapeHtml(t('onboard.reveal_finder_aria'))}">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z"/>
          </svg>
        </button>
      </div>
      <p class="moss-folder-mode__microcopy">${escapeHtml(t('folder.site_name_hint'))}</p>
    </div>
  `;
}

/**
 * Build the card grid HTML: large "Start writing" card on top, the cards
 * the host provides beneath, separated by an "or start with" divider.
 */
export function buildCardsHtml(): string {
  const startWriting = FOLDER_CARDS[0];
  const rest = FOLDER_CARDS.slice(1).filter((card) => card.available?.() ?? true);

  const startCard = `
    <button
      type="button"
      class="moss-folder-mode__card moss-folder-mode__card--large"
      data-folder-mode-card="${startWriting.kind}">
      <span class="moss-folder-mode__card-title">${escapeHtml(t(startWriting.titleKey))}</span>
      <span class="moss-folder-mode__card-description">${escapeHtml(t(startWriting.descKey))}</span>
    </button>
  `;

  const restCards = rest.map((card) => `
      <button
        type="button"
        class="moss-folder-mode__card moss-folder-mode__card--small"
        data-folder-mode-card="${card.kind}">
        <span class="moss-folder-mode__card-title">${escapeHtml(t(card.titleKey))}</span>
        <span class="moss-folder-mode__card-description">${escapeHtml(t(card.descKey))}</span>
      </button>
    `).join("");

  return `
    <div class="moss-folder-mode__cards">
      <div class="moss-folder-mode__cards-primary">${startCard}</div>
      <div class="moss-folder-mode__cards-or">${escapeHtml(t('folder.or_start_with'))}</div>
      <div class="moss-folder-mode__cards-grid">${restCards}</div>
    </div>
  `;
}

/**
 * Pure function: build the populated-folder body HTML (the divider
 * beneath the site-header that hosts the CM6 mount when articles exist).
 * The caller mounts CM6 INTO the `[data-folder-mode-body]` slot
 * returned by `getPopulatedBodyHost()`.
 */
export function buildPopulatedBodyHtml(): string {
  return `
    <div class="moss-folder-mode__divider" aria-hidden="true"></div>
    <div class="moss-folder-mode__body" data-folder-mode-body></div>
  `;
}

/**
 * Escape HTML special characters. Self-contained so this module has no
 * cross-dep on a utility; the surface is small enough that adding a
 * cross-import would cost more than it saved.
 */
export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

// ── Mount / unmount ─────────────────────────────────────────────────────

interface MountedHandles {
  host: HTMLElement;
  abort: AbortController;
}

let mounted: MountedHandles | null = null;

/**
 * Teardown callbacks fired on the NEXT `unmountFolderMode()` (or
 * re-mount, which calls unmount first). Used by sibling modules
 * (`folder-mode-handlers.ts`) to deterministically tear down listeners
 * they attach to the live folder-mode DOM (e.g. the inline-URL card's
 * document-level click-outside listener) so none survive an unmount.
 * Each callback runs at most once; the set is cleared after firing.
 */
const unmountCallbacks = new Set<() => void>();

/**
 * Register a callback to run on the next folder-mode unmount/re-mount.
 * Returns a disposer that unregisters the callback (e.g. when the
 * listener it tears down has already been removed by another path, so
 * it shouldn't fire on unmount). Cycle-safe: handlers import this, and
 * this module never imports handlers.
 */
export function onFolderModeUnmount(cb: () => void): () => void {
  unmountCallbacks.add(cb);
  return () => unmountCallbacks.delete(cb);
}

/**
 * Render folder mode into the given host element. Idempotent — calling
 * mount twice unmounts the previous instance first.
 *
 * Returns a `populatedBodyHost` element (the slot beneath the divider
 * for CM6) when `isEmpty === false`, else `null`. Callers wire CM6 into
 * the returned host themselves; folder-mode owns only the chrome.
 */
export function mountFolderMode(
  host: HTMLElement,
  options: FolderModeOptions,
): { populatedBodyHost: HTMLElement | null } {
  if (mounted) {
    unmountFolderMode();
  }

  const reveal = options.revealEntry ?? uiHost.reveal;
  const toast = options.showToast ?? showToast;

  // Empty state is deliberately bare: just the cards on the blueprint
  // grid — no site-header band (heading / microcopy / theme toggle) and no
  // "Start your site" divider. The populated state keeps the header band as
  // the site-name label above the CM6 body.
  const html = options.isEmpty
    // role="region" + aria-label give the bare empty state a landmark and an
    // accessible name now that the visible heading is gone — a screen-reader
    // user still lands on an oriented "Start your site" region.
    ? `<div class="moss-folder-mode moss-folder-mode--empty" role="region" aria-label="${escapeHtml(t('onboard.start_site_aria'))}"><canvas id="blueprint-canvas" aria-hidden="true"></canvas>${buildCardsHtml()}</div>`
    : `<div class="moss-folder-mode moss-folder-mode--populated">${buildSiteHeaderHtml(options.folderName)}${buildPopulatedBodyHtml()}</div>`;

  host.innerHTML = html;

  // Start the animated blueprint grid behind the empty-state content. Same
  // particle-physics canvas the launcher window + preview empty-state use —
  // single source of truth for that visual. The module no-ops if getContext
  // is unavailable (jsdom). Cleanup is wired into the AbortController below
  // so unmountFolderMode() tears it down.
  let cleanupGrid: (() => void) | null = null;
  if (options.isEmpty) {
    const emptyRoot = host.querySelector<HTMLElement>(".moss-folder-mode--empty");
    if (emptyRoot) {
      cleanupGrid = startBlueprintGrid(emptyRoot);
    }
  }

  const abort = new AbortController();

  // Wire reveal button.
  const revealBtn = host.querySelector<HTMLButtonElement>("[data-folder-mode-reveal]");
  // Only where the host has a file manager to show the folder in.
  if (revealBtn && !reveal) revealBtn.remove();
  else if (revealBtn && reveal) {
    revealBtn.addEventListener(
      "click",
      () => {
        // Reveal the folder itself — Finder will select it within its
        // parent, giving the user the native rename gesture.
        void Promise.resolve(reveal(options.folderPath)).catch((err: unknown) => {
          console.warn("[folder-mode] reveal failed:", err);
        });
      },
      { signal: abort.signal },
    );
  }

  // Wire cards. Every handler is a stub until start-writing +
  // try-example create real files, and the
  // other two.
  for (const btn of host.querySelectorAll<HTMLButtonElement>("[data-folder-mode-card]")) {
    btn.addEventListener(
      "click",
      () => {
        const kind = btn.getAttribute("data-folder-mode-card") as FolderCardKind | null;
        if (!kind) return;
        if (options.onCardClick) {
          options.onCardClick(kind);
          return;
        }
        // Default behaviour: stub each card per dispatch plan.
        stubCardHandler(kind, toast);
      },
      { signal: abort.signal },
    );
  }

  if (cleanupGrid) {
    abort.signal.addEventListener("abort", cleanupGrid, { once: true });
  }

  mounted = { host, abort };

  const populatedBodyHost = options.isEmpty
    ? null
    : host.querySelector<HTMLElement>("[data-folder-mode-body]");

  return { populatedBodyHost };
}

/** Tear down the mounted folder-mode component. Idempotent. */
export function unmountFolderMode(): void {
  // Fire sibling teardowns first (URL-card listeners etc.) so they run
  // while the DOM is still live, then clear the registry. Run even if
  // `mounted` is null — a sibling may have live listeners from a prior
  // mount that a defensive unmount call should still tear down.
  if (unmountCallbacks.size > 0) {
    for (const cb of [...unmountCallbacks]) {
      try {
        cb();
      } catch (err) {
        console.warn("[folder-mode] unmount callback failed:", err);
      }
    }
    unmountCallbacks.clear();
  }
  if (!mounted) return;
  mounted.abort.abort();
  mounted.host.innerHTML = "";
  mounted = null;
}

/** Read-only mounted-state introspection (for tests). */
export function isFolderModeMounted(): boolean {
  return mounted !== null;
}

// ── Card stub handlers ──────────────────────────────────────────────────

/**
 * Default no-op stub for card clicks when the caller doesn't supply
 * `onCardClick`. The caller wires the real handlers in `editor-main.ts`; the
 * stub remains as a safety net in jsdom unit tests.
 *
 * Exposed for tests and for callers that want to layer extra behaviour
 * around the stub.
 */
export function stubCardHandler(
  kind: FolderCardKind,
  toast: typeof showToast = showToast,
): void {
  // Stable console log so tests can assert on it and developers can
  // confirm the UI shell is wired without going through the full
  // editor-main mount.
  console.log(`[folder-mode] card clicked: ${kind}`);
  // No toast — real handlers in editor-main own the user feedback path.
  void toast;
}
