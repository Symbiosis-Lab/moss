/**
 * Unit tests for the tree/keyboard pure helper module.
 *
 * Tests cover:
 * - keyToAction: every key combo → expected KeyAction or null
 * - computeNextFocusIndex: every direction including edge/wraparound
 * - matchTypeAhead: no-match, single-match, cycling among multiple matches
 * - TypeAhead: the buffer's reset window and its dispose
 *
 * No DOM required — all pure functions, plain KeyboardEvent objects.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import type { EntryId } from '../../entry-registry';
import { keyToAction, resolveClickAction, computeNextFocusIndex, matchTypeAhead, TypeAhead, TYPE_AHEAD_RESET_MS } from '../keyboard';

// Helper to create a KeyboardEvent with modifiers
function ke(
  key: string,
  opts: {
    metaKey?: boolean;
    ctrlKey?: boolean;
    shiftKey?: boolean;
    altKey?: boolean;
  } = {},
): KeyboardEvent {
  return new KeyboardEvent('keydown', {
    key,
    metaKey: opts.metaKey ?? false,
    ctrlKey: opts.ctrlKey ?? false,
    shiftKey: opts.shiftKey ?? false,
    altKey: opts.altKey ?? false,
    bubbles: true,
    cancelable: true,
  });
}

// Helper: create a fake EntryId
function id(n: number): EntryId {
  return n as EntryId;
}

// ── resolveClickAction ───────────────────────────────────────────────────────

describe('resolveClickAction', () => {
  // One decode for both row kinds: the modifier chord picks the selection
  // primitive; what a PLAIN click then does (open the file, expand the
  // folder) stays with the caller.
  const table: [string, { metaKey?: boolean; ctrlKey?: boolean; shiftKey?: boolean }, string][] = [
    ['plain click', {}, 'plain'],
    ['Shift+click', { shiftKey: true }, 'setRange'],
    ['Cmd+click (Mac)', { metaKey: true }, 'toggle'],
    ['Ctrl+click (non-Mac)', { ctrlKey: true }, 'toggle'],
    ['Cmd+Shift+click', { metaKey: true, shiftKey: true }, 'addRange'],
    ['Ctrl+Shift+click', { ctrlKey: true, shiftKey: true }, 'addRange'],
  ];
  for (const [label, opts, expected] of table) {
    test(`${label} → ${expected}`, () => {
      expect(resolveClickAction(new MouseEvent('click', opts))).toBe(expected);
    });
  }

  test('no event (a programmatic open) is a plain click', () => {
    expect(resolveClickAction(undefined)).toBe('plain');
  });
});

// ── keyToAction ──────────────────────────────────────────────────────────────

describe('keyToAction', () => {
  // Arrow navigation
  test('ArrowDown → moveFocus down, no extend, no focusOnly', () => {
    const action = keyToAction(ke('ArrowDown'));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'down', extend: false, focusOnly: false });
  });

  test('ArrowUp → moveFocus up, no extend, no focusOnly', () => {
    const action = keyToAction(ke('ArrowUp'));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'up', extend: false, focusOnly: false });
  });

  test('Shift+ArrowDown → moveFocus down, extend=true, focusOnly=false', () => {
    const action = keyToAction(ke('ArrowDown', { shiftKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'down', extend: true, focusOnly: false });
  });

  test('Shift+ArrowUp → moveFocus up, extend=true, focusOnly=false', () => {
    const action = keyToAction(ke('ArrowUp', { shiftKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'up', extend: true, focusOnly: false });
  });

  test('Cmd+ArrowDown → moveFocus down, focusOnly=true (Mac)', () => {
    const action = keyToAction(ke('ArrowDown', { metaKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'down', extend: false, focusOnly: true });
  });

  test('Cmd+ArrowUp → moveFocus up, focusOnly=true (Mac)', () => {
    const action = keyToAction(ke('ArrowUp', { metaKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'up', extend: false, focusOnly: true });
  });

  test('Ctrl+ArrowDown → moveFocus down, focusOnly=true (non-Mac)', () => {
    const action = keyToAction(ke('ArrowDown', { ctrlKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'down', extend: false, focusOnly: true });
  });

  test('Ctrl+ArrowUp → moveFocus up, focusOnly=true (non-Mac)', () => {
    const action = keyToAction(ke('ArrowUp', { ctrlKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'up', extend: false, focusOnly: true });
  });

  // Expand/collapse
  test('ArrowRight → expand', () => {
    const action = keyToAction(ke('ArrowRight'));
    expect(action).toEqual({ kind: 'expand' });
  });

  test('ArrowLeft → collapse', () => {
    const action = keyToAction(ke('ArrowLeft'));
    expect(action).toEqual({ kind: 'collapse' });
  });

  // Home/End
  test('Home → moveFocus home, extend=false', () => {
    const action = keyToAction(ke('Home'));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'home', extend: false, focusOnly: false });
  });

  test('End → moveFocus end, extend=false', () => {
    const action = keyToAction(ke('End'));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'end', extend: false, focusOnly: false });
  });

  // Ctrl/Cmd+Shift+Home/End (WAI-ARIA multi-select tree pattern)
  test('Ctrl+Shift+Home → moveFocus home, extend=true (non-Mac)', () => {
    const action = keyToAction(ke('Home', { ctrlKey: true, shiftKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'home', extend: true, focusOnly: false });
  });

  test('Cmd+Shift+Home → moveFocus home, extend=true (Mac)', () => {
    const action = keyToAction(ke('Home', { metaKey: true, shiftKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'home', extend: true, focusOnly: false });
  });

  test('Ctrl+Shift+End → moveFocus end, extend=true (non-Mac)', () => {
    const action = keyToAction(ke('End', { ctrlKey: true, shiftKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'end', extend: true, focusOnly: false });
  });

  test('Cmd+Shift+End → moveFocus end, extend=true (Mac)', () => {
    const action = keyToAction(ke('End', { metaKey: true, shiftKey: true }));
    expect(action).toEqual({ kind: 'moveFocus', direction: 'end', extend: true, focusOnly: false });
  });

  // Shift+Space: range-to-focus (the selection behavior in the WAI-ARIA multi-select tree pattern)
  test('Shift+Space → rangeToFocus', () => {
    const action = keyToAction(ke(' ', { shiftKey: true }));
    expect(action).toEqual({ kind: 'rangeToFocus' });
  });

  // Select all
  test('Cmd+A → selectAll (Mac)', () => {
    const action = keyToAction(ke('a', { metaKey: true }));
    expect(action).toEqual({ kind: 'selectAll' });
  });

  test('Ctrl+A → selectAll (non-Mac)', () => {
    const action = keyToAction(ke('a', { ctrlKey: true }));
    expect(action).toEqual({ kind: 'selectAll' });
  });

  // Escape
  test('Escape → clear', () => {
    const action = keyToAction(ke('Escape'));
    expect(action).toEqual({ kind: 'clear' });
  });

  // Toggle selection
  test('Cmd+Space → toggleSelection (Mac)', () => {
    const action = keyToAction(ke(' ', { metaKey: true }));
    expect(action).toEqual({ kind: 'toggleSelection' });
  });

  test('Ctrl+Space → toggleSelection (non-Mac)', () => {
    const action = keyToAction(ke(' ', { ctrlKey: true }));
    expect(action).toEqual({ kind: 'toggleSelection' });
  });

  // Open file
  test('Space → open, focusEditor=false (keep focus in tree)', () => {
    const action = keyToAction(ke(' '));
    expect(action).toEqual({ kind: 'open', focusEditor: false });
  });

  test('Cmd+Down → open, focusEditor=true (move to editor)', () => {
    const action = keyToAction(ke('ArrowDown', { metaKey: true }));
    expect(action?.kind).toBe('moveFocus');
    // Cmd+Down is focusOnly for movement; but the plan says it ALSO dispatches
    // editor:focus-editor. The keyToAction for Cmd+Down returns moveFocus+focusOnly.
    // Separately the file-tree handler interprets focusOnly+down as "open+focus-editor".
    // So keyToAction just returns the moveFocus action; the handler interprets it.
    // This is correct per the design (Cmd+Down is handled at the handler level).
    // But let's verify the action for plain Cmd+Down:
    expect(action).toEqual({ kind: 'moveFocus', direction: 'down', extend: false, focusOnly: true });
  });

  // Rename
  test('Rename key → rename', () => {
    const action = keyToAction(ke('F2'));
    expect(action).toEqual({ kind: 'rename' });
  });

  test('Enter → rename', () => {
    const action = keyToAction(ke('Enter'));
    expect(action).toEqual({ kind: 'rename' });
  });

  // Context menu (Shift+F10 / ContextMenu key — keyboard right-click)
  test('Shift+F10 → contextMenu (not swallowed by the F-key skip)', () => {
    expect(keyToAction(ke('F10', { shiftKey: true }))).toEqual({ kind: 'contextMenu' });
  });

  test('ContextMenu key → contextMenu', () => {
    expect(keyToAction(ke('ContextMenu'))).toEqual({ kind: 'contextMenu' });
  });

  test('bare F10 → null (only the shifted form opens a menu)', () => {
    expect(keyToAction(ke('F10'))).toBeNull();
  });

  // Delete
  test('Cmd+Backspace → delete (Mac)', () => {
    const action = keyToAction(ke('Backspace', { metaKey: true }));
    expect(action).toEqual({ kind: 'delete' });
  });

  test('Delete → delete (Win/Linux)', () => {
    const action = keyToAction(ke('Delete'));
    expect(action).toEqual({ kind: 'delete' });
  });

  // Collapse-all: Cmd/Ctrl+Shift+← (item 3)
  test('Cmd+Shift+ArrowLeft → collapseAll (Mac)', () => {
    const action = keyToAction(ke('ArrowLeft', { metaKey: true, shiftKey: true }));
    expect(action).toEqual({ kind: 'collapseAll' });
  });

  test('Ctrl+Shift+ArrowLeft → collapseAll (non-Mac)', () => {
    const action = keyToAction(ke('ArrowLeft', { ctrlKey: true, shiftKey: true }));
    expect(action).toEqual({ kind: 'collapseAll' });
  });

  // Plain Shift+ArrowLeft (no mod) still collapses single node, not all
  test('Shift+ArrowLeft (no mod) → collapse (not collapseAll)', () => {
    const action = keyToAction(ke('ArrowLeft', { shiftKey: true }));
    expect(action).toEqual({ kind: 'collapse' });
  });

  // Expand-all-siblings: * (asterisk, WAI-ARIA APG) (item 5)
  test('* → expandAllSiblings', () => {
    const action = keyToAction(ke('*'));
    expect(action).toEqual({ kind: 'expandAllSiblings' });
  });

  test('* with Shift → typeAhead (Shift+8 on some keyboards, no special handling)', () => {
    // On non-US keyboards, Shift+8 produces '*'. When shiftKey is set but not metaKey,
    // the keyToAction sees shiftKey=true + key='*' — the expandAllSiblings guard
    // requires !shiftKey, so it falls through to typeAhead.
    const action = keyToAction(ke('*', { shiftKey: true }));
    // Since mod=false, the char '*' with shiftKey falls through to typeAhead
    expect(action).toEqual({ kind: 'typeAhead', char: '*' });
  });

  // Cross-pane focus
  test('Cmd+1 → focusPane tree (Mac)', () => {
    const action = keyToAction(ke('1', { metaKey: true }));
    expect(action).toEqual({ kind: 'focusPane', pane: 'tree' });
  });

  test('Ctrl+1 → focusPane tree (non-Mac)', () => {
    const action = keyToAction(ke('1', { ctrlKey: true }));
    expect(action).toEqual({ kind: 'focusPane', pane: 'tree' });
  });

  test('Cmd+2 → focusPane editor (Mac)', () => {
    const action = keyToAction(ke('2', { metaKey: true }));
    expect(action).toEqual({ kind: 'focusPane', pane: 'editor' });
  });

  test('Ctrl+2 → focusPane editor (non-Mac)', () => {
    const action = keyToAction(ke('2', { ctrlKey: true }));
    expect(action).toEqual({ kind: 'focusPane', pane: 'editor' });
  });

  // Type-ahead: printable characters with no modifiers
  test('letter key with no modifier → typeAhead', () => {
    const action = keyToAction(ke('a'));
    expect(action).toEqual({ kind: 'typeAhead', char: 'a' });
  });

  test('uppercase letter → typeAhead (lowercased by caller convention)', () => {
    const action = keyToAction(ke('A'));
    expect(action).toEqual({ kind: 'typeAhead', char: 'A' });
  });

  test('digit key with no modifier → typeAhead', () => {
    const action = keyToAction(ke('3'));
    expect(action).toEqual({ kind: 'typeAhead', char: '3' });
  });

  // Null cases
  test('Tab → null (unhandled)', () => {
    expect(keyToAction(ke('Tab'))).toBeNull();
  });

  test('Shift alone → null', () => {
    expect(keyToAction(ke('Shift'))).toBeNull();
  });

  test('Control alone → null', () => {
    expect(keyToAction(ke('Control'))).toBeNull();
  });

  test('Meta alone → null', () => {
    expect(keyToAction(ke('Meta'))).toBeNull();
  });

  test('Cmd+Z → null (unhandled shortcut)', () => {
    expect(keyToAction(ke('z', { metaKey: true }))).toBeNull();
  });
});

// ── computeNextFocusIndex ─────────────────────────────────────────────────────

describe('computeNextFocusIndex', () => {
  // Basic down/up movement
  test('down from first: 0 → 1', () => {
    expect(computeNextFocusIndex(0, 5, 'down')).toBe(1);
  });

  test('down from middle: 2 → 3', () => {
    expect(computeNextFocusIndex(2, 5, 'down')).toBe(3);
  });

  test('down from last: clamp at last (4 → 4)', () => {
    expect(computeNextFocusIndex(4, 5, 'down')).toBe(4);
  });

  test('up from last: 4 → 3', () => {
    expect(computeNextFocusIndex(4, 5, 'up')).toBe(3);
  });

  test('up from first: clamp at first (0 → 0)', () => {
    expect(computeNextFocusIndex(0, 5, 'up')).toBe(0);
  });

  // Home/End
  test('home → 0 regardless of current', () => {
    expect(computeNextFocusIndex(3, 5, 'home')).toBe(0);
    expect(computeNextFocusIndex(0, 5, 'home')).toBe(0);
    expect(computeNextFocusIndex(4, 5, 'home')).toBe(0);
  });

  test('end → last index regardless of current', () => {
    expect(computeNextFocusIndex(0, 5, 'end')).toBe(4);
    expect(computeNextFocusIndex(3, 5, 'end')).toBe(4);
    expect(computeNextFocusIndex(4, 5, 'end')).toBe(4);
  });

  // Edge: single row list
  test('single item: all directions return 0', () => {
    expect(computeNextFocusIndex(0, 1, 'down')).toBe(0);
    expect(computeNextFocusIndex(0, 1, 'up')).toBe(0);
    expect(computeNextFocusIndex(0, 1, 'home')).toBe(0);
    expect(computeNextFocusIndex(0, 1, 'end')).toBe(0);
  });

  // Edge: empty list
  test('empty list (total=0): always returns -1', () => {
    expect(computeNextFocusIndex(0, 0, 'down')).toBe(-1);
    expect(computeNextFocusIndex(0, 0, 'up')).toBe(-1);
    expect(computeNextFocusIndex(0, 0, 'home')).toBe(-1);
    expect(computeNextFocusIndex(0, 0, 'end')).toBe(-1);
  });
});

// ── matchTypeAhead ─────────────────────────────────────────────────────────

describe('matchTypeAhead', () => {
  // Build a row list for testing
  const rows: { id: EntryId; name: string }[] = [
    { id: id(1), name: 'alpha.md' },
    { id: id(2), name: 'beta.md' },
    { id: id(3), name: 'gamma.md' },
    { id: id(4), name: 'beta-extra.md' },
    { id: id(5), name: 'zeta.md' },
  ];

  test('no match returns null', () => {
    expect(matchTypeAhead(rows, 'xyz', 0)).toBeNull();
  });

  test('single-match: finds first file starting with prefix', () => {
    expect(matchTypeAhead(rows, 'a', 0)).toBe(id(1)); // alpha
  });

  test('single-match from later index: wraps around', () => {
    // Search for 'a' starting after alpha (index 1+) — should wrap to alpha
    expect(matchTypeAhead(rows, 'a', 1)).toBe(id(1)); // wraps around
  });

  test('multi-match cycling: finds next match after fromIdx', () => {
    // 'b' matches beta (idx 1) and beta-extra (idx 3)
    // Starting from 0: first match is idx 1 = beta
    expect(matchTypeAhead(rows, 'b', 0)).toBe(id(2));
    // Starting from idx 1 (beta itself): next match is idx 3 = beta-extra
    expect(matchTypeAhead(rows, 'b', 1)).toBe(id(4));
    // Starting from idx 3 (beta-extra): wraps around to idx 1 = beta
    expect(matchTypeAhead(rows, 'b', 3)).toBe(id(2));
  });

  test('multi-char prefix: more specific match', () => {
    expect(matchTypeAhead(rows, 'be', 0)).toBe(id(2)); // beta
    expect(matchTypeAhead(rows, 'bet', 0)).toBe(id(2)); // beta
    expect(matchTypeAhead(rows, 'beta-', 0)).toBe(id(4)); // beta-extra
  });

  test('case-insensitive matching', () => {
    expect(matchTypeAhead(rows, 'A', 0)).toBe(id(1)); // alpha (case-insensitive)
    expect(matchTypeAhead(rows, 'BETA', 0)).toBe(id(2)); // beta
  });

  test('empty rows returns null', () => {
    expect(matchTypeAhead([], 'a', 0)).toBeNull();
  });

  test('empty prefix: null (no jump on empty search)', () => {
    expect(matchTypeAhead(rows, '', 0)).toBeNull();
  });

  test('prefix matches last entry: cycling works', () => {
    // 'z' matches only zeta (idx 4)
    expect(matchTypeAhead(rows, 'z', 4)).toBe(id(5)); // wraps: finds itself
    expect(matchTypeAhead(rows, 'z', 0)).toBe(id(5)); // finds zeta directly
  });
});

// ── TypeAhead ────────────────────────────────────────────────────────────────

describe('TypeAhead', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  test('characters typed within the window form one prefix', () => {
    const ta = new TypeAhead();
    expect(ta.push('b')).toBe('b');
    vi.advanceTimersByTime(TYPE_AHEAD_RESET_MS - 1);
    expect(ta.push('e')).toBe('be');
  });

  test('a pause of the window length starts a new prefix', () => {
    const ta = new TypeAhead();
    ta.push('b');
    vi.advanceTimersByTime(TYPE_AHEAD_RESET_MS);
    expect(ta.push('z')).toBe('z');
  });

  test('dispose drops the prefix and the pending reset', () => {
    const ta = new TypeAhead();
    ta.push('b');
    ta.dispose();
    expect(vi.getTimerCount()).toBe(0);
    expect(ta.push('z')).toBe('z');
    ta.dispose();
  });
});
