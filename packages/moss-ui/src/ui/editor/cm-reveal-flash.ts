// Reveal flash — the editor-side answer to a SINGLE click in the preview.
//
// Preview-click vocabulary:
// a single click on annotated preview content ORIENTS — the editor scrolls the
// mapped source into view and washes it with a brief highlight, while focus
// stays in the preview. The full jump (focus + caret) is the DOUBLE click and
// lives in gotoLine (cm-scroll-sync.ts); nothing here ever moves the caret or
// takes focus.
//
// The wash is a line Decoration driven by a StateEffect — a view-only change.
// It must never be a document change: the updateListener in cm-editor.ts keys
// auto-save off `docChanged`, and a flash that dirtied the save state machine
// would schedule a spurious write on every preview click.

import { StateField, StateEffect, type ChangeSet, type Text } from '@codemirror/state';
import { EditorView, Decoration, type DecorationSet } from '@codemirror/view';

/** How long the wash stays mounted. Matches the 1.4s `moss-revealed` wash on
 *  chips so both ends of the gesture read as one vocabulary; the CSS animation
 *  fades within this window, and under prefers-reduced-motion the decoration's
 *  removal is the (jump-cut) end of the highlight. */
export const REVEAL_FLASH_MS = 1400;

/** How long the wash over lines another writer changed stays;
 *  `.cm-outside-wash` in editor.css fades over the same time. */
export const OUTSIDE_WASH_MS = 2000;

/** A run of 1-based, inclusive line numbers. */
export interface LineRange { fromLine: number; toLine: number }

/** Set the flashed source range, or clear with null. */
export const setRevealFlash = StateEffect.define<LineRange | null>();
/** Wash the lines another writer's merged edit (or the person's Undo of one) changed. */
export const setOutsideWash = StateEffect.define<readonly LineRange[]>();

const flashLineDeco = Decoration.line({ class: 'cm-reveal-flash' });
const outsideLineDeco = Decoration.line({ class: 'cm-reveal-flash cm-outside-wash' });

function lineDecos(doc: Text, ranges: readonly LineRange[], deco: Decoration): DecorationSet {
  const starts = new Set<number>();
  for (const r of ranges) {
    const from = Math.max(1, Math.min(r.fromLine, doc.lines));
    const to = Math.max(from, Math.min(r.toLine, doc.lines));
    for (let n = from; n <= to; n++) starts.add(doc.line(n).from);
  }
  return Decoration.set([...starts].sort((a, b) => a - b).map((at) => deco.range(at)));
}

/** Holds the flashed range as a DecorationSet of line decorations. */
export const revealFlashField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    if (tr.docChanged) value = value.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(setRevealFlash)) value = e.value ? lineDecos(tr.state.doc, [e.value], flashLineDeco) : Decoration.none;
      if (e.is(setOutsideWash)) value = lineDecos(tr.state.doc, e.value, outsideLineDeco);
    }
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** Extension to register in cm-editor.ts (all file types). */
export function revealFlashExtension(): typeof revealFlashField[] {
  return [revealFlashField];
}

/** One missing reference the last build recorded, placed in the open
 *  document; cm-problems.ts marks it. */
export interface PublishPreflightHint {
  from: number;
  to: number;
  reference: string;
}

export interface PublishPreflightEvidence {
  missing_references: readonly {
    source_path: string;
    source_revision: string;
    source_span: { start_byte: number; end_byte: number; line: number };
    reference: string;
  }[];
}

export interface PublishPreflightOpen {
  sourceRevision: string;
  sourceSpan: { start_byte: number; end_byte: number; line: number };
}

/** Exact raw-source byte → normalized CM body coordinate mapping. */
export function sourceSpanToBodyRange(raw: string, body: string, span: { start_byte: number; end_byte: number }): { from: number; to: number } | null {
  const bodyStart = frontmatterBodyStartByte(raw);
  const rawStart = utf8ByteToUtf16(raw, bodyStart);
  if (rawStart === null) return null;
  const rawBody = raw.slice(rawStart);
  const fromRaw = utf8ByteToUtf16(rawBody, span.start_byte - bodyStart);
  const toRaw = utf8ByteToUtf16(rawBody, span.end_byte - bodyStart);
  if (fromRaw === null || toRaw === null || toRaw < fromRaw) return null;
  const from = rawBody.slice(0, fromRaw).replace(/\r\n?/g, '\n').length;
  const to = rawBody.slice(0, toRaw).replace(/\r\n?/g, '\n').length;
  return to <= body.length ? { from, to } : null;
}

function frontmatterBodyStartByte(raw: string): number {
  if (!(raw.startsWith('---\n') || raw.startsWith('---\r\n'))) return 0;
  const match = /^(---\r?\n)[\s\S]*?\r?\n---\r?\n/.exec(raw);
  return match ? new TextEncoder().encode(match[0]).length : 0;
}

/** Null means a backend byte span is not a safe UTF-8 boundary. */
export function utf8ByteToUtf16(text: string, byte: number): number | null {
  if (byte < 0) return null;
  let bytes = 0;
  let offset = 0;
  for (const cp of text) {
    if (bytes === byte) return offset;
    bytes += new TextEncoder().encode(cp).length;
    offset += cp.length;
    if (bytes > byte) return null;
  }
  return bytes === byte ? offset : null;
}

export async function sha256Lower(text: string): Promise<string> {
  const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(bytes)].map((n) => n.toString(16).padStart(2, '0')).join('');
}

export async function preflightHints(
  projection: PublishPreflightEvidence | null,
  path: string,
  raw: string,
  body: string,
): Promise<PublishPreflightHint[]> {
  const occurrences = projection?.missing_references.filter((occurrence) => occurrence.source_path === path) ?? [];
  if (occurrences.length === 0 || (await sha256Lower(raw)).toLowerCase() !== occurrences[0].source_revision.toLowerCase()) return [];
  return occurrences.flatMap((occurrence) => {
    if (occurrence.source_revision.toLowerCase() !== occurrences[0].source_revision.toLowerCase()) return [];
    const range = sourceSpanToBodyRange(raw, body, occurrence.source_span);
    return range ? [{ ...range, reference: occurrence.reference }] : [];
  });
}

export async function revealPublishPreflight(
  view: EditorView,
  raw: string,
  body: string,
  request: PublishPreflightOpen,
): Promise<'matched' | 'mismatch' | 'invalid'> {
  if ((await sha256Lower(raw)).toLowerCase() !== request.sourceRevision.toLowerCase()) return 'mismatch';
  const range = sourceSpanToBodyRange(raw, body, request.sourceSpan);
  if (!range) return 'invalid';
  view.dispatch({ selection: { anchor: range.from, head: range.to }, scrollIntoView: true });
  const first = view.state.doc.lineAt(range.from).number;
  revealLines(view, first, view.state.doc.lineAt(range.to).number);
  view.focus();
  return 'matched';
}

/**
 * "Never intercept the expert": when the editor itself has focus AND the save
 * state machine holds a pending edit, a preview click may still flash the
 * mapped lines, but it must not move the caret or the scroll position the
 * user is typing at. (Focus alone is not enough — a focused-but-clean editor
 * is not mid-thought, and the click deserves its reveal.)
 */
export function shouldPreserveEditorPosition(
  hasFocus: boolean,
  saveState: 'clean' | 'dirty' | 'saving' | 'error',
): boolean {
  return hasFocus && (saveState === 'dirty' || saveState === 'saving');
}

// One pending clear-timer per view; a second reveal restarts the window.
const clearTimers = new WeakMap<EditorView, ReturnType<typeof setTimeout>>();

/**
 * Scroll the mapped source into view (centered) and flash it. No selection
 * change, no focus() — the single-click reveal by construction cannot steal
 * the caret. Pass `preserveScroll: true` (see shouldPreserveEditorPosition)
 * to show the flash without touching the scroll position.
 */
export function revealLines(
  view: EditorView,
  startLine: number,
  endLine: number,
  opts: { preserveScroll?: boolean } = {},
): void {
  const doc = view.state.doc;
  const from = Math.max(1, Math.min(startLine, doc.lines));
  const to = Math.max(from, Math.min(endLine, doc.lines));

  const effects: StateEffect<unknown>[] = [
    setRevealFlash.of({ fromLine: from, toLine: to }),
  ];
  if (!opts.preserveScroll) {
    // scrollIntoView-the-effect (not the dispatch flag): it needs no selection
    // change and handles positions outside the rendered viewport.
    effects.push(EditorView.scrollIntoView(doc.line(from).from, { y: 'center' }));
  }
  view.dispatch({ effects });
  clearAfter(view, REVEAL_FLASH_MS);
}

/**
 * The wash over the lines `changes` touched, in `doc`, the document they
 * produce; dispatch it with the change, then `clearOutsideWashLater`. It
 * moves neither the scroll position nor the caret.
 */
export function outsideWashOf(changes: ChangeSet, doc: Text): StateEffect<readonly LineRange[]> | null {
  const ranges: LineRange[] = [];
  changes.iterChangedRanges((_fromA, _toA, fromB, toB) => {
    ranges.push({ fromLine: doc.lineAt(fromB).number, toLine: doc.lineAt(Math.min(toB, doc.length)).number });
  });
  return ranges.length ? setOutsideWash.of(ranges) : null;
}

export function clearOutsideWashLater(view: EditorView): void {
  clearAfter(view, OUTSIDE_WASH_MS);
}

function clearAfter(view: EditorView, ms: number): void {
  const prior = clearTimers.get(view);
  if (prior) clearTimeout(prior);
  clearTimers.set(
    view,
    setTimeout(() => {
      clearTimers.delete(view);
      // The view may have been destroyed while the timer was pending; a
      // dispatch on a destroyed view throws.
      try {
        view.dispatch({ effects: setRevealFlash.of(null) });
      } catch {
        /* destroyed mid-flash — nothing to clear */
      }
    }, ms),
  );
}
