// Document-wide SOURCE MODE — the explicit escape hatch the live view keeps
// (docs/archive/2026-08-14-raw-markup-token-highlight.md, "source mode
// approved"). In source mode the whole document behaves as if the selection
// touched every line: no widget replacements, no hidden spans — raw source,
// token-highlighted with the SAME classes the revealed states already use.
//
// This module holds the state and the shared rebuild gate. The mode is
// threaded through ONE point — the shared active-line predicates in
// cm-active-lines.ts — so every decoration builder that honors the reveal
// contract honors source mode for free. There is no parallel "source mode"
// extension stack, and the reveal rules are not forked.
//
// Per-file and non-sticky by construction: the field's create() value is
// false and each file open builds a fresh EditorState (loadFile remounts
// CmEditor), so reopening or switching files always rests in live view.
// Nothing is persisted.

import { EditorState, StateEffect, StateField, type Transaction } from '@codemirror/state';
import type { ViewUpdate } from '@codemirror/view';
import { revealSuspensionChanged } from './cm-editor-focus.js';

/** Set source mode on/off. Effect-only transactions (no doc change) — the
 *  save state machine never sees a toggle. */
export const setSourceModeEffect = StateEffect.define<boolean>();

export const sourceModeField = StateField.define<boolean>({
  create: () => false,
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setSourceModeEffect)) value = e.value;
    }
    return value;
  },
});

/** True when the document is in source mode. Safe on states without the
 *  field (non-markdown editors don't register it) — defaults to false. */
export function isSourceMode(state: EditorState): boolean {
  return state.field(sourceModeField, false) ?? false;
}

/**
 * True when this transaction flipped source mode. Decoration StateFields and
 * ViewPlugins gate their rebuilds on doc/selection changes; a toggle changes
 * neither, so every builder that reads the predicates must ALSO rebuild on
 * this — otherwise the mode switch would not repaint until the next keystroke.
 */
export function sourceModeChanged(tr: Transaction): boolean {
  return isSourceMode(tr.startState) !== isSourceMode(tr.state);
}

/**
 * The one rebuild gate for every consumer of the reveal predicates: did this
 * transaction change anything getActiveLines / nodeTouchesSelection reads?
 * Doc, selection — and the mode flip and the editor gaining or losing focus,
 * which are effect-only and therefore invisible to the doc/selection checks
 * alone. Builders compose their own
 * extras (treeAdvanced, refsResolved) on top; they must never re-spell this
 * set, because a hand-written gate is how four consumers shipped stale
 * decorations across the flip (thermo review, 2026-09-01).
 */
export function revealInputsChanged(tr: Transaction): boolean {
  return tr.docChanged || !!tr.selection || sourceModeChanged(tr) || revealSuspensionChanged(tr);
}

/** ViewUpdate-shaped twin of revealInputsChanged, for ViewPlugin update gates. */
export function revealInputsChangedIn(update: ViewUpdate): boolean {
  return update.docChanged || update.selectionSet
    || update.transactions.some((tr) => sourceModeChanged(tr) || revealSuspensionChanged(tr));
}
