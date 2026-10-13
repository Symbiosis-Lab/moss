// Shared active-line predicate — the primitive behind one rule: colour-only
// feedback, such as a host's marks on a broken or unresolved reference, may
// wait until the caret leaves the construct being typed. Presentation itself
// never reads the selection; this predicate exists only for that exception.
//
// Lives in its own module (not the consumer) so multiple feedback modules
// (cm-link-clickable, cm-footnote) can share one
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
