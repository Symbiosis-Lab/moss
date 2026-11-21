/**
 * Browser utilities for plugins
 * Abstracts Tauri browser commands to decouple plugins from internal APIs
 */

import { getTauriCore } from "./tauri.js";
import { getInternalContext, hasContext } from "./context.js";

// ============================================================================
// Types
// ============================================================================

/**
 * Reason why the browser window was closed
 * @category Browser
 */
export type BrowserCloseReason =
  | { type: "user" } // User clicked X or closed window
  | { type: "timeout" } // Timed out waiting
  | { type: "programmatic" }; // Plugin called closeBrowser()

/**
 * Handle returned by openBrowser for tracking window lifecycle
 * @category Browser
 */
export interface BrowserHandle {
  /**
   * Promise that resolves when the browser window is closed.
   * Use this to detect when the user closes the window.
   */
  closed: Promise<BrowserCloseReason>;
}

/**
 * Event payload for browser-closed event
 */
interface BrowserClosedEvent {
  reason: BrowserCloseReason;
}

/**
 * Tauri event listen function type
 */
type TauriEventListen = (
  event: string,
  handler: (event: { payload: unknown }) => void
) => Promise<() => void>;

// ============================================================================
// Internal helpers
// ============================================================================

/**
 * Get Tauri event API for listening to and emitting events
 * @internal
 */
function getTauriEvent(): { listen: TauriEventListen } {
  const w = window as unknown as {
    __TAURI__?: {
      event?: {
        listen: TauriEventListen;
      };
    };
  };
  if (!w.__TAURI__?.event?.listen) {
    throw new Error("Tauri event API not available");
  }
  return w.__TAURI__.event;
}

// ============================================================================
// Bridge Script
// ============================================================================

/**
 * Bridge script that exposes `window.mossApi` in browser panel HTML.
 * This decouples plugin HTML from Tauri internals.
 *
 * Provides explicit control over browser lifecycle:
 * - `close()` - Closes the browser panel
 * - `emit(name, payload)` - Emits custom events for plugin-specific communication
 *
 * @internal
 */
const BROWSER_BRIDGE_SCRIPT = `<script>
(function() {
  const { event, core } = window.__TAURI__;
  window.mossApi = {
    close: () => core.invoke('close_action_panel'),
    emit: (name, payload) => event.emit(name, payload),
  };
})();
</script>`;

/**
 * Inject the bridge script into HTML content.
 * If HTML contains </head>, inject before it. Otherwise, prepend.
 * @internal
 */
function injectBridgeScript(html: string): string {
  const headCloseIdx = html.indexOf("</head>");
  if (headCloseIdx !== -1) {
    return html.slice(0, headCloseIdx) + BROWSER_BRIDGE_SCRIPT + html.slice(headCloseIdx);
  }
  return BROWSER_BRIDGE_SCRIPT + html;
}

// ============================================================================
// Functions
// ============================================================================

/**
 * Open a URL in the action panel
 *
 * With a URL, the panel opens (or navigates in place) to that page. With no URL,
 * the panel opens in its loading state, showing the app's own loader, until a
 * later `openBrowser(url)` call loads a page. The returned handle is the same
 * either way.
 *
 * Returns a BrowserHandle that can be used to detect when the window is closed.
 *
 * @param url - The URL to open. Omit it to open the panel in its loading state.
 * @returns BrowserHandle with a `closed` promise
 *
 * @example
 * ```typescript
 * const browser = await openBrowser("https://example.com/login");
 *
 * // Wait for user to close window or authentication to complete
 * const closeReason = await Promise.race([
 *   browser.closed,
 *   waitForAuth().then(() => ({ type: "programmatic" as const }))
 * ]);
 *
 * if (closeReason.type === "user") {
 *   console.log("User closed the window without completing");
 * }
 * ```
 * @category Browser
 */
export async function openBrowser(url?: string): Promise<BrowserHandle> {
  await getTauriCore().invoke("open_action_panel", { url: url ?? null, belowTitlebar: true });

  // Create a promise that resolves when the browser-closed event is received
  const closed = new Promise<BrowserCloseReason>((resolve) => {
    const { listen } = getTauriEvent();

    listen("browser-closed", (event) => {
      const payload = event.payload as BrowserClosedEvent;
      resolve(payload.reason);
    }).then((unlisten) => {
      // Store unlisten so it gets called after promise resolves
      closed.then(() => unlisten());
    });
  });

  return { closed };
}

/**
 * Close the action panel
 * @category Browser
 */
export async function closeBrowser(): Promise<void> {
  await getTauriCore().invoke("close_action_panel", {});
}

/**
 * Ask the app shell to restore the editor in the action panel after a login
 * flow cancel or failure. Clears the onboarding latch and re-mounts the
 * editor (empty-folder onboarding cards) in the action panel slot.
 *
 * Call this after `promptLogin()` returns false on an import (binding /
 * prompt_login) path so the user isn't left with an empty action panel.
 * @category Browser
 */
export async function returnToEditor(): Promise<void> {
  await getTauriCore().invoke("return_to_editor", {});
}

/**
 * Open a URL in the system's default browser
 *
 * Useful for OAuth flows where the user may already be logged in
 * to their browser, providing a better authentication experience.
 *
 * @param url - The URL to open
 * @example
 * ```typescript
 * // OAuth device flow - user may already be logged in
 * await openSystemBrowser("https://github.com/login/device");
 * ```
 * @category Browser
 */
export async function openSystemBrowser(url: string): Promise<void> {
  await getTauriCore().invoke("open_system_browser", { url });
}

/**
 * A sign-in that finishes in the person's own browser and comes back to moss.
 * @category Browser
 */
export interface BrowserSignIn {
  /** Where the provider sends the browser back: `http://127.0.0.1:<port>/callback`. Pass it as the redirect URI. Its port differs each time; a provider matches the rest exactly (RFC 8252 §7.3). */
  redirectUri: string;
  /**
   * Open the provider's sign-in page in the default browser and wait for it to
   * send the browser back. Resolves with the query of that redirect, `error`
   * included; check `state` yourself. Rejects with {@link SignInCancelled}
   * when the person closes the sign-in panel or another sign-in starts. Any
   * other rejection is a failure, such as "the sign-in took longer than ten
   * minutes" or "the sign-in stopped listening".
   */
  open(authorizeUrl: string): Promise<URLSearchParams>;
}

/** The text moss ends a cancelled sign-in with. */
const CANCELLED = "sign-in cancelled";

/**
 * The person stopped the sign-in: they closed it, or another sign-in started.
 * A `login` answers `{ status: "cancelled" }` for it rather than a failure.
 *
 * @example
 * ```typescript
 * try {
 *   reply = await signIn.open(authorizeUrl);
 * } catch (e) {
 *   if (e instanceof SignInCancelled) return { status: "cancelled" };
 *   throw e;
 * }
 * ```
 * @category Browser
 */
export class SignInCancelled extends Error {
  constructor() {
    super(CANCELLED);
    this.name = "SignInCancelled";
  }
}

/**
 * Start a sign-in that finishes in the person's own browser, where they are
 * usually signed in already: the OAuth flow RFC 8252 asks native apps to use.
 * moss listens on this computer for the provider's redirect, once; starting
 * another sign-in ends this one. moss hands back whatever reached the
 * redirect: the caller sends a fresh `state` and refuses a reply whose
 * `state` differs, since only it knows which request it made.
 *
 * Only in the moss app; elsewhere it rejects with "Sign in from the moss app".
 *
 * @example
 * ```typescript
 * const signIn = await startBrowserSignIn();
 * const reply = await signIn.open(`${server}/oauth/authorize?redirect_uri=${encodeURIComponent(signIn.redirectUri)}&state=${state}&...`);
 * if (reply.get("state") !== state) throw new Error("the sign-in did not come from this request");
 * const code = reply.get("code");
 * ```
 * @category Browser
 */
export async function startBrowserSignIn(): Promise<BrowserSignIn> {
  const core = getTauriCore();
  const { redirectUri } = await core.invoke<{ redirectUri: string }>("browser_sign_in_start", {});
  return {
    redirectUri,
    async open(authorizeUrl: string) {
      try {
        const { params } = await core.invoke<{ params: [string, string][] }>("browser_sign_in_wait", { url: authorizeUrl });
        return new URLSearchParams(params);
      } catch (e) {
        throw (e instanceof Error ? e.message : String(e)) === CANCELLED ? new SignInCancelled() : e;
      }
    },
  };
}

/**
 * Open the action panel with dynamic HTML content
 *
 * Automatically injects a bridge script that exposes `window.mossApi` with:
 * - `close()` - closes the browser panel
 * - `emit(name, payload)` - emits custom events for plugin-specific communication
 *
 * Uses a custom protocol (moss-plugin://) to serve HTML content
 * without requiring the `webview-data-url` Cargo feature.
 *
 * **Manual lifecycle control:**
 * After calling this function, the browser panel remains open until you explicitly
 * call `closeBrowser()` or the user closes it. Use `listen()` to handle custom
 * events emitted from the HTML.
 *
 * @param html - Raw HTML content to display
 * @example
 * ```typescript
 * import { openBrowserWithHtml, closeBrowser, listen } from "@symbiosis-lab/moss-api";
 *
 * // Open browser with custom HTML
 * await openBrowserWithHtml(`
 *   <!DOCTYPE html>
 *   <html>
 *     <head><title>My Form</title></head>
 *     <body>
 *       <form id="myForm">
 *         <input id="nameInput" name="name" />
 *         <button type="submit">Submit</button>
 *         <button type="button" onclick="window.mossApi.close()">Cancel</button>
 *       </form>
 *       <script>
 *         document.getElementById('myForm').addEventListener('submit', (e) => {
 *           e.preventDefault();
 *           window.mossApi.emit('my-plugin:form-submit', {
 *             name: document.getElementById('nameInput').value
 *           });
 *         });
 *       </script>
 *     </body>
 *   </html>
 * `);
 *
 * // Listen for custom event from HTML
 * const unlisten = await listen('my-plugin:form-submit', (event) => {
 *   console.log('User submitted:', event.payload);
 *   closeBrowser(); // Explicitly close when done
 * });
 * ```
 * @category Browser
 */
export async function openBrowserWithHtml(html: string): Promise<void> {
  const injectedHtml = injectBridgeScript(html);
  await getTauriCore().invoke("set_action_panel_html", { html: injectedHtml });
}

/**
 * Show an HTML form in the browser panel and wait for the user to submit or cancel.
 *
 * @deprecated This function couples form lifecycle to moss internals through hidden event listeners.
 * Use `openBrowserWithHtml()` + manual `closeBrowser()` instead for explicit control.
 *
 * **Migration guide:**
 * ```typescript
 * // OLD (deprecated):
 * const result = await showBrowserForm<LoginData>(html);
 * if (result) {
 *   console.log("Submitted:", result);
 * }
 *
 * // NEW (recommended):
 * await openBrowserWithHtml(html);
 *
 * // Listen for custom event
 * const unlisten = await listen<LoginData>("my-plugin:submit", (event) => {
 *   console.log("Submitted:", event.payload);
 *   closeBrowser();
 * });
 *
 * // In your HTML:
 * // <button onclick="window.mossApi.emit('my-plugin:submit', { username: '...' })">Submit</button>
 * // <button onclick="window.mossApi.close()">Cancel</button>
 * ```
 *
 * The replacement has an explicit lifecycle (open, use, close) and no hidden
 * event listeners.
 *
 * Note: This deprecated function still listens for `moss:browser-form-submit` and
 * `moss:browser-form-cancel` events for backward compatibility. New code should use
 * `window.mossApi.emit('your-event', data)` and `window.mossApi.close()` instead.
 *
 * Returns the submitted data, or `null` if the user cancelled or the timeout expired.
 * The browser is automatically closed in all cases.
 *
 * @param html - Raw HTML content with a form
 * @param options - Optional configuration
 * @param options.timeoutMs - Maximum time to wait (default: 300000ms / 5 minutes)
 * @param options.closeDelayMs - Optional delay before closing browser (default: 0ms / immediate)
 * @returns The submitted form data, or null on cancel/timeout
 *
 * @example
 * ```typescript
 * interface LoginData { username: string; password: string }
 *
 * const result = await showBrowserForm<LoginData>(`
 *   <!DOCTYPE html>
 *   <html>
 *     <head><title>Login</title></head>
 *     <body>
 *       <form id="login">
 *         <input id="user" placeholder="Username" />
 *         <input id="pass" type="password" placeholder="Password" />
 *         <button type="submit">Login</button>
 *         <button type="button" onclick="window.mossApi.close()">Cancel</button>
 *       </form>
 *       <script>
 *         document.getElementById('login').addEventListener('submit', (e) => {
 *           e.preventDefault();
 *           window.mossApi.emit('moss:browser-form-submit', {
 *             username: document.getElementById('user').value,
 *             password: document.getElementById('pass').value,
 *           });
 *         });
 *       </script>
 *     </body>
 *   </html>
 * `);
 *
 * if (result) {
 *   console.log("User submitted:", result.username);
 * } else {
 *   console.log("User cancelled or timed out");
 * }
 * ```
 * @category Browser
 */
export async function showBrowserForm<T>(
  html: string,
  options?: { timeoutMs?: number; closeDelayMs?: number }
): Promise<T | null> {
  const timeoutMs = options?.timeoutMs ?? 300000;
  const closeDelayMs = options?.closeDelayMs ?? 0;
  await openBrowserWithHtml(html);

  const tauriEvent = getTauriEvent();
  let settled = false;
  let resolveResult: (value: T | null) => void;
  const resultPromise = new Promise<T | null>((r) => {
    resolveResult = r;
  });

  const timer = setTimeout(() => settle(null), timeoutMs);

  const unlistenSubmit = await tauriEvent.listen(
    "moss:browser-form-submit",
    (e: { payload: unknown }) => settle(e.payload as T)
  );
  const unlistenCancel = await tauriEvent.listen(
    "moss:browser-form-cancel",
    () => settle(null)
  );

  function settle(value: T | null) {
    if (settled) return;
    settled = true;
    clearTimeout(timer);
    unlistenSubmit();
    unlistenCancel();

    if (closeDelayMs > 0) {
      setTimeout(() => {
        closeBrowser();
        resolveResult(value);
      }, closeDelayMs);
    } else {
      closeBrowser();
      resolveResult(value);
    }
  }

  return resultPromise;
}

/**
 * A run of the article, or a picture given as its bytes, for {@link fillEditor}.
 * @category Browser
 */
export type EditorFillPart = { html: string } | { picture: { url: string; name: string; type: string; base64: string } };

/**
 * What {@link fillEditor} puts where: the editor's elements on the network's
 * page, the article in parts, and which of moss's reshapings the editor needs.
 * @category Browser
 */
export interface EditorFill {
  editor: {
    /** The editable body. */
    body: string;
    /** The title field, an input, a textarea or an editable element. */
    title?: string;
    /** Present once the editor has loaded; `body` when absent. */
    ready?: string;
  };
  title?: string;
  parts: EditorFillPart[];
  rules?: {
    /** The article's top heading level becomes h1, every deeper one h2. */
    headings?: "two-levels";
    /** One paragraph per table row, cells joined by " · ". */
    tables?: "rows";
    /** One paragraph per line of code. */
    code?: "paragraphs";
  };
  /** How long to wait for the editor. Default 20 s. */
  timeout_ms?: number;
  /** How long each picture may take to appear. Default 15 s. */
  picture_ms?: number;
}

/**
 * How a fill went. `refused`: the editor's content did not change after the
 * first paste. `no-editor`: the editor did not appear in time (its page
 * changed, or the panel shows a sign-in). `missing` names each picture, and
 * the title, that the editor did not take; `url` is where the panel is now.
 * @category Browser
 */
export interface EditorFillAnswer {
  result: "filled" | "no-editor" | "refused";
  placed: number;
  missing: string[];
  url: string;
}

/**
 * Put an article into the network's editor that this plugin opened in the
 * panel, as the author would paste it: moss runs its own script there with
 * this data, never the plugin's code, and only on a page of one of the
 * plugin's declared domains.
 *
 * Only in the moss app; elsewhere it rejects.
 *
 * @example
 * ```typescript
 * await openBrowser("https://network.example/editor/new");
 * const answer = await fillEditor({ editor: { body: ".editor" }, title, parts });
 * if (answer.result !== "filled") showCopyPage();
 * ```
 * @category Browser
 */
export async function fillEditor(fill: EditorFill): Promise<EditorFillAnswer> {
  if (!hasContext()) throw new Error("fillEditor runs only inside a plugin hook");
  return getTauriCore().invoke<EditorFillAnswer>("fill_browser_panel", { pluginName: getInternalContext().plugin_name, fill });
}
