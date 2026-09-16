/**
 * Cookie management for moss plugins
 *
 * Allows plugins to store and retrieve authentication cookies
 * for external services (e.g., Matters.town, GitHub).
 *
 * Cookies are automatically scoped to the plugin's registered domain
 * (defined in manifest.json) - plugins cannot access other plugins' cookies.
 */

import { getTauriCore } from "./tauri.js";
import { getInternalContext, hasContext } from "./context.js";

// ============================================================================
// Types
// ============================================================================

/**
 * A cookie stored for plugin authentication
 *
 * @category Cookies
 */
export interface Cookie {
  /** Cookie name */
  name: string;
  /** Cookie value */
  value: string;
  /** Optional domain for the cookie */
  domain?: string;
  /** Optional path for the cookie */
  path?: string;
}

// ============================================================================
// Functions
// ============================================================================

/**
 * Get stored cookies for the current plugin.
 *
 * The plugin's identity is automatically detected from the runtime context.
 * Cookies are filtered by the domain declared in the plugin's manifest.json.
 *
 * @returns Array of cookies for the plugin's registered domain, or `null` if
 *          called outside of a plugin hook context.
 *
 * @example
 * ```typescript
 * // Inside a hook function:
 * const cookies = await getPluginCookie();
 *
 * // null means no context (e.g., window closed, hook ended)
 * if (cookies === null) {
 *   console.log("No plugin context - stopping");
 *   return;
 * }
 *
 * const token = cookies.find(c => c.name === "__access_token");
 * if (token) {
 *   // Use token for authenticated requests
 * }
 * ```
 * @category Cookies
 */
export async function getPluginCookie(): Promise<Cookie[] | null> {
  // Return null if called outside hook context (e.g., after window closed)
  if (!hasContext()) {
    return null;
  }

  const ctx = getInternalContext();

  return getTauriCore().invoke<Cookie[]>("get_plugin_cookie", {
    pluginName: ctx.plugin_name,
    projectPath: ctx.project_path,
  });
}

/**
 * Store cookies for the current plugin.
 *
 * The plugin's identity is automatically detected from the runtime context.
 *
 * **Must be called from within a plugin hook** (process, generate, deploy, syndicate).
 *
 * @param cookies - Array of cookies to store
 * @throws Error if called outside of a plugin hook execution
 *
 * @example
 * ```typescript
 * // Inside a hook function:
 * await setPluginCookie([
 *   { name: "session", value: "abc123" }
 * ]);
 * ```
 * @category Cookies
 */
export async function setPluginCookie(cookies: Cookie[]): Promise<void> {
  const ctx = getInternalContext();

  await getTauriCore().invoke("set_plugin_cookie", {
    pluginName: ctx.plugin_name,
    projectPath: ctx.project_path,
    cookies,
  });
}

/**
 * Delete ALL cookies on the current plugin's registered (manifest) domain from
 * the shared WebKit store. Used for force-fresh login: clears any lingering
 * server session so the login webview presents a real credential screen.
 *
 * The plugin's identity is auto-detected from the runtime context.
 * **Must be called from within a plugin hook.**
 *
 * @category Cookies
 */
export async function clearPluginCookies(): Promise<void> {
  const ctx = getInternalContext();

  await getTauriCore().invoke("clear_plugin_cookies", {
    pluginName: ctx.plugin_name,
    projectPath: ctx.project_path,
  });
}
