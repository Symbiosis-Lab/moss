/**
 * Keyboard helpers for the file-tree navigation: the pure key → action map
 * and the one stateful piece, the type-ahead buffer.
 *
 * No DOM. Every function export is pure; `TypeAhead` owns one timer.
 * Consumed by FileTree.handleKey() in file-tree.ts.
 *
 * Architecture: WAI-ARIA tree pattern with tabIndex roving (single row with
 * tabIndex=0, others -1). moss uses roving tabIndex; VS Code uses
 * aria-activedescendant. Both are valid per WAI-ARIA tree pattern; roving
 * tabIndex is simpler for non-virtualized trees.
 */

import type { EntryId } from '../entry-registry';

// ── Action discriminated union ────────────────────────────────────────────────

export type KeyAction =
  | { kind: 'moveFocus'; direction: 'up' | 'down' | 'home' | 'end'; extend: boolean; focusOnly: boolean }
  | { kind: 'expand' }
  | { kind: 'collapse' }
  | { kind: 'collapseAll' }               // Cmd/Ctrl+Shift+←: collapse entire tree to breadcrumb
  | { kind: 'expandAllSiblings' }         // * (asterisk): expand all sibling closed nodes (WAI-ARIA APG)
  | { kind: 'toggleSelection' }            // Cmd/Ctrl+Space
  | { kind: 'rangeToFocus' }              // Shift+Space: select contiguous range anchor→focus
  | { kind: 'selectAll' }                  // Cmd/Ctrl+A
  | { kind: 'clear' }                      // Escape
  | { kind: 'open'; focusEditor: boolean } // Space=keep focus, Cmd+Down handler uses moveFocus+focusOnly
  | { kind: 'rename' }                     // the rename key, or Enter
  | { kind: 'delete' }                     // Cmd+Backspace (Mac) / Delete (all)
  | { kind: 'contextMenu' }                // Shift+F10 / ContextMenu key
  | { kind: 'focusPane'; pane: 'tree' | 'editor' } // Cmd+1 / Cmd+2
  | { kind: 'typeAhead'; char: string };

// ── keyToAction ───────────────────────────────────────────────────────────────

/**
 * Map a raw KeyboardEvent to a semantic KeyAction, or null if not handled.
 *
 * Pure — reads only from the event, never from DOM or external state.
 * Callers are responsible for early-returning when isEditing() is true.
 */
export function keyToAction(event: KeyboardEvent): KeyAction | null {
  const { key, metaKey, ctrlKey, shiftKey } = event;
  const mod = metaKey || ctrlKey; // platform-adaptive modifier

  // Cross-pane shortcuts (handled even when focus is in editor)
  if (mod && key === '1') return { kind: 'focusPane', pane: 'tree' };
  if (mod && key === '2') return { kind: 'focusPane', pane: 'editor' };

  // Select-all
  if (mod && (key === 'a' || key === 'A')) return { kind: 'selectAll' };

  // Toggle selection
  if (mod && key === ' ') return { kind: 'toggleSelection' };

  // Range-to-focus: Shift+Space extends selection from anchor to focus
  // WAI-ARIA APG multi-select tree pattern selection behavior
  if (key === ' ' && shiftKey && !mod) return { kind: 'rangeToFocus' };

  // Context menu (Shift+F10 / dedicated ContextMenu key) — must precede the
  // F-key skip below, which would otherwise swallow F10.
  if (key === 'ContextMenu' || (shiftKey && key === 'F10')) return { kind: 'contextMenu' };

  // Delete (Cmd+Backspace Mac, or Delete key)
  if (mod && key === 'Backspace') return { kind: 'delete' };
  if (key === 'Delete') return { kind: 'delete' };

  // Arrow movement
  if (key === 'ArrowDown') {
    return { kind: 'moveFocus', direction: 'down', extend: shiftKey, focusOnly: mod && !shiftKey };
  }
  if (key === 'ArrowUp') {
    return { kind: 'moveFocus', direction: 'up', extend: shiftKey, focusOnly: mod && !shiftKey };
  }

  // Expand / collapse
  if (key === 'ArrowRight') return { kind: 'expand' };
  // Cmd/Ctrl+Shift+←: collapse entire tree to breadcrumb only (WAI-ARIA complement).
  // Chosen over Cmd+0 (which is already used for font-scale reset in editor-main.ts)
  // and Cmd+Shift+Up (which has OS-level meaning on macOS). Matches Zed's
  // "collapse project" action which also uses a Shift+modifier+arrow form.
  if (mod && shiftKey && key === 'ArrowLeft') return { kind: 'collapseAll' };
  if (key === 'ArrowLeft') return { kind: 'collapse' };

  // * (asterisk, the WAI-ARIA APG tree pattern): expand all siblings of the focused node.
  // Per spec: "When focus is on a treeitem inside a group, * expands all sibling
  // closed nodes at the same level."
  // https://www.w3.org/WAI/ARIA/apg/patterns/treeview/
  if (key === '*' && !mod && !shiftKey) return { kind: 'expandAllSiblings' };

  // Home / End
  // Ctrl/Cmd+Shift+Home/End: extend selection to first/last node (WAI-ARIA APG)
  if (key === 'Home') return { kind: 'moveFocus', direction: 'home', extend: shiftKey && mod, focusOnly: false };
  if (key === 'End') return { kind: 'moveFocus', direction: 'end', extend: shiftKey && mod, focusOnly: false };

  // Escape
  if (key === 'Escape') return { kind: 'clear' };

  // Open (Space — keep focus in tree)
  if (key === ' ' && !mod && !shiftKey) return { kind: 'open', focusEditor: false };

  // Rename (the rename key or Enter)
  if (key === 'F2') return { kind: 'rename' };
  if (key === 'Enter') return { kind: 'rename' };

  // Modifier-only keys and other non-printable keys — skip
  if (
    key === 'Shift' || key === 'Control' || key === 'Meta' || key === 'Alt' ||
    key === 'Tab' || key === 'CapsLock' || key === 'PageUp' || key === 'PageDown' ||
    key.startsWith('F') // the other function keys
  ) {
    return null;
  }

  // Any unhandled combination with modifier (Cmd+Z etc.) → not a tree shortcut
  if (mod) return null;

  // Single printable character (no modifier) → type-ahead
  if (key.length === 1) return { kind: 'typeAhead', char: key };

  return null;
}

// ── resolveClickAction ────────────────────────────────────────────────────────

/**
 * The selection primitive a row click asks for. `'plain'` is the caller's to
 * finish — open the file, expand the folder — which is the one thing the two
 * row kinds do differently; the modifier decode is what they used to paste.
 */
export type ClickAction = 'plain' | 'toggle' | 'setRange' | 'addRange';

/** Modifier chord → selection primitive. Same `mod` rule as keyToAction. */
export function resolveClickAction(
  event: Pick<MouseEvent, 'metaKey' | 'ctrlKey' | 'shiftKey'> | undefined,
): ClickAction {
  if (!event) return 'plain';
  const mod = event.metaKey || event.ctrlKey;
  if (mod && event.shiftKey) return 'addRange';
  if (event.shiftKey) return 'setRange';
  if (mod) return 'toggle';
  return 'plain';
}

// ── computeNextFocusIndex ─────────────────────────────────────────────────────

/**
 * Compute the next focus index given a movement direction.
 *
 * Clamping behavior (no wraparound) matches VS Code and WAI-ARIA tree:
 *   - 'down' at last row stays at last row
 *   - 'up' at first row stays at first row
 *   - 'home' always returns 0
 *   - 'end' always returns total-1
 *
 * Returns -1 when total === 0 (empty list).
 */
export function computeNextFocusIndex(
  current: number,
  total: number,
  direction: 'up' | 'down' | 'home' | 'end',
): number {
  if (total === 0) return -1;
  switch (direction) {
    case 'down': return Math.min(current + 1, total - 1);
    case 'up':   return Math.max(current - 1, 0);
    case 'home': return 0;
    case 'end':  return total - 1;
  }
}

// ── matchTypeAhead ────────────────────────────────────────────────────────────

/**
 * Find the next visible row whose name starts with `prefix` (case-insensitive),
 * cycling forward from `fromIdx` (exclusive — the "current" position).
 *
 * Cycling behavior matches macOS Finder:
 *   - Search starts AFTER fromIdx, wraps to 0, stops before fromIdx.
 *   - If fromIdx itself is the only match, it is returned (full wrap).
 *
 * Returns null if prefix is empty or no row matches.
 */
export function matchTypeAhead(
  rows: { id: EntryId; name: string }[],
  prefix: string,
  fromIdx: number,
): EntryId | null {
  if (!prefix || rows.length === 0) return null;
  const lower = prefix.toLowerCase();
  const total = rows.length;

  // Search forward from fromIdx+1 wrapping around
  for (let i = 1; i <= total; i++) {
    const idx = (fromIdx + i) % total;
    if (rows[idx].name.toLowerCase().startsWith(lower)) {
      return rows[idx].id;
    }
  }
  return null;
}

// ── TypeAhead ─────────────────────────────────────────────────────────────────

/** How long the type-ahead buffer waits for the next character before it
 *  starts over (Finder: about a second). */
export const TYPE_AHEAD_RESET_MS = 1000;

/**
 * The type-ahead buffer: characters typed in quick succession form one
 * prefix, a pause starts a new one. Two fields on the controller cleared by
 * hand in `unmount` — one record with
 * one `dispose()`, so the reset timer cannot outlive the tree.
 */
export class TypeAhead {
  private buffer = '';
  private timer: ReturnType<typeof setTimeout> | null = null;

  /** Append `char` and return the prefix to match; re-arms the reset. */
  push(char: string): string {
    if (this.timer !== null) clearTimeout(this.timer);
    this.buffer += char;
    this.timer = setTimeout(() => {
      this.buffer = '';
      this.timer = null;
    }, TYPE_AHEAD_RESET_MS);
    return this.buffer;
  }

  /** Drop the prefix and the pending reset. */
  dispose(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    this.buffer = '';
  }
}
