/**
 * Shared single-field input popover: a `.ctx-menu`-styled box at a point with
 * one `.pop-input`, Enter to submit, Escape / outside press to dismiss.
 *
 * One implementation of what the file tree's URL-import overlay hand-rolled
 * (and what naming a template, then renaming one, would have copied twice
 * more). Singular like a context menu: opening a new popover closes the live
 * one, and `closeActiveInputPopover` lets a surface tear it down on unmount.
 *
 * Positioning, clamping and dismissal reuse the context-menu primitives so
 * the popover behaves like the menu that opened it.
 */

import { armOutsideDismiss } from './dismiss-outside';
import { clampIntoViewport } from './ctx-menu';

export interface InputPopoverOptions {
  /** Caption above the field. */
  label: string;
  /** Viewport point to open at (the activating click). */
  at: { x: number; y: number };
  type?: 'text' | 'url';
  placeholder?: string;
  initial?: string;
  /** Message to show under the field, or null when `value` is acceptable. */
  validate?(value: string): string | null;
  /** Runs after the popover has closed with an accepted, non-empty value. */
  onSubmit(value: string): void;
}

export interface InputPopoverHandle {
  close(): void;
}

let active: InputPopoverHandle | null = null;

/** Close the live popover, if any. Idempotent. */
export function closeActiveInputPopover(): void {
  active?.close();
}

export function openInputPopover(opts: InputPopoverOptions): InputPopoverHandle {
  active?.close();

  const overlay = document.createElement('div');
  overlay.className = 'ctx-menu input-popover';
  overlay.style.width = '260px';
  overlay.style.padding = '8px';

  const label = document.createElement('div');
  label.style.cssText = 'font-size:11px;color:var(--moss-color-muted);margin-bottom:6px;padding:0 2px';
  label.textContent = opts.label;
  overlay.appendChild(label);

  const input = document.createElement('input');
  input.type = opts.type ?? 'text';
  input.className = 'pop-input';
  if (opts.placeholder) input.placeholder = opts.placeholder;
  if (opts.initial) input.value = opts.initial;
  input.autocomplete = 'off';
  input.spellcheck = false;
  overlay.appendChild(input);

  const errorEl = document.createElement('div');
  errorEl.style.cssText = 'font-size:10px;color:var(--moss-color-danger,#d63031);margin-top:4px;padding:0 2px;display:none';
  overlay.appendChild(errorEl);

  overlay.style.position = 'fixed';
  overlay.style.left = `${opts.at.x}px`;
  overlay.style.top = `${opts.at.y}px`;
  document.body.appendChild(overlay);
  requestAnimationFrame(() => clampIntoViewport(overlay));

  let closed = false;
  let disarmOutside: (() => void) | null = null;
  const onDocEsc = (ev: KeyboardEvent) => {
    if (ev.key === 'Escape') handle.close();
  };
  const handle: InputPopoverHandle = {
    close() {
      if (closed) return;
      closed = true;
      if (active === handle) active = null;
      overlay.remove();
      document.removeEventListener('keydown', onDocEsc);
      disarmOutside?.();
    },
  };
  active = handle;

  const submit = () => {
    const value = input.value.trim();
    if (!value) { input.focus(); return; }
    const problem = opts.validate?.(value) ?? null;
    if (problem !== null) {
      errorEl.textContent = problem;
      errorEl.style.display = '';
      input.focus();
      return;
    }
    // Close BEFORE the callback so a follow-up surface it opens survives.
    handle.close();
    opts.onSubmit(value);
  };

  // A handled key stops here, as the menu's do. Otherwise Enter bubbles on
  // to the document after `submit()` has removed the popover, and the tree's
  // key handler — which treats a focus-less document as its own — runs
  // Enter's rename on the selected row.
  input.addEventListener('keydown', (ev) => {
    if (ev.key === 'Enter') { ev.preventDefault(); ev.stopPropagation(); submit(); }
    else if (ev.key === 'Escape') { ev.preventDefault(); ev.stopPropagation(); handle.close(); }
  });
  input.addEventListener('input', () => { errorEl.style.display = 'none'; });

  // Deferred so the click that opened the popover cannot dismiss it (the
  // outside-dismiss helper defers its own registration for the same reason).
  disarmOutside = armOutsideDismiss({
    inside: [overlay],
    covers: [overlay],
    onDismiss: () => handle.close(),
  });
  setTimeout(() => {
    if (closed) return;
    document.addEventListener('keydown', onDocEsc);
    input.focus();
    if (opts.initial) input.select();
  }, 0);

  return handle;
}
