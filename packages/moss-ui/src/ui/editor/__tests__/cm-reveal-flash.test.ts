// Single-click reveal from the preview.
// The editor scrolls the mapped source into view and washes it
// — and by construction cannot move the caret, take focus, or dirty the save
// state machine. Double click keeps the full jump (gotoLine, tested in
// cm-scroll-sync.test.ts).
import { describe, test, expect, vi, afterEach, beforeAll } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import {
  revealFlashExtension,
  revealFlashField,
  setRevealFlash,
  revealLines,
  preflightHints,
  revealPublishPreflight,
  sha256Lower,
  shouldPreserveEditorPosition,
  REVEAL_FLASH_MS,
} from '../cm-reveal-flash';

// jsdom implements no layout; CM6's deferred measure pass (which fake timers
// flush) calls Range.getClientRects and needs a harmless answer, not a throw.
beforeAll(() => {
  const emptyRect = {
    x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0,
    toJSON: () => ({}),
  } as DOMRect;
  Range.prototype.getClientRects = () =>
    Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => emptyRect;
});

function makeView(doc: string): EditorView {
  const state = EditorState.create({ doc, extensions: [revealFlashExtension()] });
  return new EditorView({ state, parent: document.body });
}

function flashedLineCount(view: EditorView): number {
  let n = 0;
  view.state.field(revealFlashField).between(0, view.state.doc.length, () => { n++; });
  return n;
}

afterEach(() => {
  document.body.innerHTML = '';
  vi.useRealTimers();
});

describe('revealFlashField', () => {
  test('effect decorates every line of the range, clear empties it', () => {
    let s = EditorState.create({ doc: 'a\nb\nc\nd', extensions: [revealFlashExtension()] });
    s = s.update({ effects: setRevealFlash.of({ fromLine: 2, toLine: 3 }) }).state;
    let n = 0;
    s.field(revealFlashField).between(0, s.doc.length, () => { n++; });
    expect(n).toBe(2);

    s = s.update({ effects: setRevealFlash.of(null) }).state;
    n = 0;
    s.field(revealFlashField).between(0, s.doc.length, () => { n++; });
    expect(n).toBe(0);
  });

  test('an out-of-range line clamps instead of throwing (stale annotation safety)', () => {
    let s = EditorState.create({ doc: 'a\nb', extensions: [revealFlashExtension()] });
    s = s.update({ effects: setRevealFlash.of({ fromLine: 99, toLine: 120 }) }).state;
    let n = 0;
    s.field(revealFlashField).between(0, s.doc.length, () => { n++; });
    expect(n).toBe(1); // clamped to the last line, not a wrong-line throw
  });
});

describe('revealLines', () => {
  test('flashes without moving the caret, taking focus, or changing the doc', () => {
    vi.useFakeTimers();
    const view = makeView('one\ntwo\nthree\nfour');
    const before = view.state.selection.main.head;

    revealLines(view, 3, 3);

    expect(flashedLineCount(view)).toBe(1);
    // Orienting, not editing: the flash is a decoration, never a doc change or
    // a selection move — so it cannot dirty the save state machine (which keys
    // off docChanged) and cannot steal the caret.
    expect(view.state.doc.toString()).toBe('one\ntwo\nthree\nfour');
    expect(view.state.selection.main.head).toBe(before);
    expect(view.hasFocus).toBe(false);
    view.destroy();
  });

  test('the wash clears itself after REVEAL_FLASH_MS', () => {
    vi.useFakeTimers();
    const view = makeView('one\ntwo');
    revealLines(view, 2, 2);
    expect(flashedLineCount(view)).toBe(1);

    vi.advanceTimersByTime(REVEAL_FLASH_MS + 1);
    expect(flashedLineCount(view)).toBe(0);
    view.destroy();
  });

  test('a second reveal restarts the window instead of stacking timers', () => {
    vi.useFakeTimers();
    const view = makeView('one\ntwo\nthree');
    revealLines(view, 1, 1);
    vi.advanceTimersByTime(REVEAL_FLASH_MS - 100);
    revealLines(view, 3, 3);
    // The first timer must not clear the second flash early.
    vi.advanceTimersByTime(200);
    expect(flashedLineCount(view)).toBe(1);
    vi.advanceTimersByTime(REVEAL_FLASH_MS);
    expect(flashedLineCount(view)).toBe(0);
    view.destroy();
  });

  test('preserveScroll still shows the flash (never intercept, never go dark)', () => {
    vi.useFakeTimers();
    const view = makeView('one\ntwo\nthree');
    revealLines(view, 2, 2, { preserveScroll: true });
    // The expert keeps their position, but the flash still teaches where the
    // click landed.
    expect(flashedLineCount(view)).toBe(1);
    expect(view.hasFocus).toBe(false);
    view.destroy();
  });

  test('a destroyed view does not throw when the clear timer fires', () => {
    vi.useFakeTimers();
    const view = makeView('one');
    revealLines(view, 1, 1);
    view.destroy();
    expect(() => vi.advanceTimersByTime(REVEAL_FLASH_MS + 1)).not.toThrow();
  });
});

describe('shouldPreserveEditorPosition', () => {
  test('only a focused editor with a pending edit is the expert', () => {
    // The one combination that must never be scroll-jacked: focused + typing.
    expect(shouldPreserveEditorPosition(true, 'dirty')).toBe(true);
    expect(shouldPreserveEditorPosition(true, 'saving')).toBe(true);
    // Focused but clean: not mid-thought — the reveal may scroll.
    expect(shouldPreserveEditorPosition(true, 'clean')).toBe(false);
    expect(shouldPreserveEditorPosition(true, 'error')).toBe(false);
    // Unfocused (the normal cross-webview case — clicking the preview blurs
    // the editor): the reveal scrolls regardless of save state.
    expect(shouldPreserveEditorPosition(false, 'dirty')).toBe(false);
    expect(shouldPreserveEditorPosition(false, 'clean')).toBe(false);
  });
});

describe('publish evidence spans', () => {
  test('preserves duplicate exact spans and refuses a stale revision without changing the document', async () => {
    vi.useFakeTimers();
    const raw = '前\n![a](lost)\n![a](lost)';
    const first = new TextEncoder().encode('前\n').length;
    const referenceLength = new TextEncoder().encode('![a](lost)').length;
    const revision = await sha256Lower(raw);
    const projection = { missing_references: [
      { source_path: 'page.md', source_revision: revision, reference: 'lost', source_span: { start_byte: first, end_byte: first + referenceLength, line: 2 } },
      { source_path: 'page.md', source_revision: revision, reference: 'lost', source_span: { start_byte: first + referenceLength + 1, end_byte: first + referenceLength * 2 + 1, line: 3 } },
    ] };
    await expect(preflightHints(projection, 'page.md', raw, raw)).resolves.toEqual([
      { from: 2, to: 12, reference: 'lost' },
      { from: 13, to: 23, reference: 'lost' },
    ]);

    const view = makeView(raw);
    const before = view.state.selection.main;
    await expect(revealPublishPreflight(view, raw, raw, {
      sourceRevision: '0'.repeat(64), sourceSpan: projection.missing_references[0].source_span,
    })).resolves.toBe('mismatch');
    expect(view.state.doc.toString()).toBe(raw);
    expect(view.state.selection.main).toEqual(before);
    view.destroy();
  });
});
