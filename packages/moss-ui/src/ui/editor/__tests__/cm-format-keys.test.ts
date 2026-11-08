import { describe, test, expect, afterEach, beforeAll } from 'vitest';
import { history, undo } from '@codemirror/commands';
import { markdown } from '@codemirror/lang-markdown';
import { GFM } from '@lezer/markdown';
import { markdownStyleExtension } from '../cm-markdown-style';
import { EditorView, runScopeHandlers } from '@codemirror/view';
import { EditorSelection, EditorState } from '@codemirror/state';
import { formatKeysExtension, lineIndicatorExtension, setLineType, toggleHeading, toggleBlockquote } from '../cm-format-keys';
import { hangsOnLine } from '../cm-hangs';

function makeView(doc: string): { view: EditorView; container: HTMLElement } {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const state = EditorState.create({
    doc,
    extensions: [markdown({ extensions: [GFM] }), formatKeysExtension()],
  });
  const view = new EditorView({ state, parent: container });
  return { view, container };
}

/**
 * Fire a key through CM6's keymap dispatch. Uses runScopeHandlers which
 * is the correct way to test CM6 keymaps in jsdom — direct dispatchEvent
 * works in browsers but CM6's keymap handler may not fire in jsdom.
 */
function fireKey(view: EditorView, key: string): boolean {
  const parts = key.split('-');
  const baseKey = parts[parts.length - 1];
  const hasMod = parts.includes('Mod');
  const hasShift = parts.includes('Shift');
  const hasAlt = parts.includes('Alt');

  const event = new KeyboardEvent('keydown', {
    key: baseKey,
    metaKey: hasMod && /Mac|iPhone|iPad/.test(navigator.platform),
    ctrlKey: hasMod && !/Mac|iPhone|iPad/.test(navigator.platform),
    shiftKey: hasShift,
    altKey: hasAlt,
    bubbles: true,
    cancelable: true,
  });
  return runScopeHandlers(view, event, 'editor');
}

describe('formatKeysExtension', () => {
  let view: EditorView;
  let container: HTMLElement;

  afterEach(() => {
    view.destroy();
    document.body.innerHTML = '';
  });

  // ── Bold ──────────────────────────────────────────────────────────────

  test('Mod-b wraps selection with bold markers', () => {
    ({ view, container } = makeView('hello'));
    view.dispatch({ selection: { anchor: 0, head: 5 } });
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('**hello**');
    expect(view.state.selection.main.anchor).toBe(9); // after closing **
  });

  test('Mod-b toggles off bold when inner text is selected', () => {
    ({ view, container } = makeView('**hello**'));
    view.dispatch({ selection: { anchor: 2, head: 7 } }); // select "hello"
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('hello');
  });

  test('Mod-b at a word edge expands to the word (no bare **** injection)', () => {
    // The cursor at position 0 touches "hello" — word expansion wraps it.
    // (The bare marker-pair insert only happens with no word at the cursor;
    // see the whitespace test below.)
    ({ view, container } = makeView('hello'));
    view.dispatch({ selection: { anchor: 0, head: 0 } });
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('**hello**');
    expect(view.state.selection.main.anchor).toBe(2);
  });

  test('Mod-b toggles off bold when "hi" selected inside **hi**', () => {
    ({ view, container } = makeView('**hi**'));
    view.dispatch({ selection: { anchor: 2, head: 4 } });
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('hi');
  });

  // ── Italic ────────────────────────────────────────────────────────────

  test('Mod-i wraps selection with italic markers', () => {
    ({ view, container } = makeView('world'));
    view.dispatch({ selection: { anchor: 0, head: 5 } });
    fireKey(view, 'Mod-i');
    expect(view.state.doc.toString()).toBe('*world*');
    expect(view.state.selection.main.anchor).toBe(7);
  });

  // ── Strikethrough ─────────────────────────────────────────────────────

  test('Mod-Shift-s wraps selection with strikethrough markers', () => {
    ({ view, container } = makeView('foo'));
    view.dispatch({ selection: { anchor: 0, head: 3 } });
    fireKey(view, 'Mod-Shift-s');
    expect(view.state.doc.toString()).toBe('~~foo~~');
    expect(view.state.selection.main.anchor).toBe(7);
  });

  // ── Inline code ───────────────────────────────────────────────────────

  test('Mod-~ wraps selection with inline code markers', () => {
    ({ view, container } = makeView('bar'));
    view.dispatch({ selection: { anchor: 0, head: 3 } });
    fireKey(view, 'Mod-~');
    expect(view.state.doc.toString()).toBe('`bar`');
    expect(view.state.selection.main.anchor).toBe(5);
  });

  // ── Link ──────────────────────────────────────────────────────────────

  test('Mod-k wraps selection with link syntax, cursor INSIDE the parens', () => {
    ({ view, container } = makeView('click'));
    view.dispatch({ selection: { anchor: 0, head: 5 } });
    fireKey(view, 'Mod-k');
    expect(view.state.doc.toString()).toBe('[click]()');
    // The URL is what the user types next — land them inside ().
    expect(view.state.selection.main.anchor).toBe(8);
  });

  test('Mod-k toggles off link with empty URL', () => {
    ({ view, container } = makeView('[click]()'));
    view.dispatch({ selection: { anchor: 1, head: 6 } }); // select "click"
    fireKey(view, 'Mod-k');
    expect(view.state.doc.toString()).toBe('click');
  });

  test('Mod-k toggles off link with non-empty URL', () => {
    ({ view, container } = makeView('[click](https://x.com)'));
    view.dispatch({ selection: { anchor: 1, head: 6 } }); // select "click"
    fireKey(view, 'Mod-k');
    expect(view.state.doc.toString()).toBe('click');
  });

  test('Mod-k with no selection inserts []() with cursor in link-text position', () => {
    ({ view, container } = makeView('hello'));
    view.dispatch({ selection: { anchor: 0, head: 0 } });
    fireKey(view, 'Mod-k');
    expect(view.state.doc.toString()).toBe('[]()hello');
    expect(view.state.selection.main.anchor).toBe(1); // inside [ | ]()
  });

  // ── Word expansion (collapsed cursor) ────────────────────────────────

  test('Mod-b with the cursor inside a plain word wraps the word', () => {
    ({ view, container } = makeView('say hello there'));
    view.dispatch({ selection: { anchor: 6 } }); // inside "hello"
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('say **hello** there');
    // Caret stays at its text position (shifted by the opening marker).
    expect(view.state.selection.main.anchor).toBe(8);
  });

  test('Mod-b with the cursor inside an already-bold word toggles it off', () => {
    ({ view, container } = makeView('say **hello** there'));
    view.dispatch({ selection: { anchor: 8 } }); // inside "hello"
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('say hello there');
    expect(view.state.selection.main.anchor).toBe(6);
  });

  test('Mod-i with the cursor inside a plain word wraps the word', () => {
    ({ view, container } = makeView('say hello there'));
    view.dispatch({ selection: { anchor: 6 } });
    fireKey(view, 'Mod-i');
    expect(view.state.doc.toString()).toBe('say *hello* there');
  });

  test('Mod-b on whitespace (no word) inserts an empty marker pair', () => {
    ({ view, container } = makeView('a  b'));
    view.dispatch({ selection: { anchor: 2 } }); // between the two spaces
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('a **** b');
    expect(view.state.selection.main.anchor).toBe(4); // inside the pair
  });

  // ── Marker-inclusive selection toggle ────────────────────────────────

  test('selecting **bold** INCLUDING markers + Mod-b unwraps (no double-wrap)', () => {
    ({ view, container } = makeView('**hello**'));
    view.dispatch({ selection: { anchor: 0, head: 9 } });
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('hello');
  });

  test('selecting [text](url) INCLUDING markers + Mod-k unwraps', () => {
    ({ view, container } = makeView('[click](https://x.com)'));
    view.dispatch({ selection: { anchor: 0, head: 22 } });
    fireKey(view, 'Mod-k');
    expect(view.state.doc.toString()).toBe('click');
  });

  // ── Italic/bold collision ─────────────────────────────────────────────

  test('Mod-b on italic text produces bold-italic (valid markdown)', () => {
    ({ view, container } = makeView('*hello*'));
    view.dispatch({ selection: { anchor: 0, head: 7 } }); // select full *hello*
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('***hello***');
  });

  // ── Partial mark (no toggle) ──────────────────────────────────────────

  test('partial mark selection wraps rather than toggling', () => {
    // doc: **abc — selection 0–5, chars before pos 0 are empty, so no toggle
    ({ view, container } = makeView('**abc'));
    view.dispatch({ selection: { anchor: 0, head: 5 } });
    fireKey(view, 'Mod-b');
    expect(view.state.doc.toString()).toBe('****abc**'); // wrapped, not toggled
  });

  // ── Blockquote ────────────────────────────────────────────────────────

  test('Mod-Shift-9 prefixes the caret line with "> "', () => {
    ({ view, container } = makeView('hello'));
    view.dispatch({ selection: { anchor: 2 } });
    fireKey(view, 'Mod-Shift-9');
    expect(view.state.doc.toString()).toBe('> hello');
  });

  test('Mod-Shift-9 again toggles the "> " prefix off', () => {
    ({ view, container } = makeView('> hello'));
    view.dispatch({ selection: { anchor: 4 } });
    fireKey(view, 'Mod-Shift-9');
    expect(view.state.doc.toString()).toBe('hello');
  });

  // ── Heading hotkey ────────────────────────────────────────────────────

  test('Mod-Alt-2 toggles the caret line to H2', () => {
    ({ view, container } = makeView('Title'));
    view.dispatch({ selection: { anchor: 2 } });
    fireKey(view, 'Mod-Alt-2');
    expect(view.state.doc.toString()).toBe('## Title');
  });

  test.each([[1, '# Title'], [3, '### Title'], [6, '###### Title']])('Mod-Alt-%i makes the caret line that heading level, and again toggles it off', (level, want) => {
    ({ view, container } = makeView('Title'));
    view.dispatch({ selection: { anchor: 2 } });
    fireKey(view, `Mod-Alt-${level}`);
    expect(view.state.doc.toString()).toBe(want);
    fireKey(view, `Mod-Alt-${level}`);
    expect(view.state.doc.toString()).toBe('Title');
  });
});

describe('toggleHeading', () => {
  let view: EditorView;
  let container: HTMLElement;

  afterEach(() => {
    view.destroy();
    document.body.innerHTML = '';
  });

  test('adds ## to a plain text line', () => {
    ({ view, container } = makeView('Title'));
    view.dispatch({ selection: { anchor: 2 } });
    toggleHeading(view);
    expect(view.state.doc.toString()).toBe('## Title');
  });

  test('removes ## from a line that is already an H2 (toggle off)', () => {
    ({ view, container } = makeView('## Title'));
    view.dispatch({ selection: { anchor: 5 } });
    toggleHeading(view);
    expect(view.state.doc.toString()).toBe('Title');
  });

  test('replaces an existing H1 prefix instead of stacking ##', () => {
    ({ view, container } = makeView('# Title'));
    view.dispatch({ selection: { anchor: 4 } });
    toggleHeading(view);
    expect(view.state.doc.toString()).toBe('## Title');
  });

  test('replaces an existing H3 prefix instead of stacking ##', () => {
    ({ view, container } = makeView('### Title'));
    view.dispatch({ selection: { anchor: 4 } });
    toggleHeading(view);
    expect(view.state.doc.toString()).toBe('## Title');
  });

  test('only touches the caret line in a multi-line document', () => {
    ({ view, container } = makeView('First\nSecond\nThird'));
    view.dispatch({ selection: { anchor: 8 } }); // inside "Second"
    toggleHeading(view);
    expect(view.state.doc.toString()).toBe('First\n## Second\nThird');
  });

  test('keeps the caret at an equivalent offset in the heading text', () => {
    ({ view, container } = makeView('Title'));
    view.dispatch({ selection: { anchor: 5 } }); // end of "Title"
    toggleHeading(view);
    expect(view.state.doc.toString()).toBe('## Title');
    expect(view.state.selection.main.head).toBe(8); // end of "## Title"
  });
});

describe('toggleBlockquote', () => {
  let view: EditorView;
  let container: HTMLElement;

  afterEach(() => {
    view.destroy();
    document.body.innerHTML = '';
  });

  test('adds "> " to a plain text line', () => {
    ({ view, container } = makeView('Title'));
    view.dispatch({ selection: { anchor: 2 } });
    toggleBlockquote(view);
    expect(view.state.doc.toString()).toBe('> Title');
  });

  test('removes "> " from an already-quoted line (toggle off)', () => {
    ({ view, container } = makeView('> Title'));
    view.dispatch({ selection: { anchor: 4 } });
    toggleBlockquote(view);
    expect(view.state.doc.toString()).toBe('Title');
  });

  test('removes a bare ">" with no trailing space', () => {
    ({ view, container } = makeView('>Title'));
    view.dispatch({ selection: { anchor: 3 } });
    toggleBlockquote(view);
    expect(view.state.doc.toString()).toBe('Title');
  });

  test('only touches the caret line in a multi-line document', () => {
    ({ view, container } = makeView('First\nSecond\nThird'));
    view.dispatch({ selection: { anchor: 8 } }); // inside "Second"
    toggleBlockquote(view);
    expect(view.state.doc.toString()).toBe('First\n> Second\nThird');
  });
});

// ── Line type: the margin indicator (heading `##`, quote `>`) ─────────────────

let indicatorView: EditorView | null = null;
afterEach(() => {
  indicatorView?.destroy();
  indicatorView = null;
  document.querySelectorAll('[role="menu"]').forEach((m) => m.remove());
});

// jsdom implements no layout; the indicator views are attached to the
// document, so CM6's deferred measure pass calls Range.getClientRects after a
// test and needs a harmless answer, not a throw.
beforeAll(() => {
  const emptyRect = {
    x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0,
    toJSON: () => ({}),
  } as DOMRect;
  Range.prototype.getClientRects = () =>
    Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => emptyRect;
});

function indicatorViewFor(doc: string, anchor = doc.length): EditorView {
  indicatorView = new EditorView({
    state: EditorState.create({
      doc,
      selection: { anchor },
      extensions: [markdown({ extensions: [GFM] }), history(), markdownStyleExtension(), lineIndicatorExtension()],
    }),
    parent: document.body,
  });
  return indicatorView;
}
const backspace = (v: EditorView, shiftKey = false) => runScopeHandlers(v, new KeyboardEvent('keydown', { key: 'Backspace', shiftKey }), 'editor');

describe('Backspace at the start of a heading\'s or quote\'s text removes one level', () => {
  test('### -> ## -> # -> paragraph, the caret staying at the start of the text', () => {
    const v = indicatorViewFor('### Title', 4);
    for (const [doc, head] of [['## Title', 3], ['# Title', 2], ['Title', 0]] as const) {
      expect(backspace(v)).toBe(true);
      expect(v.state.doc.toString()).toBe(doc);
      expect(v.state.selection.main.head).toBe(head);
    }
  });

  test('a quote loses one "> " per press: nested to single to paragraph', () => {
    const v = indicatorViewFor('> > deep', 4);
    backspace(v);
    expect(v.state.doc.toString()).toBe('> deep');
    backspace(v);
    expect(v.state.doc.toString()).toBe('deep');
  });

  test('an empty heading and an empty quote are removed too', () => {
    const v = indicatorViewFor('#', 1);
    backspace(v);
    expect(v.state.doc.toString()).toBe('');
    v.dispatch({ changes: { from: 0, insert: '> ' }, selection: { anchor: 2 } });
    backspace(v);
    expect(v.state.doc.toString()).toBe('');
  });

  test('one undo restores exactly the level that was removed, even after quick presses', () => {
    const v = indicatorViewFor('### Title', 4);
    backspace(v);
    backspace(v);
    undo(v);
    expect(v.state.doc.toString()).toBe('## Title');
    undo(v);
    expect(v.state.doc.toString()).toBe('### Title');
  });

  test('Shift-Backspace behaves the same', () => {
    const v = indicatorViewFor('## Title', 3);
    backspace(v, true);
    expect(v.state.doc.toString()).toBe('# Title');
  });

  test('anywhere else, and with a selection, Backspace is left to CM6', () => {
    const v = indicatorViewFor('## Title', 5);
    expect(backspace(v)).toBe(false);
    v.dispatch({ selection: EditorSelection.range(3, 5) });
    expect(backspace(v)).toBe(false);
    const plain = indicatorViewFor('plain', 0);
    expect(backspace(plain)).toBe(false);
  });
});

describe('Mod-Backspace at a text start', () => {
  const key = (v: EditorView, init: KeyboardEventInit) => runScopeHandlers(v, new KeyboardEvent('keydown', init), 'editor');
  const mac = /Mac|iPhone|iPad/.test(navigator.platform); // false under jsdom: Mod is Ctrl

  test('Mod-Backspace at a text start removes every indicator at once: the line becomes a paragraph', () => {
    for (const [doc, head, want] of [['### Title', 4, 'Title'], ['> > deep', 4, 'deep'], ['> ## x', 5, 'x'], ['a\n## b', 5, 'a\nb']] as const) {
      const v = indicatorViewFor(doc, head);
      expect(key(v, mac ? { key: 'Backspace', metaKey: true } : { key: 'Backspace', ctrlKey: true })).toBe(true);
      expect(v.state.doc.toString()).toBe(want);
      v.destroy();
    }
  });

  test('Mod-Backspace anywhere else is CM6\'s own', () => {
    const v = indicatorViewFor('## Title', 6);
    expect(key(v, mac ? { key: 'Backspace', metaKey: true } : { key: 'Backspace', ctrlKey: true })).toBe(false);
  });
});

describe('setLineType rewrites the prefix in one transaction', () => {
  // The menu's way in: `target` is the indicator that was clicked.
  test.each([
    ['## Title', { kind: 'heading', level: 1 }, '# Title'],
    ['# Title', { kind: 'heading', level: 3 }, '### Title'],
    ['## Title', { kind: 'paragraph' }, 'Title'],
    ['## Title', { kind: 'quote' }, '> Title'],
    ['> > Title', { kind: 'heading', level: 2 }, '## Title'],
    ['> > Title', { kind: 'quote' }, '> Title'],
    ['> Title', { kind: 'paragraph' }, 'Title'],
  ] as const)('%s -> %j', (doc, type, want) => {
    const v = indicatorViewFor(doc, doc.length);
    let transactions = 0;
    const plain = v.dispatch.bind(v);
    v.dispatch = ((...a: Parameters<EditorView['dispatch']>) => { transactions++; return plain(...a); }) as EditorView['dispatch'];
    expect(setLineType(v, 0, type, hangsOnLine(v.state, 0)[0])).toBe(true);
    expect(v.state.doc.toString()).toBe(want);
    expect(v.state.selection.main.head, 'the caret stays at the end of the text').toBe(want.length);
    expect(transactions).toBe(1);
  });

  test('choosing the type the line already has changes nothing', () => {
    const v = indicatorViewFor('## Title');
    expect(setLineType(v, 0, { kind: 'heading', level: 2 })).toBe(false);
    expect(v.state.doc.toString()).toBe('## Title');
  });

  test('only the named line changes', () => {
    const v = indicatorViewFor('# One\n## Two\n### Three', 0);
    setLineType(v, 6, { kind: 'paragraph' });
    expect(v.state.doc.toString()).toBe('# One\nTwo\n### Three');
  });

  test('a caret in the text keeps its place in the text', () => {
    const v = indicatorViewFor('# Title', 4); // between "Ti" and "tle"
    setLineType(v, 0, { kind: 'heading', level: 3 });
    expect(v.state.doc.toString()).toBe('### Title');
    expect(v.state.selection.main.head).toBe(6);
  });
});

describe('a heading inside a quote has two indicators, and each toggle and menu pick acts on one', () => {
  const lineOf = (doc: string, run: (v: EditorView) => void, anchor = doc.length) => {
    const v = indicatorViewFor(doc, anchor);
    run(v);
    return v.state.doc.toString();
  };

  test('⌘⌥2 sets the level inside the quote and keeps the "> "', () => {
    expect(lineOf('> # x', (v) => toggleHeading(v, 2))).toBe('> ## x');
    expect(lineOf('> x', (v) => toggleHeading(v, 2))).toBe('> ## x');
  });

  test('the same level again clears the heading and keeps the quote', () => {
    expect(lineOf('> ## x', (v) => toggleHeading(v, 2))).toBe('> x');
  });

  test('⌘⇧9 quotes a heading without touching it, and un-quotes a quoted heading without touching it', () => {
    expect(lineOf('## x', (v) => toggleBlockquote(v))).toBe('> ## x');
    expect(lineOf('> ## x', (v) => toggleBlockquote(v))).toBe('## x');
  });

  test('the menu acts on the indicator that was clicked', () => {
    const pick = (doc: string, clicked: 'quote' | 'heading', type: Parameters<typeof setLineType>[2]) =>
      lineOf(doc, (v) => setLineType(v, 0, type, hangsOnLine(v.state, 0).find((h) => h.kind === clicked)));
    expect(pick('> ## x', 'quote', { kind: 'paragraph' })).toBe('## x');
    expect(pick('> ## x', 'heading', { kind: 'paragraph' })).toBe('> x');
    expect(pick('> ## x', 'heading', { kind: 'heading', level: 3 })).toBe('> ### x');
    // a different kind replaces the clicked indicator, and the line's other one of that kind goes
    expect(pick('> ## x', 'quote', { kind: 'heading', level: 1 })).toBe('# x');
    expect(pick('> ## x', 'heading', { kind: 'quote' })).toBe('> x');
  });

  test('a click on the quote and a click on the heading open the menu on the right one', () => {
    const v = indicatorViewFor('> ## x', 6);
    const [quote, heading] = Array.from(v.dom.querySelectorAll<HTMLElement>('.cm-hang[data-indicator]'));
    quote.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 }));
    document.querySelector<HTMLElement>('[role="menu"] [data-action="paragraph"]')!.click();
    expect(v.state.doc.toString()).toBe('## x');
    expect(heading).toBeDefined();
  });
});

describe('the indicator is a control', () => {
  const press = (el: Element) => el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 }));

  test('the hung marker carries its line type as a tooltip and a data attribute', () => {
    const v = indicatorViewFor('## Title\n\n> quoted');
    const [h, q] = Array.from(v.dom.querySelectorAll<HTMLElement>('.cm-hang[data-indicator]'));
    expect([h.dataset.indicator, h.dataset.tooltip]).toEqual(['heading', 'Heading 2']);
    expect([q.dataset.indicator, q.dataset.tooltip]).toEqual(['quote', 'Quote']);
    expect(h.hasAttribute('title')).toBe(false);
  });

  test('a press on it opens the line-type menu, puts the caret at the text start, and a pick rewrites the line', () => {
    const v = indicatorViewFor('## Title', 8);
    const hang = v.dom.querySelector('.cm-hang[data-indicator]')!;
    expect(press(hang)).toBe(false); // preventDefault: CM6 never places a caret in the margin
    expect(v.state.selection.main.head).toBe(3);
    const items = Array.from(document.querySelectorAll<HTMLElement>('[role="menu"] [data-action]'));
    expect(items.map((i) => i.dataset.action)).toEqual(['heading-1', 'heading-2', 'heading-3', 'paragraph', 'quote']);
    expect(items.map((i) => i.textContent)).toEqual(['Heading 1', 'Heading 2', 'Heading 3', 'Paragraph', 'Quote']);
    items.find((i) => i.dataset.action === 'quote')!.click();
    expect(v.state.doc.toString()).toBe('> Title');
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  test('a read-only editor opens no menu', () => {
    const v = new EditorView({
      state: EditorState.create({
        doc: '## Title',
        extensions: [markdown({ extensions: [GFM] }), markdownStyleExtension(), lineIndicatorExtension(), EditorState.readOnly.of(true)],
      }),
      parent: document.body,
    });
    indicatorView = v;
    press(v.dom.querySelector('.cm-hang')!);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  test('a press elsewhere in the line is not taken', () => {
    const v = indicatorViewFor('## Title');
    press(v.dom.querySelector('.cm-line')!);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
});
