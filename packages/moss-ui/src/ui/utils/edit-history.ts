// Undo / Redo, routed from the app menu to whatever the author is editing.
//
// The Edit menu's Undo and Redo are moss's own items (the desktop app builds
// its own menu), not the system's. The system's version
// only knows about letters typed by hand: every edit moss makes on the author's
// behalf — a block from the insert bar, a chip toggle, ⌘B — is invisible to it,
// so ⌘Z after one of those did nothing, and did it silently, because the menu
// had already claimed the keystroke before CodeMirror could see it.
//
// So Rust broadcasts `menu:edit-undo` / `menu:edit-redo` to every webview and
// this module answers in exactly one of them: the one the author is looking at.
// A surface that owns a real history (the editor's CodeMirror document)
// registers a handler; anything else — a settings field, the chip bar's text
// inputs — falls through to the browser's own history for that field.

import { onMossSignal } from './moss-events';

/**
 * Apply undo/redo to a surface with its own history.
 *
 * Return `true` if this surface handled it. Return `false` to fall through to
 * the focused form field's built-in history — which is the right answer when
 * the caret is not in that surface at all.
 */
export type EditHistoryHandler = (direction: 'undo' | 'redo') => boolean;

/**
 * Wire the app menu's Undo/Redo into this webview. Returns a teardown fn.
 *
 * `handler` is optional: a webview with no rich-text surface of its own (the
 * shell, the launcher) can call this with no arguments and still get working
 * ⌘Z in its text fields.
 */
export function initEditHistoryMenu(handler?: EditHistoryHandler): () => void {
  const unlisten: Promise<() => void>[] = [];

  const apply = (direction: 'undo' | 'redo'): void => {
    // Every webview receives the broadcast; only the focused one may act, or a
    // background editor would silently rewind its document.
    if (!document.hasFocus()) return;
    if (handler?.(direction)) return;
    // Plain form fields: the browser kept their history, because their edits
    // were typed. `execCommand` is deprecated but is still the only way to
    // reach that history from script.
    document.execCommand(direction);
  };

  unlisten.push(onMossSignal('menu:edit-undo', () => apply('undo')));
  unlisten.push(onMossSignal('menu:edit-redo', () => apply('redo')));

  return () => {
    for (const p of unlisten) void p.then((off) => off());
  };
}
