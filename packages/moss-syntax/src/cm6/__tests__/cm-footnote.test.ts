/**
 * Rendering tests for footnotes in live preview.
 *
 * The decisive one is `an undefined marker is left completely alone` — it is
 * the behavior that keeps `[^0-9]` in a sentence about regexes from being
 * quietly turned into a footnote, and it mirrors what the build does
 * (crates/moss-core/tests/footnotes_and_strikethrough.rs).
 */

import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { footnoteConfig } from '../../footnote-grammar.js';
import { getActiveLines } from '../cm-active-lines.js';
import { setSourceModeEffect, sourceModeField } from '../cm-source-mode.js';
import { footnoteIndex, footnoteJumpTarget, footnoteDecorations, footnoteExtension } from '../cm-footnote.js';

function stateFor(doc: string, cursor?: number): EditorState {
  return EditorState.create({
    doc,
    selection: { anchor: cursor ?? doc.length },
    extensions: [markdown({ extensions: [footnoteConfig] })],
  });
}

interface Flat { from: number; to: number; cls?: string; hidden: boolean }

function decosFor(doc: string, cursor?: number): Flat[] {
  const state = stateFor(doc, cursor);
  return footnoteDecorations(state, getActiveLines(state)).map((d) => {
    const spec = d.deco.spec as { class?: string };
    return { from: d.from, to: d.to, cls: spec.class, hidden: spec.class == null };
  });
}

// Class containment, not equality: a rendered label carries a second class
// when it can jump back to a reference, and that is a separate assertion.
const hasMark = (d: Flat[], cls: string, from: number, to: number) =>
  d.some((x) => x.cls?.split(' ').includes(cls) && x.from === from && x.to === to);
const hasHidden = (d: Flat[], from: number, to: number) =>
  d.some((x) => x.hidden && x.from === from && x.to === to);

// `A[^1] text.` + blank + `[^1]: the note` — offsets are ASCII so they read plainly.
const DOC = 'A[^1] text.\n\n[^1]: the note';

describe('markers', () => {
  test('a defined marker hides its brackets and raises its label', () => {
    const d = decosFor(DOC, 0);
    expect(hasHidden(d, 1, 3)).toBe(true);                     // "[^"
    expect(hasHidden(d, 4, 5)).toBe(true);                     // "]"
    expect(hasMark(d, 'cm-lp-footnote-ref', 3, 4)).toBe(true); // "1"
  });

  test('an undefined marker is left completely alone', () => {
    const d = decosFor('Use [^0-9] to strip non-digits.', 0);
    expect(d).toEqual([]);
  });

  test('a marker reveals raw when the cursor touches it', () => {
    const d = decosFor(DOC, 3); // inside [^1]
    expect(hasHidden(d, 1, 3)).toBe(false);
    expect(hasMark(d, 'cm-lp-footnote-ref', 3, 4)).toBe(false);
  });

  test("a sibling marker on the same line keeps rendering", () => {
    // Per-node reveal: touching one marker must not un-render the other.
    const doc = 'A[^1] B[^2].\n\n[^1]: one\n\n[^2]: two';
    const d = decosFor(doc, 3); // inside [^1]
    expect(hasMark(d, 'cm-lp-footnote-ref', 3, 4)).toBe(false);  // touched → raw
    expect(hasMark(d, 'cm-lp-footnote-ref', 9, 10)).toBe(true);  // sibling → rendered
  });
});

describe('definitions', () => {
  test('the marker collapses and the line is tinted', () => {
    const d = decosFor(DOC, 0);
    expect(hasHidden(d, 13, 15)).toBe(true);                       // "[^"
    expect(hasHidden(d, 16, 18)).toBe(true);                       // "]:"
    expect(hasMark(d, 'cm-lp-footnote-label', 15, 16)).toBe(true); // "1"
    expect(hasMark(d, 'cm-lp-footnote-def', 13, 13)).toBe(true);   // line tint
  });

  test('the line tint survives reveal — no layout shift on cursor enter', () => {
    const d = decosFor(DOC, 20); // cursor on the definition line
    expect(hasMark(d, 'cm-lp-footnote-def', 13, 13)).toBe(true);
    expect(hasHidden(d, 13, 15)).toBe(false); // but the marker shows raw
  });
});

describe('footnoteIndex', () => {
  test('separates what is defined from what is merely referenced', () => {
    const { definitions, firstRefs } = footnoteIndex(stateFor('x[^a] y[^b]\n\n[^a]: A'));
    expect([...definitions.keys()]).toEqual(['a']);
    expect([...firstRefs.keys()]).toEqual(['a', 'b']);
  });

  test('a label defined twice keeps the FIRST, like the build does', () => {
    const { definitions } = footnoteIndex(stateFor('x[^a]\n\n[^a]: first\n\n[^a]: second'));
    expect(definitions.get('a')?.from).toBe(7);
  });
});

describe('in a live EditorView', () => {
  test('mounts and renders the raised label, brackets gone', () => {
    const view = new EditorView({
      state: EditorState.create({
        doc: DOC,
        selection: { anchor: 0 },
        extensions: [markdown({ extensions: [footnoteConfig] }), footnoteExtension()],
      }),
    });
    try {
      // jsdom lays out nothing, so assert on the DOM the plugin produced rather
      // than on geometry: the label is there, wrapped, and the `[^` is not.
      const el = view.dom.querySelector('.cm-lp-footnote-ref');
      expect(el?.textContent).toBe('1');
      expect(view.dom.textContent).not.toContain('[^');
    } finally {
      view.destroy();
    }
  });

  test('the effect-only source-mode flip clears the widgets without a caret move', () => {
    const view = new EditorView({
      state: EditorState.create({
        doc: DOC,
        selection: { anchor: 0 },
        extensions: [sourceModeField, markdown({ extensions: [footnoteConfig] }), footnoteExtension()],
      }),
    });
    try {
      expect(view.dom.querySelector('.cm-lp-footnote-ref')).not.toBeNull();
      // The toggle changes neither doc nor selection — the plugin's update
      // gate must rebuild on the mode flip alone (revealInputsChangedIn).
      view.dispatch({ effects: setSourceModeEffect.of(true) });
      expect(view.dom.querySelector('.cm-lp-footnote-ref')).toBeNull();
      expect(view.dom.textContent).toContain('[^');
    } finally {
      view.destroy();
    }
  });
});

describe('footnoteJumpTarget', () => {
  // Two-way, like the built page: a marker goes down to the note, and the
  // note's `[^1]:` chrome goes back up to the first reference (`↩`).
  const DOC2 = 'a[^1] b[^1]\n\n[^1]: the note';

  test('a marker jumps to its definition', () => {
    expect(footnoteJumpTarget(stateFor(DOC2), 3)?.from).toBe(13);
  });

  test('a second marker with the same label jumps to the same definition', () => {
    expect(footnoteJumpTarget(stateFor(DOC2), 9)?.from).toBe(13);
  });

  test("a definition's chrome jumps back to the FIRST reference", () => {
    expect(footnoteJumpTarget(stateFor(DOC2), 15)?.from).toBe(1);
  });

  test('the note body is not a jump target — Mod-Enter still edits there', () => {
    expect(footnoteJumpTarget(stateFor(DOC2), 24)).toBeNull();
  });

  test('an undefined marker refuses rather than jumping somewhere plausible', () => {
    expect(footnoteJumpTarget(stateFor('a[^zz] b\n\n[^1]: note'), 3)).toBeNull();
  });

  test('a definition nobody references has nowhere to go back to', () => {
    expect(footnoteJumpTarget(stateFor('plain\n\n[^1]: note'), 9)).toBeNull();
  });

  test('a multi-line note still refuses from its body', () => {
    const doc = 'a[^1]\n\n[^1]: first\n\n    second para';
    expect(footnoteJumpTarget(stateFor(doc), doc.indexOf('second') + 3)).toBeNull();
    expect(footnoteJumpTarget(stateFor(doc), 9)?.from).toBe(1); // chrome still jumps
  });

  test('ordinary prose is not a footnote', () => {
    expect(footnoteJumpTarget(stateFor('just words here'), 5)).toBeNull();
  });
});

describe('the followable affordance', () => {
  // `cm-link-clickable` is the editor's ONE followable-cursor class (links and
  // embed cards wear it too). What matters here is WHERE it is emitted from:
  // outside the reveal gate, so it survives the construct showing its source.
  const MARKER = 'a[^1]\n\n[^1]: note';   // marker 1..5, definition 7.., chrome ends 12

  test('a marker offers the follow, over the whole [^1]', () => {
    expect(hasMark(decosFor(MARKER, 0), 'cm-link-clickable', 1, 5)).toBe(true);
  });

  test('a referenced definition offers the back-jump, over its [^1]: chrome', () => {
    expect(hasMark(decosFor(MARKER, 0), 'cm-link-clickable', 7, 12)).toBe(true);
  });

  test('an unreferenced definition does not — the pointer never promises a refusal', () => {
    const d = decosFor('plain\n\n[^1]: note', 0);
    expect(hasMark(d, 'cm-lp-footnote-label', 9, 10)).toBe(true);
    expect(hasMark(d, 'cm-link-clickable', 7, 12)).toBe(false);
  });

  // The regression. Both constructs still FOLLOW while showing raw source, so
  // the hand has to still be there; deriving it from the render meant the
  // marker went bare the moment the caret revealed it.
  test('the marker keeps it while revealed', () => {
    const d = decosFor(MARKER, 3);                     // caret inside `[^1]`
    expect(hasMark(d, 'cm-lp-footnote-ref', 3, 4)).toBe(false);  // revealed: raw
    expect(hasMark(d, 'cm-link-clickable', 1, 5)).toBe(true);
  });

  test('the definition keeps it while revealed', () => {
    const d = decosFor(MARKER, 9);                     // caret on the chrome line
    expect(hasMark(d, 'cm-lp-footnote-label', 9, 10)).toBe(false); // revealed: raw
    expect(hasMark(d, 'cm-link-clickable', 7, 12)).toBe(true);
  });
});

describe('a note that spans lines', () => {
  const MULTI = 'a[^1]\n\n[^1]: first\n\n    second para\n';

  test('every line the note spans carries the tint, not just the first', () => {
    const d = decosFor(MULTI, 0);
    const state = stateFor(MULTI, 0);
    for (const n of [3, 4, 5]) {
      const at = state.doc.line(n).from;
      expect(hasMark(d, 'cm-lp-footnote-def', at, at)).toBe(true);
    }
  });

  test('the tint stops where the note stops', () => {
    const doc = '[^1]: note\n\nplain para\n';
    const state = stateFor(doc, 0);
    const at = state.doc.line(3).from;
    expect(hasMark(decosFor(doc, 0), 'cm-lp-footnote-def', at, at)).toBe(false);
  });

  test('a caret in the note body does not un-render the marker three lines up', () => {
    // Per-line reveal is scoped to the CHROME's line. Revealing `[^1]:` from
    // the body would be a jump the author never asked for.
    const d = decosFor(MULTI, MULTI.indexOf('second para') + 3);
    expect(hasHidden(d, 7, 9)).toBe(true); // "[^" still collapsed
  });

  test('a caret on the chrome line still reveals it', () => {
    const d = decosFor(MULTI, 9);
    expect(hasHidden(d, 7, 9)).toBe(false);
  });
});
