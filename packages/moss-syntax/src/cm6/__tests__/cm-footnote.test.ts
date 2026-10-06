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
import { footnoteIndex, footnoteJumpTarget, footnoteDecorations, footnoteExtension, footnoteViewPluginForTest } from '../cm-footnote.js';

function stateFor(doc: string, cursor?: number): EditorState {
  return EditorState.create({
    doc,
    selection: { anchor: cursor ?? doc.length },
    extensions: [markdown({ extensions: [footnoteConfig] })],
  });
}

interface Flat {
  from: number;
  to: number;
  cls?: string;
  hidden: boolean;
  widget?: boolean;
  widgetClass?: string;
  widgetText?: string;
}

function decosFor(doc: string, cursor?: number): Flat[] {
  const state = stateFor(doc, cursor);
  return footnoteDecorations(state).map((d) => {
    const spec = d.deco.spec as { class?: string; widget?: { toDOM(): HTMLElement } };
    const dom = spec.widget?.toDOM();
    return {
      from: d.from,
      to: d.to,
      cls: spec.class,
      hidden: spec.class == null && spec.widget == null,
      widget: spec.widget != null,
      widgetClass: dom?.className,
      widgetText: dom?.textContent ?? undefined,
    };
  });
}

// Class containment, not equality: a rendered label carries a second class
// when it can jump back to a reference, and that is a separate assertion.
const hasMark = (d: Flat[], cls: string, from: number, to: number) =>
  d.some((x) => x.cls?.split(' ').includes(cls) && x.from === from && x.to === to);

// `A[^1] text.` + blank + `[^1]: the note` — offsets are ASCII so they read plainly.
const DOC = 'A[^1] text.\n\n[^1]: the note';

describe('markers', () => {
  test('a defined marker is a chip carrying its label', () => {
    const d = decosFor(DOC, 0);
    const chip = d.find((x) => x.from === 1 && x.to === 5); // the whole "[^1]"
    expect(chip?.widgetClass).toContain('cm-footnote-chip');
    expect(chip?.widgetText).toBe('1');
  });

  test('an undefined marker is left completely alone', () => {
    const d = decosFor('Use [^0-9] to strip non-digits.', 0);
    expect(d).toEqual([]);
  });

  test('a sibling marker on the same line keeps rendering as its own chip', () => {
    // Per-node reveal is gone, but this still checks that touching one
    // marker doesn't disturb its neighbor's separate widget.
    const doc = 'A[^1] B[^2].\n\n[^1]: one\n\n[^2]: two';
    const d = decosFor(doc, 3); // inside [^1]
    expect(d.find((x) => x.from === 1 && x.to === 5)?.widgetText).toBe('1');
    expect(d.find((x) => x.from === 7 && x.to === 11)?.widgetText).toBe('2');
  });

  test('a defined marker is a chip widget regardless of where the caret is', () => {
    // caret INSIDE [^1] — today this reveals it raw; the new rule never does.
    const d = decosFor(DOC, 3);
    const chip = d.find((x) => x.from === 1 && x.to === 5); // the whole "[^1]"
    expect(chip?.widget).toBe(true);
    expect(chip?.widgetClass).toContain('cm-footnote-chip');
    expect(chip?.widgetText).toBe('1');
  });
});

describe('definitions', () => {
  test('the definition prefix hangs and the line is tinted', () => {
    const d = decosFor(DOC, 0);
    expect(hasMark(d, 'cm-hang', 13, 18)).toBe(true);            // "[^1]:" as one hung mark
    expect(hasMark(d, 'cm-lp-footnote-def', 13, 13)).toBe(true); // line tint
  });

  test('the definition prefix is hung regardless of caret position', () => {
    // caret ON the definition line — today this reveals the "[^1]:" raw;
    // the new rule hangs it into the margin unconditionally.
    const d = decosFor(DOC, 20);
    expect(hasMark(d, 'cm-hang', 13, 18)).toBe(true);          // "[^1]:" as one hung mark
    expect(hasMark(d, 'cm-lp-footnote-def', 13, 13)).toBe(true);
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
  test('mounts and renders the chip; the reference brackets are gone, the definition prefix is not', () => {
    const view = new EditorView({
      state: EditorState.create({
        doc: DOC,
        selection: { anchor: 0 },
        extensions: [markdown({ extensions: [footnoteConfig] }), footnoteExtension()],
      }),
    });
    try {
      // jsdom lays out nothing, so assert on the DOM the plugin produced
      // rather than on geometry. The reference is a STAND-IN — its `[^1]` is
      // gone from the DOM, replaced by the chip. The definition's `[^1]:` is
      // HUNG, not hidden — its real characters stay in the DOM (moved into
      // the margin by CSS alone, which jsdom cannot lay out), so it is not
      // a "brackets gone" case at all.
      const el = view.dom.querySelector('.cm-footnote-chip');
      expect(el?.textContent).toBe('1');
      expect(view.dom.textContent).toBe('A1 text.[^1]: the note');
    } finally {
      view.destroy();
    }
  });

  test('the chip itself carries the followable affordance, not just a coincident mark', () => {
    // A Decoration.mark spanning the same range as a Decoration.replace's
    // widget never reaches that widget's DOM — CodeMirror only applies a
    // mark's class to text it actually renders, and the replace hides that
    // text. So the class has to be on the widget's own element to ever
    // appear in the real DOM; this reads the rendered chip directly rather
    // than the pre-`Decoration.set` array `decosFor` reads.
    const view = new EditorView({
      state: EditorState.create({
        doc: DOC,
        selection: { anchor: 0 },
        extensions: [markdown({ extensions: [footnoteConfig] }), footnoteExtension()],
      }),
    });
    try {
      const chip = view.dom.querySelector('.cm-footnote-chip');
      expect(chip?.classList.contains('cm-link-clickable')).toBe(true);
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
  // embed cards wear it too). It is emitted unconditionally, never derived
  // from a caret-dependent render — alongside the hung chrome for a
  // definition (a real mark, below), and on the chip widget's own DOM
  // element for a marker (a mark over the same range a Decoration.replace
  // covers never reaches the DOM, so decosFor's pre-render array can only
  // assert this via widgetClass, not via hasMark).
  const MARKER = 'a[^1]\n\n[^1]: note';   // marker 1..5, definition 7.., chrome ends 12

  test('a marker offers the follow, over the whole [^1]', () => {
    const chip = decosFor(MARKER, 0).find((x) => x.from === 1 && x.to === 5);
    expect(chip?.widgetClass).toContain('cm-link-clickable');
  });

  test('a referenced definition offers the back-jump, over its [^1]: chrome', () => {
    expect(hasMark(decosFor(MARKER, 0), 'cm-link-clickable', 7, 12)).toBe(true);
  });

  test('an unreferenced definition does not — the pointer never promises a refusal', () => {
    const d = decosFor('plain\n\n[^1]: note', 0);
    expect(hasMark(d, 'cm-hang', 7, 12)).toBe(true); // the chrome still hangs
    expect(hasMark(d, 'cm-link-clickable', 7, 12)).toBe(false); // but nowhere to go back to
  });

  // The regression this whole describe guards: both constructs FOLLOW no
  // matter where the caret is, because the affordance is not derived from
  // whatever is rendered at the caret.
  test('the marker keeps its follow affordance regardless of the caret', () => {
    const d = decosFor(MARKER, 3); // caret inside `[^1]`
    const chip = d.find((x) => x.from === 1 && x.to === 5);
    expect(chip?.widgetClass).toContain('cm-footnote-chip');
    expect(chip?.widgetClass).toContain('cm-link-clickable');
  });

  test('the definition keeps its back-jump affordance regardless of the caret', () => {
    const d = decosFor(MARKER, 9); // caret on the chrome line
    expect(hasMark(d, 'cm-hang', 7, 12)).toBe(true);
    expect(hasMark(d, 'cm-link-clickable', 7, 12)).toBe(true);
  });
});

describe('composition', () => {
  test('no chip is added or removed on the composing line while composing', () => {
    // A defined label already in the doc, so typing a matching `[^1]`
    // reference while composing would normally add a chip widget plus its
    // follow mark — a real, visible decoration-count change, not just a
    // pure function that would see the fresh doc regardless of composing.
    const view = new EditorView({
      state: EditorState.create({
        doc: 'plain\n\n[^1]: note',
        extensions: [markdown({ extensions: [footnoteConfig] }), footnoteExtension()],
      }),
    });
    // footnoteDecorations(view.state) itself is a pure function with no
    // composing awareness and DOES see the fresh doc — the claim under test
    // is about the LIVE ViewPlugin's own decoration set, read through the
    // test-only seam below, which must still be the pre-edit, mapped-
    // through-changes set.
    const plugin = view.plugin(footnoteViewPluginForTest)!;
    const before = plugin.decorations.size;
    // `composing` is a read-only getter derived from `inputState.composing`
    // (> 0 while an IME composition is in progress); set the field the
    // getter reads rather than the getter itself.
    (view as unknown as { inputState: { composing: number } }).inputState.composing = 1;
    view.dispatch({ changes: { from: 0, to: 0, insert: 'x[^1] ' } }); // would normally add a chip
    expect(plugin.decorations.size).toBe(before); // unchanged — never rebuilt mid-composition
    view.destroy();
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

  test('a caret in the note body does not change the chrome three lines up', () => {
    // There is no per-line reveal any more, so a caret three lines into the
    // note body has no effect on the chrome's rendering at all.
    const d = decosFor(MULTI, MULTI.indexOf('second para') + 3);
    expect(hasMark(d, 'cm-hang', 7, 12)).toBe(true); // "[^1]:" still hung
  });

  test('a caret on the chrome line changes nothing about how it renders', () => {
    const d = decosFor(MULTI, 9);
    expect(hasMark(d, 'cm-hang', 7, 12)).toBe(true);
  });
});
