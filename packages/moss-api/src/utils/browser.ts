/**
 * Browser utilities for plugins
 * Abstracts Tauri browser commands to decouple plugins from internal APIs
 */

import { getTauriCore } from "./tauri.js";

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
 * Returns a BrowserHandle that can be used to detect when the window is closed.
 *
 * @param url - The URL to open
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
export async function openBrowser(url: string): Promise<BrowserHandle> {
  await getTauriCore().invoke("open_action_panel", { url, belowTitlebar: true });

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
 * **Why migrate:**
 * - Explicit browser lifecycle control (no magic auto-close)
 * - No hidden event listeners (`moss:browser-form-submit`, `moss:browser-form-cancel`)
 * - Simpler mental model: open, use, close
 * - Matches modern plugin patterns (see Matters plugin)
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
