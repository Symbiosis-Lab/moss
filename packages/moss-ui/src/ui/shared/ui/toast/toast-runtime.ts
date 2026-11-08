// The host seam for toasts — a log sink, the system opener, and any
// special-case URL routes the host wants consulted before it.
//
// These are INJECTED rather than imported so the toast modules stay pure (no
// host globals, importable by every host). Injection is one-time and global rather than a parameter,
// because the ~18 call sites of `showToast`/`showAck` do not know or care about
// the opener; threading it through all of them would push the coupling outward
// instead of removing it.
//
// The entry points install it through `installHostToastRuntime` (utils/open-external.ts)
// before they mount toasts; a webview that skips that install opens its toast links
// through `window.open` instead of the system browser.

import type { ResultLike } from './toast-types';

/**
 * A host-installed special-case opener, consulted before the generic
 * {@link ToastRuntime.openUrl}. Toast actions are plain `{label, url}` data
 * (plugins can only send serializable event payloads across the IPC boundary,
 * never a live callback), so a workflow that needs its URLs opened specially —
 * OnionPress routing `.onion` hosts through the Tor-aware backend opener —
 * registers a route here; the toast layer, as the one place that actually
 * opens the URL, matches it by predicate. The knowledge of WHICH URLs qualify
 * stays with the workflow that installs the route, not in shared/.
 */
export interface UrlOpenRoute {
  /** Short name used in error logs (e.g. "onion"). */
  name: string;
  /** Does this route claim the URL? */
  matches(url: string): boolean;
  /** Open the URL via the host. */
  open(url: string): Promise<ResultLike<unknown>>;
}

/**
 * The host capabilities a toast needs. Supplied by the app shell via
 * {@link configureToastRuntime}; see the module header for why it's a
 * one-time global rather than a per-call parameter.
 */
export interface ToastRuntime {
  /** Log sink for a failed open. */
  logError(message: string): void;
  /**
   * Log sink for a toast actually being shown — id/variant only, never
   * message content.
   */
  logInfo(message: string): void;
  /**
   * Log sink for an already-formatted {@link formatUiEvent} breadcrumb line.
   * Reuses this seam rather than a dedicated one because it exists for
   * exactly this reason — `shared/ui` may not import `platform/` directly
   * (`ctx-menu.ts` and `run-action.ts`
   * are the two shared/ui modules that needed it).
   */
  logUiEvent(line: string): void;
  /** Open a URL in the system browser. */
  openUrl(url: string): Promise<void>;
  /** Special-case openers consulted before {@link openUrl}; first match wins. */
  urlRoutes?: UrlOpenRoute[];
}

/**
 * Browser-only fallback: no Tauri, no special routes. Used until an entry
 * point installs the real runtime, and in unit tests.
 */
const defaultRuntime: ToastRuntime = {
  logError: (message) => console.error(message),
  logInfo: (message) => console.info(message),
  logUiEvent: (line) => console.info(line),
  openUrl: async (url) => {
    window.open(url, "_blank");
  },
};

let runtime: ToastRuntime = defaultRuntime;

/**
 * Read the installed runtime. An accessor, not an exported binding, so a
 * consumer can never capture a stale reference across a
 * {@link configureToastRuntime} call — call it at use time, not import time.
 */
export function getToastRuntime(): ToastRuntime {
  return runtime;
}

/**
 * Install host capabilities on the toast runtime. Omitted members keep whatever was installed before.
 */
export function configureToastRuntime(next: Partial<ToastRuntime>): void {
  runtime = { ...runtime, ...next };
}

/** Restore the browser-only fallback runtime. @internal (tests) */
export function _resetToastRuntime(): void {
  runtime = defaultRuntime;
}

/**
 * Pure formatter for the `[ui] <name> field=value…` breadcrumb line (see
 * `log.ts`'s `uiEvent` for the full rationale). Lives here, next to
 * {@link ToastRuntime}, rather than in `platform/`, because `shared/ui` may
 * not depend on `platform/` — `ctx-menu.ts` and `run-action.ts` format the
 * line with this and hand it to {@link ToastRuntime.logUiEvent}.
 * `log.ts`'s own `uiEvent` calls this same formatter so the two
 * paths never drift.
 */
export function formatUiEvent(
  name: string,
  fields?: Record<string, string | number | boolean | undefined>,
): string {
  let line = `[ui] ${name}`;
  if (fields) {
    for (const [key, value] of Object.entries(fields)) {
      if (value === undefined) continue;
      line += ` ${key}=${value}`;
    }
  }
  return line;
}

/**
 * Format and log a `[ui] …` breadcrumb through the injected host seam in one
 * call — the single implementation `ctx-menu.ts` and `run-action.ts` both
 * use, rather than each defining its own one-line wrapper over
 * {@link formatUiEvent} + {@link getToastRuntime}.
 */
export function uiEvent(
  name: string,
  fields?: Record<string, string | number | boolean | undefined>,
): void {
  getToastRuntime().logUiEvent(formatUiEvent(name, fields));
}
