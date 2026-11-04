/**
 * localStorage that never throws. Reading `localStorage` itself raises
 * SecurityError when site data is blocked or in a sandboxed frame, so every
 * access goes through here: a read yields null, a write does nothing, and the
 * page keeps working for the visit without remembering.
 */
export function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeStored(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* storage blocked: preference lasts for this visit only */
  }
}
