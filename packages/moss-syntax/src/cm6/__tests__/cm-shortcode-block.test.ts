import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import type { DecorationSet } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { shortcodeBlockConfig } from '../../shortcode.js';
import {
  collectShortcodeBlocks, shortcodeBlockExtension,
  flattenBlocks, isCellDividerLine, dividesCellsAt,
  isLegacyDividerLine, legacyDividesCellsAt, topLevelLegacyDividerCount,
  shortcodeBodyRanges,
} from '../cm-shortcode-block.js';

/**
 * Build a state whose syntax tree is COMPLETE.
 *
 * `EditorState.create` parses only within a 20ms budget capped at the first
 * 3000 characters, and with no `EditorView` attached nothing ever resumes it —
 * so `syntaxTree(state)` can return a partial tree. Every assertion here reads
 * that tree, so a loaded machine gets through less of the document in the same
 * budget and sees fewer blocks than an idle one. `pairs two top-level blocks`
 * failed intermittently for exactly this reason, always passing when the file
 * ran alone.
 *
 * `ensureSyntaxTree` alone does NOT fix it. It advances the language field's
 * mutable `context` and returns `context.tree`, but `syntaxTree(state)` reads
 * `field.tree` — a snapshot taken when the `LanguageState` was constructed.
 * Only `LanguageState.apply` republishes the context's tree onto the field, so
 * the advanced parse stays invisible until some transaction runs. Measured: a
 * 9261-char doc reports a 3012-char tree after `ensureSyntaxTree`, and the full
 * 9261 after one no-op `state.update({})`.
 *
 * So: advance, republish, and re-check, because `apply` itself re-parses under
 * a fresh 20ms budget. The loop is what makes this load-independent; it throws
 * rather than silently handing back a short tree and failing as a confusing
 * count mismatch somewhere downstream.
 */
function fullyParsed(state: EditorState): EditorState {
  for (let i = 0; i < 20; i++) {
    ensureSyntaxTree(state, state.doc.length, 1e9);
    if (syntaxTree(state).length >= state.doc.length) return state;
    state = state.update({}).state;
  }
  throw new Error('syntax tree never reached the end of the document');
}

function stateFor(doc: string, cursor = 0): EditorState {
  return fullyParsed(EditorState.create({
    doc, selection: { anchor: cursor },
    extensions: [markdown({ extensions: [shortcodeBlockConfig] })],
  }));
}

/** Collect classes, replace (hide) count, and token-mark spans. */
function inspectDecorations(state: EditorState): {
  lineClasses: string[];
  replaces: number;
  marks: { cls: string; from: number; to: number }[];
} {
  const lineClasses: string[] = [];
  const marks: { cls: string; from: number; to: number }[] = [];
  let replaces = 0;
  for (const provided of state.facet(EditorView.decorations)) {
    if (typeof provided === 'function') continue;
    const it = (provided as DecorationSet).iter();
    while (it.value) {
      const cls = (it.value.spec as { class?: string }).class;
      // A REPLACE decoration spans from < to with no class; line decorations
      // and widgets are point decorations; a token MARK spans with a class.
      if (it.from < it.to) {
        if (cls) marks.push({ cls, from: it.from, to: it.to });
        else replaces++;
      } else if (cls) {
        lineClasses.push(cls);
      }
      it.next();
    }
  }
  return { lineClasses, replaces, marks };
}

function stateAt(doc: string, cursor: number): EditorState {
  return fullyParsed(EditorState.create({
    doc, selection: { anchor: cursor },
    extensions: [markdown({ extensions: [shortcodeBlockConfig] }), shortcodeBlockExtension()],
  }));
}

describe('fences are always hung and token-highlighted, regardless of caret', () => {
  test('open fence and closing fence carry cm-hang, regardless of caret position', () => {
    const doc = 'above\n\n:::hero {image=a.jpg}\nbody\n:::\n';
    const above = inspectDecorations(stateAt(doc, 0));       // caret above the block
    const inside = inspectDecorations(stateAt(doc, doc.indexOf('body')));
    for (const { marks, lineClasses } of [above, inside]) {
      expect(marks.some((m) => m.cls.includes('cm-hang') && m.from === doc.indexOf(':::') && m.to === doc.indexOf(':::') + 3)).toBe(true);
      expect(lineClasses.some((c) => c.includes('cm-hung'))).toBe(true);
      expect(marks.some((m) => m.cls === 'cm-sc-name')).toBe(true); // "hero" still visible, not hung
    }
  });

  test('a `+++` cell divider carries cm-hang and is never replaced', () => {
    const doc = ':::grid 2\nleft\n+++\nright\n:::\n';
    const { marks, replaces } = inspectDecorations(stateAt(doc, doc.indexOf('left')));
    const plus = doc.indexOf('+++');
    expect(marks.some((m) => m.cls.includes('cm-hang') && m.from === plus && m.to === plus + 3)).toBe(true);
    expect(replaces).toBe(0);
  });

  test('no MicroTagWidget is ever built — the open fence is never replaced', () => {
    const doc = 'above\n\n:::hero {image=a.jpg}\nbody\n:::\n';
    expect(inspectDecorations(stateAt(doc, 0)).replaces).toBe(0);
  });
});

describe('collectShortcodeBlocks (marker pairing)', () => {
  test('pairs two top-level blocks with names/attrs', () => {
    const doc = ':::grid {cols=3}\ncell\n:::\n\ntext\n\n:::hero {image=h.png}\nx\n:::\n';
    const blocks = collectShortcodeBlocks(stateFor(doc));
    expect(blocks.length).toBe(2);
    expect(blocks[0].name).toBe('grid');
    expect(blocks[0].attrs).toContain('cols=3');
    expect(blocks[1].name).toBe('hero');
  });

  test('nested ::::buttons inside :::grid → one top-level block (the grid)', () => {
    const doc = ':::grid\n::::buttons\n[a](#)\n::::\ncell\n:::\n';
    const blocks = collectShortcodeBlocks(stateFor(doc));
    expect(blocks.length).toBe(1);
    expect(blocks[0].name).toBe('grid');
    // block extent spans through the final ::: close
    expect(blocks[0].to).toBe(doc.lastIndexOf(':::') + 3);
  });

  test('nested ::::buttons is attached as a child of the grid (not lost)', () => {
    const doc = ':::grid\n::::buttons\n[a](#)\n::::\ncell\n:::\n';
    const blocks = collectShortcodeBlocks(stateFor(doc));
    expect(blocks[0].children.map((c) => c.name)).toEqual(['buttons']);
    expect(blocks[0].children[0].arity).toBe(4);
    // flatten exposes both for the decoration layer, document order
    expect(flattenBlocks(blocks).map((b) => b.name)).toEqual(['grid', 'buttons']);
  });

  test('unmatched close fence (no open) is ignored — no crash, no block', () => {
    const doc = 'text\n:::\nmore\n';
    const blocks = collectShortcodeBlocks(stateFor(doc));
    expect(blocks.length).toBe(0);
  });

  test('unclosed open emits no block (no close to pair)', () => {
    const doc = ':::grid\nbody never closed\n';
    const blocks = collectShortcodeBlocks(stateFor(doc));
    expect(blocks.length).toBe(0);
  });

  test('block at EOF without trailing newline still pairs', () => {
    const doc = ':::grid\ncell\n:::'; // no trailing \n
    const blocks = collectShortcodeBlocks(stateFor(doc));
    expect(blocks.length).toBe(1);
    expect(blocks[0].name).toBe('grid');
  });

  test('nameless `:::{.class}` (Pure-CSS region) pairs with an empty name', () => {
    const doc = ':::{.tagline}\nx\n:::\n';
    const blocks = collectShortcodeBlocks(stateFor(doc));
    expect(blocks.length).toBe(1);
    expect(blocks[0].name).toBe('');
    expect(blocks[0].attrs).toBe('{.tagline}');
  });
});


describe('cell divider (+++) recognition', () => {
  test('isCellDividerLine: only a bare `+++` line', () => {
    expect(isCellDividerLine('+++')).toBe(true);
    expect(isCellDividerLine('  +++  ')).toBe(true);
    expect(isCellDividerLine('++')).toBe(false);
    expect(isCellDividerLine('+++ text')).toBe(false);
    expect(isCellDividerLine('---')).toBe(false); // the legacy form has its own predicate
  });

  test('dividesCellsAt: `+++` in a grid body IS a divider', () => {
    const doc = ':::grid 2\nleft\n+++\nright\n:::\n';
    const subtree = flattenBlocks(collectShortcodeBlocks(stateFor(doc)));
    const plus = doc.indexOf('+++');
    expect(dividesCellsAt(plus, subtree)).toBe(true);
  });

  test('dividesCellsAt: `+++` in a buttons body IS a divider', () => {
    const doc = ':::buttons\n[a](#)\n+++\n[b](#)\n:::\n';
    const subtree = flattenBlocks(collectShortcodeBlocks(stateFor(doc)));
    expect(dividesCellsAt(doc.indexOf('+++'), subtree)).toBe(true);
  });

  test('dividesCellsAt: `+++` in a non-cell shortcode (hero) is NOT a divider', () => {
    const doc = ':::hero\n+++\n:::\n';
    const subtree = flattenBlocks(collectShortcodeBlocks(stateFor(doc)));
    expect(dividesCellsAt(doc.indexOf('+++'), subtree)).toBe(false);
  });

  test('dividesCellsAt: `+++` at grid level (after a nested buttons) divides the GRID', () => {
    // The homepage shape: buttons nested in cell 1, `+++` separates the grid cells.
    const doc = ':::grid 2\n::::buttons\n[a](#)\n::::\n+++\nright\n:::\n';
    const subtree = flattenBlocks(collectShortcodeBlocks(stateFor(doc)));
    expect(dividesCellsAt(doc.indexOf('+++'), subtree)).toBe(true);
  });
});

describe('nested fences + dividers render the same at every caret position', () => {
  const doc = 'above\n\n:::grid 2\nleft\n::::buttons {inverted}\n[a](#)\n::::\n+++\nright\n:::\n';

  test('no fence or divider is ever replaced', () => {
    expect(inspectDecorations(stateAt(doc, 0)).replaces).toBe(0);
    expect(inspectDecorations(stateAt(doc, doc.indexOf('[a]'))).replaces).toBe(0);
  });

  test('both open fences (grid and nested buttons) are hung', () => {
    const { lineClasses } = inspectDecorations(stateAt(doc, 0));
    expect(lineClasses.filter((c) => c.includes('cm-sc-line-fence') && c.includes('cm-hung')).length).toBe(4); // 2 open + 2 close
  });

  test('the `+++` divider is hung, not drawn as a CSS rule over hidden text', () => {
    const { lineClasses } = inspectDecorations(stateAt(doc, 0));
    expect(lineClasses.some((c) => c.includes('cm-sc-line-divider') && c.includes('cm-hung'))).toBe(true);
  });
});

describe('fence token highlighting (caret-invariant)', () => {
  // Every fence renders as raw source, regardless of caret; raw is
  // token-highlighted, not plain: colons/braces muted (cm-sc-delim), the name
  // in keyword weight (cm-sc-name), attr keys secondary (cm-sc-attr-key),
  // values plain.
  // Token match, not exact-string match: a fence's delimiter mark now also
  // carries `cm-hang` (it hangs into the margin), so checking for the plain
  // `cm-sc-delim` class must not fail just because the mark carries a second
  // class alongside it.
  const has = (
    marks: { cls: string; from: number; to: number }[],
    cls: string, from: number, to: number,
  ) => marks.some((m) => m.cls.split(' ').includes(cls) && m.from === from && m.to === to);

  test('open fence `:::hero {image=a.jpg cols=2}`: delim/name/key spans', () => {
    const doc = ':::hero {image=a.jpg cols=2}\nbody\n:::\n';
    const { marks } = inspectDecorations(stateAt(doc, doc.indexOf('body')));
    expect(has(marks, 'cm-sc-delim', 0, 3)).toBe(true);                    // :::
    expect(has(marks, 'cm-sc-name', 3, 7)).toBe(true);                     // hero
    expect(has(marks, 'cm-sc-delim', 8, 9)).toBe(true);                    // {
    expect(has(marks, 'cm-sc-attr-key', 9, 14)).toBe(true);                // image
    expect(has(marks, 'cm-sc-attr-key', 21, 25)).toBe(true);               // cols
    expect(has(marks, 'cm-sc-delim', 27, 28)).toBe(true);                  // }
    // Values stay plain — no mark covers `a.jpg` or `2`.
    expect(marks.some((m) => m.from >= 15 && m.to <= 20)).toBe(false);     // a.jpg
    // Close fence colons muted.
    const close = doc.lastIndexOf(':::');
    expect(has(marks, 'cm-sc-delim', close, close + 3)).toBe(true);
  });

  test('nested fences: the inner :::: gets marks too', () => {
    const doc = ':::grid 2\n::::buttons\n[a](#)\n::::\n:::\n';
    const { marks } = inspectDecorations(stateAt(doc, doc.indexOf('[a]')));
    const inner = doc.indexOf('::::buttons');
    expect(has(marks, 'cm-sc-delim', inner, inner + 4)).toBe(true);        // ::::
    expect(has(marks, 'cm-sc-name', inner + 4, inner + 11)).toBe(true);    // buttons
    const innerClose = doc.indexOf('\n::::\n') + 1;
    expect(has(marks, 'cm-sc-delim', innerClose, innerClose + 4)).toBe(true);
  });

  test('nameless `:::{.tagline}` block: colons and braces muted, no name mark', () => {
    const doc = ':::{.tagline}\nbody\n:::\n';
    const { marks } = inspectDecorations(stateAt(doc, doc.indexOf('body')));
    expect(has(marks, 'cm-sc-delim', 0, 3)).toBe(true);  // :::
    expect(has(marks, 'cm-sc-delim', 3, 4)).toBe(true);  // {
    expect(has(marks, 'cm-sc-delim', 12, 13)).toBe(true); // }
    expect(marks.some((m) => m.cls === 'cm-sc-name')).toBe(false);
  });

  test('fence marks are present with the caret above the block, same spans as inside it', () => {
    const doc = 'above\n\n:::hero {image=a.jpg}\nbody\n:::\n';
    const { marks } = inspectDecorations(stateAt(doc, 0));
    const open = doc.indexOf(':::');
    expect(marks.some((m) => m.cls.includes('cm-hang') && m.from === open && m.to === open + 3)).toBe(true);
  });
});

/** Count rendered `.cm-sc-legacy-hint` widgets (the "use +++" note). */
function legacyHints(state: EditorState): string[] {
  const out: string[] = [];
  for (const provided of state.facet(EditorView.decorations)) {
    if (typeof provided === 'function') continue;
    const it = (provided as DecorationSet).iter();
    while (it.value) {
      const w = (it.value.spec as { widget?: { toDOM(): HTMLElement } }).widget;
      if (w) {
        const dom = w.toDOM();
        if (dom.className === 'cm-sc-legacy-hint') out.push(dom.textContent ?? '');
      }
      it.next();
    }
  }
  return out;
}

describe('deprecated `---` cell divider', () => {
  test('isLegacyDividerLine matches only a bare `---`', () => {
    expect(isLegacyDividerLine('---')).toBe(true);
    expect(isLegacyDividerLine('  ---  ')).toBe(true);
    expect(isLegacyDividerLine('----')).toBe(false);
    expect(isLegacyDividerLine('--- text')).toBe(false);
    expect(isLegacyDividerLine('+++')).toBe(false);
  });

  test('legacyDividesCellsAt: `---` divides inside grid, not inside buttons', () => {
    // Mirrors the Rust rule: `split_grid_cells` accepts `---`, `split_cells`
    // (every other cell type) does not.
    const grid = ':::grid 2\nleft\n---\nright\n:::\n';
    expect(legacyDividesCellsAt(grid.indexOf('---'),
      flattenBlocks(collectShortcodeBlocks(stateFor(grid))))).toBe(true);

    const buttons = ':::buttons\n[a](#)\n---\n[b](#)\n:::\n';
    expect(legacyDividesCellsAt(buttons.indexOf('---'),
      flattenBlocks(collectShortcodeBlocks(stateFor(buttons))))).toBe(false);
  });

  test('resting: the `---` line is marked and annotated, and stays visible', () => {
    const doc = 'above\n\n:::grid 2\nleft\n---\nright\n:::\n';
    const state = stateAt(doc, 0);
    const { lineClasses, replaces } = inspectDecorations(state);

    expect(lineClasses.some((c) => c.includes('cm-sc-line-legacy-divider'))).toBe(true);
    // Nothing is replaced any more — the fences are hung, not hidden.
    expect(replaces).toBe(0);
    // The note names the replacement, not just the problem.
    expect(legacyHints(state)).toHaveLength(1);
    expect(legacyHints(state)[0]).toContain('+++');
  });

  test('active: the note follows the cursor into the block', () => {
    const doc = ':::grid 2\nleft\n---\nright\n:::\n';
    expect(legacyHints(stateAt(doc, doc.indexOf('left')))).toHaveLength(1);
  });

  test('a `---` inside a nested buttons block is not annotated', () => {
    // The negative case the Rust scanner also guards: `---` is ordinary content
    // outside a grid, so hinting there would be wrong in both states.
    const doc = 'above\n\n:::grid 2\n::::buttons\n[a](#)\n---\n[b](#)\n::::\n:::\n';
    expect(legacyHints(stateAt(doc, 0))).toHaveLength(0);
    expect(legacyHints(stateAt(doc, doc.indexOf('[a]')))).toHaveLength(0);
  });

  test('a canonical `+++` divider gets no note', () => {
    const doc = 'above\n\n:::grid 2\nleft\n+++\nright\n:::\n';
    expect(legacyHints(stateAt(doc, 0))).toHaveLength(0);
  });

  test('topLevelLegacyDividerCount counts what the Rust scan counts', () => {
    // Rust records dividers only at depth 1 of a top-level `:::grid`; the
    // dev divergence check compares against this, so the rules must match.
    const doc = ':::grid 3\na\n+++\nb\n---\nc\n:::\n\n:::buttons\n---\n:::\n';
    expect(topLevelLegacyDividerCount(stateFor(doc))).toBe(1);
  });
});

describe('shortcodeBodyRanges', () => {
  test('covers the body, and neither fence line', () => {
    const doc = 'before\n\n:::grid\ncell\n:::\n\nafter\n';
    const ranges = shortcodeBodyRanges(stateFor(doc));
    expect(ranges).toHaveLength(1);
    const inBody = (needle: string) => {
      const at = doc.indexOf(needle);
      return ranges.some((r) => at >= r.from && at < r.to);
    };
    expect(inBody('cell')).toBe(true);
    expect(inBody('before')).toBe(false);
    expect(inBody('after')).toBe(false);
    // The open fence's own text is chrome, not body.
    expect(inBody(':::grid')).toBe(false);
  });

  test('a nested block yields one covering range, not two that overlap', () => {
    const doc = ':::grid\na\n::::buttons\nb\n::::\n:::\n';
    const ranges = shortcodeBodyRanges(stateFor(doc));
    for (let i = 1; i < ranges.length; i++) {
      expect(ranges[i].from).toBeGreaterThanOrEqual(ranges[i - 1].to);
    }
    const at = doc.indexOf('b');
    expect(ranges.some((r) => at >= r.from && at < r.to)).toBe(true);
  });

  test('a cell divider line is inside the body', () => {
    const doc = ':::grid\na\n+++\nb\n:::\n';
    const ranges = shortcodeBodyRanges(stateFor(doc));
    const at = doc.indexOf('+++');
    expect(ranges.some((r) => at >= r.from && at < r.to)).toBe(true);
  });

  test('no fence, no ranges', () => {
    expect(shortcodeBodyRanges(stateFor('just a paragraph\n'))).toEqual([]);
  });
});
