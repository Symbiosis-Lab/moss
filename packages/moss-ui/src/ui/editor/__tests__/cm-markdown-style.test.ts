import { describe, test, expect, vi } from 'vitest';
import { EditorSelection, EditorState } from '@codemirror/state';
import { cursorCharLeft, cursorCharRight, cursorLineBoundaryBackward, deleteCharBackward, selectCharLeft } from '@codemirror/commands';
import { EditorView, runScopeHandlers, type DecorationSet } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { syntaxHighlighting, syntaxTree } from '@codemirror/language';
import { GFM, Table } from '@lezer/markdown';
import { wikilinkConfig, mathConfig, footnoteConfig } from '@symbiosis-lab/moss-syntax';
import { mossHighlight } from '@symbiosis-lab/moss-syntax/cm6';
import { buildMarkdownStyleDecorations, markdownStyleExtension, markdownStylePlugin } from '../cm-markdown-style';
import { setLocale } from "../../i18n/index";

// Every construct the styling extension handles (headings, lists, blockquotes,
// fenced code, display/inline math, wikilinks, tasks) needs a grammar
// extension `markdown()`'s CommonMark-only default doesn't register — GFM
// (Table/TaskList/Strikethrough/Autolink) plus moss's own wikilink and math
// grammars. One shared helper keeps every test in this file describing the
// same parser the running editor actually uses (cm-editor.ts wires the same
// set via moss-syntax's wikilinkConfig/mathConfig + GFM).
function decosFor(doc: string): { from: number; to: number; cls?: string }[] {
  const state = EditorState.create({
    doc,
    extensions: [markdown({ extensions: [GFM, wikilinkConfig, mathConfig, footnoteConfig] })],
  });
  const set = buildMarkdownStyleDecorations(state, [{ from: 0, to: doc.length }]);
  const out: { from: number; to: number; cls?: string }[] = [];
  const cur = set.iter();
  while (cur.value) {
    out.push({ from: cur.from, to: cur.to, cls: (cur.value.spec as { class?: string }).class });
    cur.next();
  }
  return out;
}

describe('heading line class', () => {
  test('## Heading gets cm-lp-h2 on the line, no marker hidden', () => {
    const d = decosFor('## Heading');
    expect(d.some((x) => x.cls === 'cm-lp-h2' && x.from === 0 && x.to === 0)).toBe(true);
    // nothing REPLACEs the "## " — it hangs, it is never hidden
    expect(d.every((x) => x.cls !== undefined)).toBe(true);
  });

  test('## Heading hangs the "## " prefix, not hidden', () => {
    const d = decosFor('## Heading');
    const hang = d.find((x) => x.cls === 'cm-hang');
    expect(hang).toEqual({ from: 0, to: 3, cls: 'cm-hang' }); // "## "
    expect(d.some((x) => x.cls === 'cm-hung' && x.from === 0)).toBe(true);
  });

  test('empty heading "##" still hangs — nothing to guard once hanging never erases', () => {
    const d = decosFor('##');
    expect(d.some((x) => x.cls === 'cm-hang' && x.from === 0 && x.to === 2)).toBe(true);
  });
});

describe('blockquote', () => {
  test('blockquote: bar on every line, "> " hangs on every line', () => {
    const d = decosFor('> line one\n> line two');
    expect(d.filter((x) => x.cls === 'cm-lp-blockquote').length).toBe(2);
    const hangs = d.filter((x) => x.cls === 'cm-hang');
    expect(hangs).toContainEqual({ from: 0, to: 2, cls: 'cm-hang' }); // "> "
    expect(hangs).toContainEqual({ from: 11, to: 13, cls: 'cm-hang' }); // "> " on line two
  });
});

describe('list markers', () => {
  // A list marker is where the published page draws its own bullet or number,
  // so it stays in the text flow, faint — never hung in the margin.
  const hangs = (doc: string) => decosFor(doc).filter((x) => x.cls === 'cm-hang');
  const marks = (doc: string) => decosFor(doc).filter((x) => x.cls === 'cm-lp-list-mark');

  test('plain bullet "- item": marker is a faint mark, not a hang', () => {
    expect(hangs('- item')).toEqual([]);
    expect(marks('- item')).toEqual([{ from: 0, to: 1, cls: 'cm-lp-list-mark' }]);
  });
  test('nested "  - item": the marker is faint in the flow, the indent is plain text', () => {
    expect(hangs('- parent\n  - nested')).toEqual([]);
    expect(marks('- parent\n  - nested')).toContainEqual({ from: 11, to: 12, cls: 'cm-lp-list-mark' });
  });
  test('ordered "1. item" and "2) item" are faint marks, not hangs', () => {
    expect(hangs('1. item\n2) two')).toEqual([]);
    expect(marks('1. item\n2) two')).toEqual([
      { from: 0, to: 2, cls: 'cm-lp-list-mark' },
      { from: 8, to: 10, cls: 'cm-lp-list-mark' },
    ]);
  });
  test('a task\'s "- " before its checkbox stays in the flow, in the accent colour, not a hang', () => {
    expect(hangs('- [ ] task')).toEqual([]);
    expect(marks('- [ ] task')).toEqual([]);
    expect(decosFor('- [ ] task')).toContainEqual({ from: 0, to: 1, cls: 'cm-lp-task-dash' });
  });
  test('"> - item": only the quote mark hangs; the list marker stays in the flow', () => {
    expect(hangs('> - item')).toEqual([{ from: 0, to: 2, cls: 'cm-hang' }]);
    expect(marks('> - item')).toEqual([{ from: 2, to: 3, cls: 'cm-lp-list-mark' }]);
  });
});

describe('embed source stays on screen, styled', () => {
  test('![[path|50%]]: brackets and the width suffix faint, the path in the link colour', () => {
    const d = decosFor('![[images/logo.png|50%]]');
    expect(d).toContainEqual({ from: 0, to: 3, cls: 'cm-lp-delim' }); // ![[
    expect(d).toContainEqual({ from: 3, to: 18, cls: 'cm-lp-link-dest' }); // images/logo.png
    expect(d).toContainEqual({ from: 18, to: 22, cls: 'cm-lp-delim' }); // |50%
    expect(d).toContainEqual({ from: 22, to: 24, cls: 'cm-lp-delim' }); // ]]
    expect(d.some((x) => x.cls === undefined)).toBe(false); // nothing replaced
  });
  test('![Alt](path): the punctuation faint, the path in the link colour, the alt left alone', () => {
    const d = decosFor('![Alt](p.png)');
    expect(d).toContainEqual({ from: 0, to: 2, cls: 'cm-lp-delim' });
    expect(d).toContainEqual({ from: 6, to: 7, cls: 'cm-lp-delim' });
    expect(d).toContainEqual({ from: 7, to: 12, cls: 'cm-lp-link-dest' });
    expect(d.some((x) => x.from >= 2 && x.to <= 5)).toBe(false); // "Alt"
  });
});

describe('thematic break', () => {
  test('--- stays as faint text, its line carries the hairline class, nothing is replaced', () => {
    const d = decosFor('above\n\n---\n\nbelow');
    expect(d).toContainEqual({ from: 7, to: 7, cls: 'cm-lp-rule' });
    expect(d).toContainEqual({ from: 7, to: 10, cls: 'cm-lp-delim' });
    expect(d.every((x) => x.cls !== undefined)).toBe(true); // no replace (no class, a widget) anywhere
  });
  test('lists replace nothing either: every decoration is a class, or a zero-width widget that hides no text', () => {
    const doc = ['- a', '  - nested', '    - deeper wrapped item', '    lazy tail', '1. one', '10. ten', '- [ ] task', '- [x] done', '\t- tab nested', '-\ttab after marker', '> - quoted', '- # heading item', '  continued', ''].join('\n');
    const state = EditorState.create({ doc, extensions: [markdown({ extensions: [GFM, wikilinkConfig, mathConfig, footnoteConfig] })] });
    const set = buildMarkdownStyleDecorations(state, [{ from: 0, to: doc.length }]);
    let widgets = 0, lists = 0;
    for (const cur = set.iter(); cur.value; cur.next()) {
      const spec = cur.value.spec as { class?: string; widget?: unknown };
      if (spec.class === 'cm-li-hang') lists++;
      if (spec.class === undefined) { widgets++; expect(spec.widget, `a classless decoration at ${cur.from} is a widget`).toBeDefined(); expect(cur.to, `and hides no text at ${cur.from}`).toBe(cur.from); }
    }
    expect(lists).toBeGreaterThan(10);
    expect(widgets).toBeGreaterThan(5);
  });
  test('a setext heading underline is not a rule', () => {
    expect(decosFor('Title\n---').some((x) => x.cls === 'cm-lp-rule')).toBe(false);
  });
});

describe('fenced code and math fences', () => {
  test('fenced code stays in the text column (no hang); the language name takes the accent mark', () => {
    const d = decosFor('```js\ncode here\n```');
    expect(d.some((x) => x.cls === 'cm-hang' || x.cls === 'cm-hung')).toBe(false);
    expect(d).toContainEqual({ from: 3, to: 5, cls: 'cm-lp-fence-lang' }); // "js"
  });
  test('inline code marker is never hung', () => {
    const d = decosFor('a `code` b');
    expect(d.some((x) => x.cls === 'cm-hang')).toBe(false);
  });
  test('display math $$ fences stay in the column, faint; interior tinted', () => {
    const d = decosFor('$$\nx^2\n$$');
    expect(d.some((x) => x.cls === 'cm-hang')).toBe(false);
    expect(d).toContainEqual({ from: 0, to: 2, cls: 'cm-lp-delim' }); // opening $$
    expect(d).toContainEqual({ from: 7, to: 9, cls: 'cm-lp-delim' }); // closing $$
    expect(d).toContainEqual({ from: 3, to: 6, cls: 'cm-lp-math' }); // "x^2" interior
  });
});

describe('strikethrough and inline-code payload style', () => {
  test('strikethrough payload gets cm-lp-strike (mossHighlight has no rule for it)', () => {
    const d = decosFor('~~gone~~');
    expect(d).toContainEqual({ from: 2, to: 6, cls: 'cm-lp-strike' });
  });
  test('inline code payload gets cm-lp-code', () => {
    const d = decosFor('`x`');
    expect(d).toContainEqual({ from: 1, to: 2, cls: 'cm-lp-code' });
  });
});

describe('wikilink and inline math', () => {
  test('wikilink: brackets faint, label styled, aliased target muted', () => {
    const d = decosFor('[[Page|Label]]');
    expect(d).toContainEqual({ from: 0, to: 2, cls: 'cm-lp-delim' }); // [[
    // "[[Page|Label]]" is 14 chars (0-13). Its closing "]]" occupies
    // the half-open span [12, 14), including both delimiters.
    expect(d).toContainEqual({ from: 12, to: 14, cls: 'cm-lp-delim' }); // ]]
    expect(d).toContainEqual({ from: 2, to: 6, cls: 'cm-lp-link-dest' }); // Page
    expect(d).toContainEqual({ from: 7, to: 12, cls: 'cm-lp-link-text' }); // Label
  });
  test('inline math: fences faint, payload tinted', () => {
    const d = decosFor('$x$');
    expect(d).toContainEqual({ from: 0, to: 1, cls: 'cm-lp-delim' });
    expect(d).toContainEqual({ from: 1, to: 2, cls: 'cm-lp-math' });
  });
});

describe('footnote reference', () => {
  test('[^1] keeps its characters: a faint raised mark over the whole reference, no replace', () => {
    const d = decosFor('A note.[^1]\n\n[^1]: The note.');
    expect(d).toContainEqual({ from: 7, to: 11, cls: 'cm-footnote-ref cm-link-clickable' });
    // moss-syntax's own chip is a replace (a widget, so no class): none may remain.
    expect(d.every((x) => x.cls !== undefined)).toBe(true);
  });
  test('the definition prefix stays in the column, faint, and an undefined label is left as plain text', () => {
    const d = decosFor('A note.[^1]\n\n[^1]: The note.\n\nA regex [^0-9] class.');
    expect(d).toContainEqual({ from: 13, to: 18, cls: 'cm-lp-delim' });
    expect(d.some((x) => x.cls === 'cm-hang' || x.cls === 'cm-hung')).toBe(false);
    expect(d.filter((x) => x.cls?.startsWith('cm-footnote-ref'))).toHaveLength(1);
  });
});

describe('callout label is not a link', () => {
  test('> [!note] gets no link styling (cm-callout.ts tints it)', () => {
    expect(decosFor('> [!note] title').some((x) => x.cls === 'cm-lp-link-text')).toBe(false);
  });
  test('a [!word] link elsewhere still does', () => {
    expect(decosFor('see [!note] here').some((x) => x.cls === 'cm-lp-link-text')).toBe(true);
  });
});

describe('a link shows every character, styled', () => {
  // [text](https://x): `[` 0-1, text 1-5, `]` 5-6, `(` 6-7, URL 7-16, `)` 16-17
  const d = decosFor('[text](https://x)');
  test('the label takes the link colour', () => {
    expect(d).toContainEqual({ from: 1, to: 5, cls: 'cm-lp-link-text' });
  });
  test('the URL is faint; the brackets and parentheses are tagged by mossHighlight, not here', () => {
    // source-colours.spec.ts asserts the computed colour of every mark.
    expect(d).toContainEqual({ from: 7, to: 16, cls: 'cm-lp-link-dest' });
    expect(d.some((x) => x.cls === 'cm-lp-delim')).toBe(false);
  });
  test('everything from `]` on is smaller, and nothing is replaced', () => {
    expect(d).toContainEqual({ from: 5, to: 17, cls: 'cm-lp-link-tail' });
    expect(d.every((x) => x.cls !== undefined)).toBe(true);
  });
  test('a picture inside the text keeps its own marks: the text colour goes around it', () => {
    const linked = decosFor('[see ![alt](p.png) here](/a/)');
    expect(linked).toContainEqual({ from: 1, to: 5, cls: 'cm-lp-link-text' });
    expect(linked).toContainEqual({ from: 18, to: 23, cls: 'cm-lp-link-text' });
    expect(linked).toContainEqual({ from: 12, to: 17, cls: 'cm-lp-link-dest' }); // p.png
    expect(linked.some((x) => x.cls === 'cm-lp-link-text' && x.from < 12 && x.to > 12)).toBe(false);
  });
  test('a link with no destination styles its label only', () => {
    const empty = decosFor('[text]()');
    expect(empty).toContainEqual({ from: 1, to: 5, cls: 'cm-lp-link-text' });
    expect(empty.some((x) => x.cls === 'cm-lp-link-tail' || x.cls === 'cm-lp-link-dest')).toBe(false);
  });
});

describe('a task shows its dash and brackets, styled', () => {
  const done = decosFor('- [x] done');
  const open = decosFor('- [ ] todo');
  test('the dash is in the accent colour, not the muted list-marker colour', () => {
    expect(done).toContainEqual({ from: 0, to: 1, cls: 'cm-lp-task-dash' });
    expect(done.some((x) => x.cls === 'cm-lp-list-mark')).toBe(false);
    expect(decosFor('- plain').some((x) => x.cls === 'cm-lp-task-dash')).toBe(false);
  });
  test('the whole [ ] is the click target; the brackets are faint', () => {
    for (const d of [done, open]) {
      expect(d).toContainEqual({ from: 2, to: 5, cls: 'cm-task-toggle' });
      expect(d).toContainEqual({ from: 2, to: 3, cls: 'cm-lp-delim' });
      expect(d).toContainEqual({ from: 4, to: 5, cls: 'cm-lp-delim' });
    }
  });
  test('only a done task has an accented x', () => {
    expect(done).toContainEqual({ from: 3, to: 4, cls: 'cm-lp-task-x' });
    expect(open.some((x) => x.cls === 'cm-lp-task-x')).toBe(false);
  });
});

describe('done-task muting', () => {
  test('a checked task mutes its text; an unchecked one does not', () => {
    const d = decosFor('- [x] done\n- [ ] not done');
    expect(d).toContainEqual({ from: 5, to: 10, cls: 'cm-lp-done' });
    expect(d.some((x) => x.cls === 'cm-lp-done' && x.from > 10)).toBe(false);
  });
});

describe('table styling comes from mossHighlight, not this module', () => {
  test('table pipes and header row already come from mossHighlight, not this module', () => {
    const state = EditorState.create({
      doc: '| a | b |\n| - | - |\n| 1 | 2 |',
      extensions: [markdown({ extensions: [Table] }), syntaxHighlighting(mossHighlight)],
    });
    const dom = document.createElement('div');
    const view = new EditorView({ state, parent: dom });
    const lines = view.dom.querySelectorAll('.cm-line');
    // The plan's own draft asserted a literal "tok-" substring, which
    // CodeMirror's style-mod never emits (it generates opaque "ͼ<n>" atom
    // classes instead) — that assertion could never have passed, plan or no
    // plan. Assert the actual coverage claim instead: a TableDelimiter pipe
    // gets styled everywhere (every pipe span carries a class, proving
    // `tags.processingInstruction` → faint already applies with zero code
    // here), and a HEADER row's pipe carries an EXTRA class the same pipe in
    // a body row does not — proving `TableHeader` → `tags.heading` already
    // bolds the header row for free.
    const headerPipe = lines[0].querySelector('span');
    const bodyPipe = lines[2].querySelector('span');
    expect(headerPipe?.className).toBeTruthy();
    expect(bodyPipe?.className).toBeTruthy();
    expect(headerPipe!.className.split(' ').length).toBeGreaterThan(bodyPipe!.className.split(' ').length);
    view.destroy();
  });
});

describe('caret invariance', () => {
  test('CARET INVARIANCE: decorations are identical at every caret position over a corpus', () => {
    const corpus = [
      '# H1',
      '## H2 with `code` and [text](u) and [[Wiki|Alias]]',
      '> quote\n> more',
      '- item\n- [x] done\n- [ ] not',
      '1. one\n2. two',
      '```js\ncode\n```',
      '$$\nx^2\n$$',
      '$x$',
      '~~strike~~',
    ].join('\n');
    const state = EditorState.create({
      doc: corpus,
      extensions: [markdown({ extensions: [GFM, wikilinkConfig, mathConfig] })],
    });
    const flatten = (set: DecorationSet) => {
      const out: unknown[] = [];
      const cur = set.iter();
      while (cur.value) {
        out.push([cur.from, cur.to, (cur.value.spec as { class?: string }).class]);
        cur.next();
      }
      return out;
    };
    const baseline = flatten(
      buildMarkdownStyleDecorations(
        EditorState.create({ doc: corpus, extensions: [markdown({ extensions: [GFM, wikilinkConfig, mathConfig] })] }),
        [{ from: 0, to: corpus.length }],
      ),
    );
    for (let pos = 0; pos <= corpus.length; pos += 7) {
      // buildMarkdownStyleDecorations takes no selection parameter at all —
      // passing a state whose selection.main.head differs must produce
      // byte-identical output, since the function has nowhere to read it from.
      const withCaret = state.update({ selection: { anchor: pos } }).state;
      const decos = flatten(buildMarkdownStyleDecorations(withCaret, [{ from: 0, to: corpus.length }]));
      expect(decos).toEqual(baseline);
    }
  });
});

describe('no needless rebuild', () => {
  test('NO SELECTION REBUILD: a selection-only transaction never rebuilds the plugin', () => {
    // Ported from cm-live-preview-rebuild-gate.test.ts's "image block field
    // rebuild gate" describe: identity-based assertions prove a rebuild
    // happened (or didn't) without inspecting the decorations themselves.
    // markdownStylePlugin is a ViewPlugin, not a StateField, so the
    // instance itself (not a facet-provided DecorationSet) is what we ask
    // for an identity from.
    const plugin = markdownStylePlugin;
    const state = EditorState.create({ doc: '# H1\n\nsome text', extensions: [markdown(), markdownStyleExtension()] });
    const dom = document.createElement('div');
    const view = new EditorView({ state, parent: dom });
    const before = view.plugin(plugin as never) as { decorations: DecorationSet } | null;
    expect(before).not.toBeNull();
    const beforeDecorations = before!.decorations;

    view.dispatch({ selection: { anchor: 3 } });

    const after = view.plugin(plugin as never) as { decorations: DecorationSet };
    expect(after.decorations).toBe(beforeDecorations); // same object — update()'s early return never reassigned it
    view.destroy();
  });
});

describe('the caret rules read one line, never the document', () => {
  test('a keystroke asks the tree about the typed line only', () => {
    const doc = Array.from({ length: 200 }, (_, i) => (i % 5 === 0 ? `## Heading ${i}` : `> quote ${i}`)).join('\n');
    const view = new EditorView({
      state: EditorState.create({ doc, extensions: [markdown({ extensions: [GFM] }), markdownStyleExtension()] }),
      parent: document.body,
    });
    const line = view.state.doc.line(100);
    const spy = vi.spyOn(Object.getPrototypeOf(syntaxTree(view.state)), 'iterate');
    spy.mockClear();
    view.dispatch({ changes: { from: line.to, insert: 'x' }, selection: { anchor: line.to + 1 }, userEvent: 'input.type' });
    const walks = spy.mock.calls.map(([spec]) => spec as { from?: number; to?: number });
    spy.mockRestore();
    view.destroy();
    // The typed line (now one character longer) was asked about on its own: the filter did not walk the document for it.
    expect(walks.some((w) => w.from === line.from && w.to === line.to + 1)).toBe(true);
  });
});

describe('CSS: cm-hang/cm-hung base rule', () => {
  test('cm-markdown-style.css defines the hang positioning rule once', async () => {
    const fs = await import('node:fs');
    const path = await import('node:path');
    const css = fs.readFileSync(path.join(__dirname, '../cm-markdown-style.css'), 'utf8');
    expect(css).toMatch(/\.cm-hung\s*\{[^}]*position:\s*relative/);
    expect(css).toMatch(/\.cm-hang(?:,\s*\.cm-hang-run)?\s*\{[^}]*position:\s*absolute/);
  });
});

describe('a hung indicator is not a caret position', () => {
  function viewFor(doc: string, anchor: number): EditorView {
    const state = EditorState.create({
      doc,
      selection: { anchor },
      extensions: [markdown({ extensions: [GFM, wikilinkConfig, mathConfig] }), markdownStyleExtension()],
    });
    return new EditorView({ state, parent: document.body });
  }

  const press = (view: EditorView, init: KeyboardEventInit) => runScopeHandlers(view, new KeyboardEvent('keydown', init), 'editor');

  test.each([
    ['heading', 'intro\n## Title', 9],
    ['quote', 'intro\n> quoted', 8],
    ['nested quote', 'intro\n> > deep', 10],
  ])('%s: ← from the text start goes to the previous line end, → goes back to the text start', (_n, doc, textStart) => {
    const view = viewFor(doc, textStart);
    expect(press(view, { key: 'ArrowLeft' })).toBe(true);
    expect(view.state.selection.main.head).toBe(5);
    cursorCharRight(view);
    expect(view.state.selection.main.head).toBe(textStart);
    expect(view.state.selection.main.assoc).toBe(1);
    view.destroy();
  });

  test('on the first line ← from the text start stays put (nothing before it)', () => {
    const view = viewFor('## Title', 3);
    expect(press(view, { key: 'ArrowLeft' })).toBe(true);
    expect(view.state.selection.main.head).toBe(3);
    view.destroy();
  });

  test('⌥← (word left) from the text start also goes to the previous line end', () => {
    const view = viewFor('intro\n## Title', 9);
    expect(press(view, { key: 'ArrowLeft', ctrlKey: true })).toBe(true);
    expect(view.state.selection.main.head).toBe(5);
    view.destroy();
  });

  test('← elsewhere is left to CM6, and so is a range that is not at a text start', () => {
    const view = viewFor('intro\n## Title', 11);
    expect(press(view, { key: 'ArrowLeft' })).toBe(false);
    view.dispatch({ selection: EditorSelection.range(9, 12) });
    expect(press(view, { key: 'ArrowLeft' })).toBe(false);
    view.destroy();
  });

  test('Home from mid-text lands on the text start, never in the margin', () => {
    const view = viewFor('## Title', 6);
    cursorLineBoundaryBackward(view);
    expect(view.state.selection.main.head).toBe(3);
    view.destroy();
  });

  test('a selection spanning the whole line keeps the indicator characters (copy gives the full source)', () => {
    const view = viewFor('intro\n## Title', 5);
    view.dispatch({ selection: EditorSelection.range(5, 14) });
    expect(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to)).toBe('\n## Title');
    view.dispatch({ selection: EditorSelection.range(14, 6) }); // the head comes from the text: it goes to the previous line end, the anchor stays
    expect(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to)).toBe('\n## Title');
    view.destroy();
  });

  test('Backspace at the text start deletes the whole prefix (heading becomes a paragraph)', () => {
    const view = viewFor('## Title', 3);
    deleteCharBackward(view);
    expect(view.state.doc.toString()).toBe('Title');
    view.destroy();
  });

  test.each([
    ['bullet', '- item', 2],
    ['ordered', '1. item', 3],
  ])('%s marker is ordinary text: ← from the text start steps one character, never over the marker', (_n, doc, textStart) => {
    const view = viewFor(doc, textStart);
    cursorCharLeft(view);
    expect(view.state.selection.main.head).toBe(textStart - 1);
    cursorCharLeft(view);
    expect(view.state.selection.main.head).toBe(textStart - 2);
    view.destroy();
  });

  test('Backspace at a list item\'s text start deletes one character, like any text', () => {
    const view = viewFor('- item', 2);
    deleteCharBackward(view);
    expect(view.state.doc.toString()).toBe('-item');
    view.destroy();
  });

  test('typing "#" at the line start still deepens a heading', () => {
    const view = viewFor('## Title', 0);
    view.dispatch({ changes: { from: 0, insert: '#' }, userEvent: 'input.type' });
    expect(view.state.doc.toString()).toBe('### Title');
    view.destroy();
  });

  test('a nested list item: indent and marker are ordinary characters, one step each', () => {
    const view = viewFor('- parent\n  - child', 13); // 13 = "child" start
    cursorCharLeft(view);
    expect(view.state.selection.main.head).toBe(12); // the space after the marker
    cursorCharLeft(view);
    expect(view.state.selection.main.head).toBe(11); // the marker
    view.destroy();
  });

  test('Home on a nested item lands on its marker (first non-blank), not in the margin', () => {
    const view = viewFor('- parent\n  - child', 15);
    cursorLineBoundaryBackward(view);
    expect(view.state.selection.main.head).toBe(11);
    view.destroy();
  });
});

describe('the caret filter pins the text side inside the transaction', () => {
  function viewWith(doc: string, selection: EditorSelection): EditorView {
    const state = EditorState.create({
      doc,
      selection,
      extensions: [markdown({ extensions: [GFM, wikilinkConfig, mathConfig] }), markdownStyleExtension()],
    });
    return new EditorView({ state, parent: document.body });
  }

  test('assoc at a prefix end is the text side', () => {
    const view = viewWith('## Title', EditorSelection.single(0));
    view.dispatch({ selection: EditorSelection.create([EditorSelection.cursor(3, -1)]) });
    expect(view.state.selection.main.head).toBe(3);
    expect(view.state.selection.main.assoc).toBe(1);
    view.destroy();
  });

  test('no extra transaction is dispatched: the pin lands in the very update that moved the caret', () => {
    const view = viewWith('## Title', EditorSelection.single(0));
    const seen: boolean[] = [];
    const plain = view.dispatch.bind(view);
    let dispatched = 0;
    view.dispatch = ((...a: Parameters<EditorView['dispatch']>) => { dispatched++; return plain(...a); }) as EditorView['dispatch'];
    view.dispatch({ selection: EditorSelection.create([EditorSelection.cursor(3, -1)]) });
    seen.push(view.state.selection.main.assoc === 1);
    expect(seen).toEqual([true]); // already pinned synchronously — no microtask needed
    expect(dispatched).toBe(1);
    view.destroy();
  });

  test.each([
    // [what, doc, anchor, head, want anchor, want head, drawn on the text side]
    ['from above: the head goes to the text start', 'intro\n## Title', 2, 7, 2, 9, true],
    ['from above, head on the line start: it stays (the break, not the marker)', 'intro\n## Title', 5, 6, 5, 6, false],
    ['from the text or a later line: the end of the previous line', 'a\n## T\nb', 8, 2, 8, 1, false],
    ['from this line\'s own text: the end of the previous line', 'intro\n## Title', 12, 6, 12, 5, false],
    ['on the first line: the document start, so the indicator is selected too', '## Title', 8, 1, 8, 0, false],
    ['on the first line, from a later line', '## Title\nb', 10, 1, 10, 0, false],
    ['inside a nested quote run', 'x\n> > deep', 9, 3, 9, 1, false],
  ])('a selection head in the indicator: %s', (_what, doc, anchor, head, wantAnchor, wantHead, textSide) => {
    const view = viewWith(doc, EditorSelection.single(doc.length));
    view.dispatch({ selection: EditorSelection.range(anchor, head), userEvent: 'select.pointer' });
    const r = view.state.selection.main;
    expect([r.anchor, r.head]).toEqual([wantAnchor, wantHead]);
    if (textSide) expect(r.assoc).toBe(1); // drawn at the text, not at the indicator's right edge in the margin
    view.destroy();
  });

  test('Shift-← from the text start extends over the indicator to the previous line end', () => {
    const view = viewWith('intro\n## Title', EditorSelection.single(9));
    selectCharLeft(view);
    expect([view.state.selection.main.anchor, view.state.selection.main.head]).toEqual([9, 5]);
    view.destroy();
  });

  test('Shift-Home from mid-text selects to the text start, not into the previous line', () => {
    const view = viewWith('intro\n## Title', EditorSelection.single(12));
    expect(runScopeHandlers(view, new KeyboardEvent('keydown', { key: 'Home', shiftKey: true }), 'editor')).toBe(true);
    const r = view.state.selection.main;
    expect([r.anchor, r.head, r.assoc]).toEqual([12, 9, 1]);
    view.destroy();
  });

  test('Shift-Home on a line with no indicator is left to CM6', () => {
    const view = viewWith('intro\n## Title', EditorSelection.single(3));
    expect(runScopeHandlers(view, new KeyboardEvent('keydown', { key: 'Home', shiftKey: true }), 'editor')).toBe(false);
    view.destroy();
  });

  test('a head already in the text, and an anchor inside the indicator, are left alone', () => {
    const view = viewWith('intro\n## Title', EditorSelection.single(0));
    view.dispatch({ selection: EditorSelection.range(6, 14) }); // a whole line: anchor at the line start, head at its end
    expect(view.state.selection.main.anchor).toBe(6);
    expect(view.state.selection.main.head).toBe(14);
    view.destroy();
  });

  test('goalColumn survives the pin', () => {
    const view = viewWith('## Title', EditorSelection.single(0));
    view.dispatch({ selection: EditorSelection.create([EditorSelection.cursor(3, -1, undefined, 42)]) });
    expect(view.state.selection.main.assoc).toBe(1);
    expect(view.state.selection.main.goalColumn).toBe(42);
    view.destroy();
  });

  test('typing the prefix pins the caret too (a doc change, not just a selection set)', () => {
    const view = viewWith('#', EditorSelection.single(1));
    view.dispatch({ changes: { from: 1, insert: ' ' }, selection: EditorSelection.create([EditorSelection.cursor(2, -1)]), userEvent: 'input.type' });
    expect(view.state.selection.main.head).toBe(2);
    expect(view.state.selection.main.assoc).toBe(1);
    view.destroy();
  });

  test('composition is not touched: an input.type.compose transaction passes through', () => {
    const view = viewWith('#', EditorSelection.single(1));
    view.dispatch({ changes: { from: 1, insert: ' ' }, selection: EditorSelection.create([EditorSelection.cursor(2, -1)]), userEvent: 'input.type.compose' });
    expect(view.state.selection.main.assoc).toBe(-1);
    view.destroy();
  });

  test.each([
    ['## Title', 0, 3], // the line start (a click left of the marker, or ↑ landing on it)
    ['## Title', 1, 3], // inside the marker
    ['> > deep', 1, 4], // inside a nested quote run
    ['  ## Title', 0, 5], // the indentation belongs to the indicator
  ])('%s: a caret placed at %i is moved to the text start %i', (doc, at, textStart) => {
    const view = viewWith(doc, EditorSelection.single(doc.length));
    view.dispatch({ selection: EditorSelection.single(at), userEvent: 'select.pointer' });
    expect(view.state.selection.main.head).toBe(textStart);
    expect(view.state.selection.main.assoc).toBe(1);
    view.destroy();
  });

  test('a caret is moved only in the indicator: elsewhere on the line, and on a list marker, it stays', () => {
    const view = viewWith('## Title\n- item', EditorSelection.single(5));
    view.dispatch({ selection: EditorSelection.single(10) }); // the list marker's start
    expect(view.state.selection.main.head).toBe(10);
    view.destroy();
  });
});

describe('ArrowDown/ArrowUp handler (moveLine)', () => {
  // jsdom has no layout, so the pixel probe is stubbed: what is under test is
  // what the handler does with the answer, not where CM6 puts it.
  function setup(doc = 'one\n\ntwo') {
    const state = EditorState.create({ doc, selection: EditorSelection.single(1), extensions: [markdown({ extensions: [GFM] }), markdownStyleExtension()] });
    const view = new EditorView({ state, parent: document.body });
    const moved = vi.spyOn(view, 'moveVertically').mockReturnValue(EditorSelection.cursor(6, 1, 1));
    vi.spyOn(view, 'coordsAtPos').mockReturnValue({ left: 10, right: 10, top: 0, bottom: 10 });
    const press = (key: string) => runScopeHandlers(view, new KeyboardEvent('keydown', { key }), 'editor');
    return { view, moved, press };
  }

  test('the bidi level of the moved caret survives', () => {
    // The stub returns an RTL caret (level 1): the handler must hand
    // moveVertically's level to the new cursor rather than default it.
    const { view, moved, press } = setup();
    expect(press('ArrowDown')).toBe(true);
    expect(view.state.selection.main.head).toBe(6);
    expect(view.state.selection.main.bidiLevel).toBe(moved.mock.results[0].value.bidiLevel);
    expect(view.state.selection.main.bidiLevel).toBe(1);
    view.destroy();
  });

  test('while an IME composition is open the arrow is left to the browser', () => {
    const { view, moved, press } = setup();
    Object.defineProperty(view, 'composing', { get: () => true });
    expect(press('ArrowDown')).toBe(false);
    expect(moved).not.toHaveBeenCalled();
    expect(view.state.selection.main.head).toBe(1);
    view.destroy();
  });

  test('a neighbouring line outside the rendered viewport is scrolled into it before the move', () => {
    const { view, press } = setup('one\n\ntwo');
    const dispatched: number[] = [];
    const plain = view.dispatch.bind(view);
    view.dispatch = ((...a: Parameters<EditorView['dispatch']>) => {
      for (const spec of a) for (const e of [(spec as { effects?: unknown }).effects].flat()) {
        const head = (e as { value?: { range?: { head: number } } } | undefined)?.value?.range?.head;
        if (head !== undefined) dispatched.push(head);
      }
      return plain(...a);
    }) as EditorView['dispatch'];
    Object.defineProperty(view, 'viewport', { get: () => ({ from: 0, to: 3 }) }); // only line 1 rendered
    press('ArrowDown');
    expect(dispatched[0]).toBe(4); // the start of line 2
    view.destroy();
  });

  test('a non-empty selection collapses without rendering the neighbour first', () => {
    const { view, press } = setup('one\n\ntwo');
    view.dispatch({ selection: EditorSelection.single(0, 2) });
    const dispatch = vi.spyOn(view, 'dispatch');
    Object.defineProperty(view, 'viewport', { get: () => ({ from: 0, to: 3 }) });
    expect(press('ArrowDown')).toBe(false);
    expect(dispatch).not.toHaveBeenCalled();
    view.destroy();
  });

  test('Shift-ArrowDown renders the neighbouring line too, then leaves the extension to CM6', () => {
    const { view } = setup('one\n\ntwo');
    const dispatched: number[] = [];
    const plain = view.dispatch.bind(view);
    view.dispatch = ((...a: Parameters<EditorView['dispatch']>) => {
      for (const spec of a) for (const e of [(spec as { effects?: unknown }).effects].flat()) {
        const head = (e as { value?: { range?: { head: number } } } | undefined)?.value?.range?.head;
        if (head !== undefined) dispatched.push(head);
      }
      return plain(...a);
    }) as EditorView['dispatch'];
    Object.defineProperty(view, 'viewport', { get: () => ({ from: 0, to: 3 }) }); // only line 1 rendered
    expect(runScopeHandlers(view, new KeyboardEvent('keydown', { key: 'ArrowDown', shiftKey: true }), 'editor')).toBe(false);
    expect(dispatched[0]).toBe(4); // the start of line 2
    view.destroy();
  });
});

describe('an indicator tooltip follows the app language', () => {
  const tooltip = (view: EditorView) => view.dom.querySelector('.cm-hang')?.getAttribute('data-tooltip');
  test('switching language relabels the tooltip without an edit, and stops listening when the view is destroyed', () => {
    const view = new EditorView({
      state: EditorState.create({ doc: '## Title', extensions: [markdown({ extensions: [GFM] }), markdownStyleExtension()] }),
      parent: document.body,
    });
    try {
      expect(tooltip(view)).toBe('Heading 2');
      setLocale('zh-hans');
      expect(tooltip(view)).toBe('标题 2');
      setLocale('en');
      expect(tooltip(view)).toBe('Heading 2');
      view.destroy();
      const dispatch = vi.spyOn(view, 'dispatch');
      setLocale('zh-hans');
      expect(dispatch).not.toHaveBeenCalled(); // a destroyed view is not listening
    } finally {
      setLocale('en');
      view.destroy();
    }
  });
});

describe('an escape', () => {
  test('only the backslash is faint; the escaped character stays plain', () => {
    const doc = 'a \\*b\\* c';
    const d = decosFor(doc);
    const first = doc.indexOf('\\');
    const faint = d.filter((x) => x.cls === 'cm-lp-delim').map((x) => [x.from, x.to]);
    expect(faint).toEqual([[first, first + 1], [first + 3, first + 4]]);
  });
});
