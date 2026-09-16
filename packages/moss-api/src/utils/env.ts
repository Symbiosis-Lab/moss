/**
 * Plugin-side env-var access.
 *
 * Plugins run inside a webview (no Node `process.env`), so reading host
 * environment variables requires a Rust→TS bridge. The
 * `get_plugin_env_var` Tauri command in `src-tauri/src/plugins/runtime.rs`
 * enforces a server-side allow-list — plugins cannot read arbitrary
 * environment variables, only the ones moss has whitelisted for test /
 * harness use.
 *
 * Currently allow-listed (see runtime.rs `ALLOWED` constant):
 * - `MOSS_MATTERS_TEST_PROFILE` — bypasses Matters auth and switches to
 *   public-fetch mode for the named profile (T8a e2e harness).
 * - `MOSS_MATTERS_DOMAIN` — overrides the Matters domain (e.g. "matters.icu")
 *   so moss-claude.sh can target the test env without pre-seeding config.json.
 *
 * New entries require an explicit Rust-side edit + code review.
 */

import { getTauriCore, isTauriAvailable } from "./tauri.js";

/**
 * Read a host environment variable into the plugin webview.
 *
 * Returns `undefined` if:
 * - Tauri is unavailable (running outside the moss webview),
 * - the variable is not in the server-side allow-list, or
 * - the variable is not set in the host process.
 *
 * Plugins should treat the return value as best-effort: a missing value
 * is the production default, not an error.
 * @category Environment
 */
export async function getPluginEnvVar(name: string): Promise<string | undefined> {
  if (!isTauriAvailable()) return undefined;
  try {
    const value = await getTauriCore().invoke<string | null>("get_plugin_env_var", {
      name,
    });
    return value ?? undefined;
  } catch (err) {
    // Don't crash the plugin if the bridge isn't there yet (older moss
    // versions, command not registered, etc.). Log so it's debuggable.
    console.warn(`[moss-api] getPluginEnvVar(${name}) failed:`, err);
    return undefined;
  }
}
