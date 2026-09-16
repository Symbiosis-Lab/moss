/**
 * Tauri core utilities for plugin communication
 * @category Tauri core (deprecated)
 */

export interface TauriCore {
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
}

interface TauriWindow {
  __TAURI__?: {
    core: TauriCore;
  };
}

/**
 * Get the Tauri core API
 *
 * @deprecated Use higher-level APIs instead:
 * - File operations: `readFile`, `writeFile`, `listFiles`, `fileExists`
 * - HTTP: `fetchUrl`, `downloadAsset`
 * - Binary execution: `executeBinary`
 * - Cookies: `getPluginCookie`, `setPluginCookie`
 *
 * @throws Error if Tauri is not available
 * @category Tauri core (deprecated)
 */
export function getTauriCore(): TauriCore {
  const w = window as unknown as TauriWindow;
  if (!w.__TAURI__?.core) {
    throw new Error("Tauri core not available");
  }
  return w.__TAURI__.core;
}

/**
 * Check if Tauri is available
 * @category Tauri core (deprecated)
 */
export function isTauriAvailable(): boolean {
  const w = window as unknown as TauriWindow;
  return !!w.__TAURI__?.core;
}
