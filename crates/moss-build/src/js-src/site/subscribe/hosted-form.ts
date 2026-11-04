/** What every hosted form script (subscribe, request forms) shares. */

export type FormState = 'idle' | 'loading' | 'success' | 'error';

const hydrated = new WeakSet<HTMLElement>();

/**
 * First claim wins. A WeakSet keyed on the element, not a `data-` flag: idiomorph
 * strips attributes the runtime added, so a flag would vanish from the node it
 * guards and a post-morph sweep would bind a second listener.
 */
export function claim(el: HTMLElement): boolean {
  if (hydrated.has(el)) return false;
  hydrated.add(el);
  return true;
}

/** Hydrates matching forms now, once the document is parsed, and after every preview morph (which patches the DOM without re-running scripts). */
export function autoHydrate<T extends HTMLElement>(selector: string, hydrate: (el: T) => void): void {
  if (typeof document === 'undefined') return;
  const sweep = () => document.querySelectorAll<T>(selector).forEach(hydrate);
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', sweep);
  else sweep();
  document.addEventListener('moss-morph-patched', sweep);
}

/** The preview has no live service behind a form: scripts simulate the send. */
export const inPreview = (): boolean => document.body.hasAttribute('data-moss-preview');

/** Sets `data-state` on `root` (the form, or the block around it) and mirrors it to the button as `aria-disabled`. */
export function setState(root: HTMLElement, state: FormState, btn?: HTMLButtonElement | null): void {
  root.dataset.state = state;
  const button = btn ?? root.querySelector<HTMLButtonElement>('button[type="submit"]');
  if (!button) return;
  if (state === 'loading' || state === 'success') button.setAttribute('aria-disabled', 'true');
  else button.removeAttribute('aria-disabled');
}

/**
 * Freezes the button slot to the button's idle width, once, so the field beside it
 * never sees the button change size. A stacked (column) form keeps the slot fluid.
 */
export function lockSlotWidth(form: HTMLElement, btn: HTMLButtonElement): void {
  const slot = btn.parentElement;
  if (!slot || !slot.classList.contains('moss-btn-slot')) return;
  if (getComputedStyle(form).flexDirection === 'column') {
    slot.style.width = '';
    return;
  }
  if (slot.style.width) return;
  const w = btn.offsetWidth;
  if (w <= 0) return; // layout not ready (detached, display:none, jsdom)
  slot.style.width = `${w}px`;
}
