/**
 * Environment badge for the titlebar.
 *
 * Shows a STAGING or LOCAL chip next to the edit button when the active
 * folder's hosting environment is non-production. Production shows nothing.
 *
 * Fail-safe: on any error the chip is hidden — a stale STAGING label is
 * worse than no label.
 */

import { host } from "../host";

let getFolder: (() => string | null) | null = null;

/**
 * Wire the environment chip.
 *
 * @param getFolderPath - accessor that returns the currently active folder
 *   path, or null when no folder is open yet.
 */
export function initEnvChip(getFolderPath: () => string | null): void {
  getFolder = getFolderPath;
  void refreshEnvChip();
}

/**
 * Re-query the hosting environment and update the chip visibility/label.
 * Called on init and whenever the active folder changes.
 */
export async function refreshEnvChip(): Promise<void> {
  const el = document.getElementById("moss-env-chip");
  if (!el) return;
  const folder = getFolder?.() ?? null;
  if (!folder) {
    el.hidden = true;
    return;
  }
  try {
    const info = await host.hostingEnvironment?.(folder);
    if (!info || info.is_production) {
      el.hidden = true;
    } else {
      el.textContent = info.environment.toUpperCase(); // "STAGING" | "LOCAL"
      el.hidden = false;
    }
  } catch {
    // Fail-safe: never show a stale environment label.
    el.hidden = true;
  }
}
