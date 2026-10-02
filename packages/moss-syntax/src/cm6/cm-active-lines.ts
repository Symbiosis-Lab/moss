// Shared active-line predicate — the primitive the editor-wide colour-only
// suppression contract uses (design-vocabulary.md: "Colour-only feedback —
// diagnostics, unresolved-link dimming — may wait until the caret leaves the
// construct being typed"). Presentation itself never reads the selection;
// this predicate exists only for that one allowlisted exception.
//
// Lives in its own module (not the consumer) so multiple feedback modules
// (cm-link-resolver, cm-footnote in moss-desktop's callers) can share one
// predicate without an import cycle.

import type { EditorState } from '@codemirror/state';

/**
 * True if any selection range overlaps the closed interval [from, to].
 *
 * Boundary-inclusive (non-strict `<=`): a collapsed caret parked exactly at
 * `from` or `to` counts as touching, so a node revealed on edge contact is
 * editable. Strict operators are deliberately avoided — they cause the
 * "markers re-hide and trap the cursor outside the run" class of bugs.
 */
export function nodeTouchesSelection(state: EditorState, from: number, to: number): boolean {
  return state.selection.ranges.some((r) => from <= r.to && r.from <= to);
}
