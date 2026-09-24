// Reveal follows the caret only while the user is working in the editor.
//
// Unfocused, the editor is something being read, not edited: the whole
// document renders as live view, with no line showing raw source because the
// caret happens to rest on it. A freshly opened editor is the common case — its
// caret sits at the start of the document, so without this the first line
// always opened half-raw.
//
// Opt-in and host-driven. Hosts decide what "working in the editor" means
// (a desktop host may count its own find bar or title field as the editor), so
// this module only holds the state; the host installs `editorFocusField` and
// dispatches `setEditorFocusedEffect` as focus comes and goes. A state without
// the field reveals exactly as before. Threaded through the shared predicates
// in cm-active-lines.ts the same way source mode is, so every builder that
// honours the reveal contract honours focus with no logic of its own.

import { EditorState, StateEffect, StateField, type Transaction } from '@codemirror/state';

/** The host's report that the user started or stopped working in the editor. */
export const setEditorFocusedEffect = StateEffect.define<boolean>();

/** Install to make reveals follow focus. Starts unfocused, as a newly opened
 *  editor is until the user reaches into it. */
export const editorFocusField = StateField.define<boolean>({
  create: () => false,
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setEditorFocusedEffect)) value = e.value;
    }
    return value;
  },
});

/** True when the host installed `editorFocusField` and the editor is not
 *  focused: no line is active and no node touches the selection. */
export function isRevealSuspended(state: EditorState): boolean {
  return state.field(editorFocusField, false) === false;
}

/** True when this transaction suspended or resumed reveals — effect-only, so
 *  invisible to the doc/selection checks alone. */
export function revealSuspensionChanged(tr: Transaction): boolean {
  return isRevealSuspended(tr.startState) !== isRevealSuspended(tr.state);
}
