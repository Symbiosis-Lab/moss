// Who owns ⌘Z — the pure decision, with no DOM writes and no history calls.
//
// WHY THIS IS TWO QUESTIONS AND NOT ONE PER SURFACE
//
// Asking about surfaces by name (chip popover open? heading focused? heading
// bound to Title or Filename? state loaded?) makes the router reason about
// other modules' internals from outside. In particular the heading commits on
// BLUR, so a focused heading may hold text the store has never seen: "has a
// pending edit" is not the same question as "has focus".
//
// So the router asks two questions in which no surface's name appears:
//
//   1. Does anything hold an edit the document log has not seen? If so it must
//      be committed first — and that commit IS the history entry, so the key is
//      spent. Only the surface can answer this; `editor-main` collects the
//      answers.
//   2. Does the focused field own a history the document log does not? Plain
//      inputs and the heading in Filename mode do (a rename is not expressible
//      as a log entry). The CM6 body does NOT, even though its contentDOM is
//      itself contenteditable — that is the one trap, and it is the adapter's
//      job rather than a rule encoded in the order of if-statements.
//
// Anything else: the document's history is the one meant.
//
// The payoff is that the NEXT surface with commit-on-blur state does not need a
// route of its own, or an edit to this file. It answers question 1.

/** What the caller should do with this ⌘Z / ⌘⇧Z. */
export type HistoryRoute =
  /** Commit the pending edit; that commit is the step, so nothing is walked. */
  | 'flush'
  /** Walk the combined document history. */
  | 'document'
  /** Let the focused form field's own history answer (execCommand). */
  | 'native';

export interface HistoryRouteInput {
  /**
   * Some surface holds an edit the document log has not been told about — an
   * open chip popover mid-visit, a heading typed into but not yet blurred.
   */
  pendingEdit: boolean;
  /**
   * Focus is in a field whose undo history the document log does not own.
   *
   * True for plain inputs/textareas and for the heading in Filename mode (that
   * commit is a RENAME, not expressible as a log entry — and a user who renames
   * and presses ⌘Z must not get a frontmatter revert that looks like a failed
   * rename). False for the CM6 body, whose edits the log does own.
   */
  nativeOwner: boolean;
}

/**
 * Decide who answers. The order IS the design:
 *
 * 1. A pending edit outranks everything, including focus on nothing at all. A
 *    popover can hold focus nowhere (`showEdit` skips autofocus for the union
 *    widget), and reaching past it would revert an older change while the
 *    popover sat over the result with an uncommitted snapshot.
 * 2. Then whoever owns their own history keeps it — and that propagates:
 *    execCommand fires a real input event, which re-runs the widget listener
 *    and re-commits through the store.
 * 3. Otherwise the document's history is the one meant.
 */
export function routeHistoryKey(i: HistoryRouteInput): HistoryRoute {
  if (i.pendingEdit) return 'flush';
  if (i.nativeOwner) return 'native';
  return 'document';
}

/** The environment the two inputs are read out of. */
export interface HistoryEnvironment {
  /** `document.activeElement` at the moment the key arrived. */
  activeElement: Element | null;
  /**
   * Does the CM6 body hold focus? Asked separately because its `contentDOM` IS
   * contenteditable and therefore matches FORM_FIELD_SELECTOR — letting that
   * stand would hand the body's ⌘Z to CodeMirror's own history and undo the
   * body alone, the exact split this feature exists to remove.
   */
  cmBodyHasFocus: boolean;
  /** One flag per surface that can hold an edit the log has not seen. */
  pending: boolean[];
  /**
   * Fields that LOOK native but whose history the document log owns — today
   * just the heading while it is bound to Title. `null` entries are surfaces
   * that do not currently qualify (the heading in Filename mode, or before its
   * state has loaded), so the caller can map without branching.
   */
  logOwned: (Element | null)[];
}

/**
 * Read the two inputs out of the environment.
 *
 * Split from `routeHistoryKey` because this half is where the defects actually
 * were, and where they were untestable: it lived as one expression inside
 * `editor-main`, reachable only through a booted editor. Four mutation probes
 * could change it — including deleting the CM6-body exclusion outright —
 * without failing the editor suite. The decision below
 * is small enough to check by reading; this mapping is not.
 */
export function computeHistoryInputs(env: HistoryEnvironment): HistoryRouteInput {
  const active = env.activeElement;
  return {
    pendingEdit: env.pending.some(Boolean),
    nativeOwner:
      (active?.matches(FORM_FIELD_SELECTOR) ?? false) &&
      !env.cmBodyHasFocus &&
      !env.logOwned.some((el) => el !== null && el === active),
  };
}

/**
 * The selector for "a field with its own undo history".
 *
 * Attribute-PRESENCE form on purpose: the editor heading is
 * `contenteditable="plaintext-only"` and does not match
 * `[contenteditable="true"]`, which is a bug that already exists elsewhere in
 * the editor (file-tree.ts's drag guard) and must not be copied here.
 */
export const FORM_FIELD_SELECTOR =
  'input, textarea, [contenteditable]:not([contenteditable="false"])';
