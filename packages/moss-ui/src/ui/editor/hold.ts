// The hold: why the folder takes no edits right now, and how the editor shows
// it. The reasons are a set, so one holder's release never lifts another's,
// and module state rather than an editor's, so a file opened during a hold
// starts held. The look: the text keeps full contrast, the insert bar's place
// in the bottom band says why, and an attempt to edit pulses that note.

import { applyStaticStrings } from '../i18n/apply-static-strings';
import { editorBand } from './editor-band';

export type HoldReason = 'publish' | 'commit';

const reasons = new Set<HoldReason>();
const listeners = new Set<() => void>();

export function setHold(reason: HoldReason, held: boolean): void {
  if (held === reasons.has(reason)) return;
  if (held) reasons.add(reason);
  else reasons.delete(reason);
  for (const fn of listeners) fn();
}

export const isHeldFor = (reason: HoldReason): boolean => reasons.has(reason);

/** The reason to show: a publish outlasts and outranks a rename's rewrite. */
export function heldFor(): HoldReason | null {
  return reasons.has('publish') ? 'publish' : reasons.has('commit') ? 'commit' : null;
}

export function onHoldChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

const NOTE_KEY: Record<HoldReason, string> = {
  publish: 'editor.hold_publishing',
  commit: 'editor.hold_committing',
};

/** Controls around the text that write to the folder. The title, undo and redo hold themselves. */
const WRITE_CONTROLS = '#chip-bar, .editor-create-btn';

const note = (): HTMLElement | null => document.querySelector<HTMLElement>('.hold-note');

function pulse(): void {
  const el = note();
  if (!el) return;
  el.classList.remove('is-pulsing');
  void el.offsetWidth; // restarts the animation
  el.classList.add('is-pulsing');
}

function refuse(e: Event): void {
  e.preventDefault();
  e.stopImmediatePropagation();
}

function onPointer(e: Event): void {
  if (!(e.target as Element | null)?.closest?.(WRITE_CONTROLS)) return;
  refuse(e);
  if (e.type === 'mousedown') pulse();
}

function onKey(e: KeyboardEvent): void {
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  // Space scrolls a page that cannot be typed in.
  const edits = (e.key.length === 1 && e.key !== ' ') || ['Backspace', 'Delete', 'Enter'].includes(e.key);
  if (!edits) return;
  const target = e.target as Element | null;
  // A field inside a write control writes; one elsewhere (the tree's filter) does not.
  if (target?.closest?.(WRITE_CONTROLS)) refuse(e);
  else if (target?.closest?.('input, textarea, #nav-grid')) return;
  pulse();
}

function listen(on: boolean): void {
  const act = on ? document.addEventListener : document.removeEventListener;
  for (const type of ['pointerdown', 'mousedown', 'click', 'dblclick', 'contextmenu']) act.call(document, type, onPointer, true);
  act.call(document, 'keydown', onKey as EventListener, true);
}

/**
 * Show the hold for `reason`, or lift it with `null`. Safe to call again after
 * the pane is rebuilt. Only a publish holds the controls outside the text: a
 * rename's rewrite is over in a moment, and Enter in the title is what starts it.
 */
export function showHold(reason: HoldReason | null): void {
  const publishing = reason === 'publish';
  document.getElementById('moss-editor-root')?.toggleAttribute('data-hold', publishing);
  document.getElementById('editor-heading')
    ?.setAttribute('contenteditable', publishing ? 'false' : 'plaintext-only');
  listen(publishing);
  let el = note();
  if (!reason) {
    el?.remove();
    return;
  }
  const band = editorBand();
  if (el?.parentElement !== band) {
    el?.remove();
    el = document.createElement('div');
    el.className = 'hold-note';
    el.setAttribute('role', 'status');
    band.appendChild(el);
  }
  el.dataset.i18n = NOTE_KEY[reason];
  applyStaticStrings(band);
}
