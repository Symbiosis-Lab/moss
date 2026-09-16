import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { parseMarks, criticmarkupExtension } from '../cm-criticmarkup.js';

// ── Helpers ────────────────────────────────────────────────────────────

/**
 * Markdown state — `parseMarks` masks code regions from the SYNTAX TREE
 * (FencedCode / indented CodeBlock / InlineCode nodes), so the markdown
 * language must be present.
 */
function mdState(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [markdown()] });
}

/** Build a headless EditorView with markdown + the criticmarkup extension. */
function makeView(doc: string): EditorView {
  return new EditorView({
    state: EditorState.create({ doc, extensions: [markdown(), criticmarkupExtension()] }),
  });
}

/** Read the live DecorationSet the plugin currently exposes. */
function getDecoSet(view: EditorView) {
  // Walk the plugin field and pick the criticmarkup instance — identified by
  // its `_fullBuilds` test counter (the markdown language also installs
  // ViewPlugins, so `decorations` alone is not unique).
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fields = (view as any).plugins as Array<{ value: { decorations?: unknown; _fullBuilds?: number } }>;
  for (const f of fields) {
    if (f.value && typeof f.value === 'object' && '_fullBuilds' in f.value) {
      return f.value as { decorations: import('@codemirror/view').DecorationSet; _fullBuilds: number };
    }
  }
  throw new Error('criticmarkup ViewPlugin not found on view');
}

/** Flatten the decoration set into [from, to, class] tuples for comparison. */
function decoRanges(view: EditorView): Array<{ from: number; to: number; cls: string }> {
  const set = getDecoSet(view).decorations;
  const out: Array<{ from: number; to: number; cls: string }> = [];
  const cursor = set.iter();
  while (cursor.value !== null) {
    const spec = cursor.value.spec as { class?: string };
    out.push({ from: cursor.from, to: cursor.to, cls: spec.class ?? '' });
    cursor.next();
  }
  return out;
}

describe('parseMarks — token recognition', () => {
  test('addition {++...++}', () => {
    const marks = parseMarks(mdState('before {++ added ++} after'));
    expect(marks).toEqual([{ from: 7, to: 20, type: 'addition' }]);
  });

  test('deletion {--...--}', () => {
    const marks = parseMarks(mdState('before {-- removed --} after'));
    expect(marks).toEqual([{ from: 7, to: 22, type: 'deletion' }]);
  });

  test('highlight {==...==}', () => {
    const marks = parseMarks(mdState('before {== hi ==} after'));
    expect(marks).toEqual([{ from: 7, to: 17, type: 'highlight' }]);
  });

  test('comment {>>...<<}', () => {
    const marks = parseMarks(mdState('before {>> note <<} after'));
    expect(marks).toEqual([{ from: 7, to: 19, type: 'comment' }]);
  });

  test('substitution {~~old~>new~~} records the ~> split position', () => {
    const text = 'see {~~old~>new~~} here';
    const marks = parseMarks(mdState(text));
    expect(marks).toHaveLength(1);
    expect(marks[0].type).toBe('substitution');
    expect(marks[0].from).toBe(4);
    expect(marks[0].to).toBe(18);
    expect(marks[0].mid).toBe(10); // position of ~>
  });
});

describe('parseMarks — multiple marks on one line', () => {
  test('returns all in document order', () => {
    const marks = parseMarks(mdState('{==a==} then {++b++} end'));
    expect(marks.map((m) => m.type)).toEqual(['highlight', 'addition']);
  });

  test('adjacent highlight + comment (the annotation pattern)', () => {
    const marks = parseMarks(mdState('{==thing==}{>> why <<}'));
    expect(marks.map((m) => m.type)).toEqual(['highlight', 'comment']);
    expect(marks[0].from).toBe(0);
    expect(marks[0].to).toBe(11);
    expect(marks[1].from).toBe(11);
    expect(marks[1].to).toBe(22);
  });
});

describe('parseMarks — nested with other markdown', () => {
  test('wiki-link inside highlight: outer mark recognized, inner content preserved', () => {
    const marks = parseMarks(mdState('{==[[media|media files]]==}'));
    expect(marks).toEqual([{ from: 0, to: 27, type: 'highlight' }]);
  });
});

describe('parseMarks — code block isolation', () => {
  test('does not match inside fenced code block', () => {
    expect(parseMarks(mdState('```\n{==fake==}\n```'))).toEqual([]);
  });

  test('does not match inside tilde fenced block', () => {
    expect(parseMarks(mdState('~~~\n{==fake==}\n~~~'))).toEqual([]);
  });

  test('does not match inside inline code', () => {
    expect(parseMarks(mdState('example: `{==fake==}` end'))).toEqual([]);
  });

  test('does not match inside an INDENTED code block', () => {
    // The old hand-rolled masking only knew fences; the tree also masks
    // 4-space-indented CodeBlock regions, agreeing with what the editor
    // highlights as code.
    expect(parseMarks(mdState('para\n\n    {==fake==}\n\nafter'))).toEqual([]);
  });

  test('real mark outside code block still parsed when code block precedes it', () => {
    const marks = parseMarks(mdState('```\ncode\n```\n\n{==real==}'));
    expect(marks).toHaveLength(1);
    expect(marks[0].type).toBe('highlight');
  });
});

describe('parseMarks — malformed input', () => {
  test('unclosed addition does not create a mark', () => {
    expect(parseMarks(mdState('before {++ oops'))).toEqual([]);
  });

  test('unclosed substitution does not create a mark', () => {
    expect(parseMarks(mdState('before {~~old~> oops'))).toEqual([]);
  });

  test('substitution without ~> is not a mark', () => {
    expect(parseMarks(mdState('{~~ no arrow ~~}'))).toEqual([]);
  });

  test('empty input returns empty array', () => {
    expect(parseMarks(mdState(''))).toEqual([]);
  });

  test('plain text with no markup returns empty array', () => {
    expect(parseMarks(mdState('just some prose'))).toEqual([]);
  });
});

describe('parseMarks — nested marks drop the inner', () => {
  test('deletion inside highlight: outer kept, inner dropped', () => {
    const marks = parseMarks(mdState('{==hi {-- bye --} end==}'));
    expect(marks).toHaveLength(1);
    expect(marks[0].type).toBe('highlight');
  });
});

describe('parseMarks — multi-line', () => {
  test('mark spanning two lines within a paragraph is recognized', () => {
    const text = '{==first\nsecond==}';
    const marks = parseMarks(mdState(text));
    expect(marks).toHaveLength(1);
    expect(marks[0].type).toBe('highlight');
    expect(marks[0].from).toBe(0);
    expect(marks[0].to).toBe(text.length);
  });

  test('mark spanning a BLANK LINE (cross-paragraph) is recognized', () => {
    // Build parity: the Rust pipeline's accept_criticmarkup uses (?s) regexes
    // that cross paragraph boundaries — the editor must mark what the build
    // will accept. This is WHY criticmarkup is not a Lezer inline grammar
    // (inline contexts are paragraph-bounded).
    const text = '{++first para\n\nsecond para++}';
    const marks = parseMarks(mdState(text));
    expect(marks).toHaveLength(1);
    expect(marks[0].type).toBe('addition');
    expect(marks[0].from).toBe(0);
    expect(marks[0].to).toBe(text.length);
  });
});

// ── PATTERN 7 — incremental decoration update via RangeSet.map ────────
//
// These tests pin the contract refactored in PR-3 (see
// docs/archive/2026-05-22-editor-state-architecture.md): typing plain prose must
// remap existing decorations through the change set instead of rebuilding the
// whole DecorationSet by re-parsing the document.

describe('criticmarkupExtension — incremental update', () => {
  test('decoration positions update correctly after inserting text before existing markup', () => {
    const view = makeView('hello {++added++} world');
    const before = decoRanges(view);
    expect(before).toEqual([
      { from: 6, to: 9, cls: 'cm-criticmarkup-delim' },
      { from: 9, to: 14, cls: 'cm-criticmarkup-addition' },
      { from: 14, to: 17, cls: 'cm-criticmarkup-delim' },
    ]);

    // Insert 5 plain chars (no trigger) at position 0 — all ranges shift right by 5.
    view.dispatch({ changes: { from: 0, to: 0, insert: 'XXXXX' } });
    const after = decoRanges(view);
    expect(after).toEqual([
      { from: 11, to: 14, cls: 'cm-criticmarkup-delim' },
      { from: 14, to: 19, cls: 'cm-criticmarkup-addition' },
      { from: 19, to: 22, cls: 'cm-criticmarkup-delim' },
    ]);
    view.destroy();
  });

  test('decoration positions update correctly after inserting text inside an existing mark', () => {
    const view = makeView('{++foo++}');
    const before = decoRanges(view);
    expect(before).toEqual([
      { from: 0, to: 3, cls: 'cm-criticmarkup-delim' },
      { from: 3, to: 6, cls: 'cm-criticmarkup-addition' },
      { from: 6, to: 9, cls: 'cm-criticmarkup-delim' },
    ]);

    // Insert plain text inside the payload — payload grows, delimiters shift.
    view.dispatch({ changes: { from: 5, to: 5, insert: 'BAR' } });
    expect(view.state.doc.toString()).toBe('{++foBARo++}');
    expect(decoRanges(view)).toEqual([
      { from: 0, to: 3, cls: 'cm-criticmarkup-delim' },
      { from: 3, to: 9, cls: 'cm-criticmarkup-addition' },
      { from: 9, to: 12, cls: 'cm-criticmarkup-delim' },
    ]);
    view.destroy();
  });

  test('substitution split positions remap through inserts in the old half', () => {
    const view = makeView('{~~old~>new~~}');
    const before = decoRanges(view);
    // Delimiters + inner-old + payload wash + inner-new (equal-`from` ranges
    // iterate shorter-first, so old sorts before the wash at position 3).
    expect(before.map((d) => d.cls)).toEqual([
      'cm-criticmarkup-delim',
      'cm-criticmarkup-substitution-old',
      'cm-criticmarkup-substitution',
      'cm-criticmarkup-substitution-new',
      'cm-criticmarkup-delim',
    ]);
    expect(before[1]).toEqual({ from: 3, to: 6, cls: 'cm-criticmarkup-substitution-old' });
    expect(before[2]).toEqual({ from: 3, to: 11, cls: 'cm-criticmarkup-substitution' });
    expect(before[3]).toEqual({ from: 8, to: 11, cls: 'cm-criticmarkup-substitution-new' });

    // Insert 2 plain chars inside the OLD half. Wash + old-inner both grow;
    // new-inner and the closing delimiter shift right by 2, keeping width.
    view.dispatch({ changes: { from: 5, to: 5, insert: 'ZZ' } });
    const after = decoRanges(view);
    expect(after[0]).toEqual({ from: 0, to: 3, cls: 'cm-criticmarkup-delim' });
    expect(after[1]).toEqual({ from: 3, to: 8, cls: 'cm-criticmarkup-substitution-old' });
    expect(after[2]).toEqual({ from: 3, to: 13, cls: 'cm-criticmarkup-substitution' });
    expect(after[3]).toEqual({ from: 10, to: 13, cls: 'cm-criticmarkup-substitution-new' });
    expect(after[4]).toEqual({ from: 13, to: 16, cls: 'cm-criticmarkup-delim' });
    view.destroy();
  });

  test('typing a trigger character forces a full re-parse', () => {
    const view = makeView('hello world');
    const plugin = getDecoSet(view);
    const buildsBefore = plugin._fullBuilds;

    // Insert plain text — fast path, no rebuild.
    view.dispatch({ changes: { from: 5, to: 5, insert: ' there' } });
    expect(getDecoSet(view)._fullBuilds).toBe(buildsBefore);

    // Insert a `{` — must rebuild because a mark *could* now open.
    view.dispatch({ changes: { from: 0, to: 0, insert: '{' } });
    expect(getDecoSet(view)._fullBuilds).toBe(buildsBefore + 1);
    view.destroy();
  });

  test('deleting a delimiter character forces a full re-parse', () => {
    const view = makeView('{++foo++}');
    const plugin = getDecoSet(view);
    const buildsBefore = plugin._fullBuilds;
    expect(decoRanges(view)).toHaveLength(3); // delim + payload + delim

    // Delete one `+` — the mark is now malformed and must disappear.
    view.dispatch({ changes: { from: 2, to: 3, insert: '' } });
    expect(getDecoSet(view)._fullBuilds).toBe(buildsBefore + 1);
    expect(decoRanges(view)).toEqual([]);
    view.destroy();
  });

  test('plain-prose keystrokes do not call the full-build code path', () => {
    // Document with many existing marks — full rebuild would be expensive.
    const doc = Array.from({ length: 40 }, (_, i) => `line ${i} {++note ${i}++}`).join('\n');
    const view = makeView(doc);
    const plugin = getDecoSet(view);
    const decoCountBefore = decoRanges(view).length;
    expect(decoCountBefore).toBe(120); // 40 marks × (delim + payload + delim)

    const buildsBefore = plugin._fullBuilds;
    // Simulate 50 plain-prose keystrokes (no trigger chars). Each one inserts
    // a single benign character at the very start of the document.
    for (let i = 0; i < 50; i++) {
      view.dispatch({ changes: { from: 0, to: 0, insert: 'x' } });
    }
    const buildsAfter = getDecoSet(view)._fullBuilds;

    // Pattern-7 contract: no full rebuilds on the incremental path.
    expect(buildsAfter - buildsBefore).toBe(0);
    // Decoration count must be preserved (positions remapped, not rebuilt).
    expect(decoRanges(view).length).toBe(decoCountBefore);
    // First decoration must have shifted by exactly 50 chars.
    const firstAfter = decoRanges(view)[0];
    // Doc originally starts with "line 0 {++note 0++}" — opening delim at 7..10.
    expect(firstAfter.from).toBe(7 + 50);
    expect(firstAfter.to).toBe(10 + 50);
    view.destroy();
  });

  test('semantic equivalence: incremental + rebuild paths agree after a sequence of edits', () => {
    const view = makeView('start {==hi==} mid {++yo++} end');

    // Mixed edits — plain insert, then trigger insert (forces rebuild), then plain delete.
    view.dispatch({ changes: { from: 0, to: 0, insert: 'AA ' } });
    view.dispatch({ changes: { from: view.state.doc.length, to: view.state.doc.length, insert: ' {--gone--}' } });
    view.dispatch({ changes: { from: 3, to: 6, insert: '' } });

    const finalText = view.state.doc.toString();
    const decosFromView = decoRanges(view).map(({ from, to, cls }) => ({ from, to, cls }));

    // Build a fresh view from the same final doc — the rebuild path. Both paths
    // must produce the same DecorationSet (positions, ordering, classes).
    const fresh = makeView(finalText);
    const decosFromScratch = decoRanges(fresh).map(({ from, to, cls }) => ({ from, to, cls }));

    expect(decosFromView).toEqual(decosFromScratch);
    view.destroy();
    fresh.destroy();
  });
});
