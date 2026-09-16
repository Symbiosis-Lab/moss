// Shared active-line predicates — the primitives of the editor-wide reveal
// contract (see the cm-live-preview.ts header): any line a selection range
// touches is ACTIVE and shows raw source / full feedback suppression applies.
//
// Lives in its own module (not cm-live-preview) so feedback modules that
// cm-live-preview itself imports (cm-reference-resolver) can use the same
// predicates without an import cycle.
//
// SOURCE MODE (cm-source-mode.ts) is threaded HERE, not in each builder:
// in source mode every line is active and every node touches the selection,
// so all decoration builders that honor the reveal contract show raw,
// token-highlighted source with zero additional logic of their own.
// (Retired 2026-08-18 for its affordance, revived 2026-09-01 with the mode
// control — docs/archive/2026-08-31-editor-modes-and-chip-bar-redesign.md §2.)

import type { EditorState } from '@codemirror/state';
import { isSourceMode } from './cm-source-mode.js';

/** Returns the set of 1-based line numbers that contain any selection range.
 *  In source mode: every line of the document. */
export function getActiveLines(state: EditorState): Set<number> {
  const lines = new Set<number>();
  if (isSourceMode(state)) {
    for (let l = 1; l <= state.doc.lines; l++) lines.add(l);
    return lines;
  }
  for (const range of state.selection.ranges) {
    const startLine = state.doc.lineAt(range.from).number;
    const endLine = state.doc.lineAt(range.to).number;
    for (let l = startLine; l <= endLine; l++) {
      lines.add(l);
    }
  }
  return lines;
}

/** Returns true if any line of the node range overlaps with active lines. */
export function isNodeActive(
  state: EditorState,
  from: number,
  to: number,
  activeLines: Set<number>,
): boolean {
  const startLine = state.doc.lineAt(from).number;
  const endLine = state.doc.lineAt(to).number;
  for (let l = startLine; l <= endLine; l++) {
    if (activeLines.has(l)) return true;
  }
  return false;
}

/** True when [from, to] touches any line a selection range is on. */
export function spanOnActiveLine(state: EditorState, from: number, to: number): boolean {
  return isNodeActive(state, from, to, getActiveLines(state));
}

/**
 * True if any selection range overlaps the closed interval [from, to].
 *
 * Boundary-inclusive (non-strict `<=`): a collapsed caret parked exactly at
 * `from` or `to` counts as touching, so a node revealed on edge contact is
 * editable. Strict operators are deliberately avoided — they cause the
 * "markers re-hide and trap the cursor outside the run" class of bugs.
 *
 * This is the inline (per-node) reveal predicate — the granular counterpart to
 * isNodeActive's per-line test. Used by cm-live-preview (Emphasis/Strong/
 * Strikethrough/InlineCode/Link/Wikilink) and cm-link-resolver (link feedback).
 */
export function nodeTouchesSelection(state: EditorState, from: number, to: number): boolean {
  if (isSourceMode(state)) return true;
  return state.selection.ranges.some((r) => from <= r.to && r.from <= to);
}
