/**
 * Internal-files toggle.
 *
 * Persisted to localStorage as `moss-editor-show-internal`. When toggled,
 * emits a `CustomEvent<boolean>` on `document` named
 * `editor:internal-toggle-changed` with `detail` = new state.
 */

const STORAGE_KEY = 'moss-editor-show-internal';

/** Read the persisted state (default: false). */
export function getInternalToggleState(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === 'true';
  } catch {
    return false;
  }
}

/** Flip the internal-files toggle, persist to localStorage, and dispatch the change event. */
export function toggleInternalState(): void {
  const next = !getInternalToggleState();
  try { localStorage.setItem(STORAGE_KEY, String(next)); } catch {}
  document.dispatchEvent(
    new CustomEvent<boolean>('editor:internal-toggle-changed', { detail: next }),
  );
}
