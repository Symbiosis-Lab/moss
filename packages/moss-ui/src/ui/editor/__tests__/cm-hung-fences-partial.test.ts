import { describe, test, expect, afterEach, beforeAll, vi } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { EditorView, runScopeHandlers } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { history } from '@codemirror/commands';
import { MARKDOWN_EXTENSIONS } from '../cm-editor';
import { formatKeysExtension, lineIndicatorExtension } from '../cm-format-keys';
import { markdownStyleExtension } from '../cm-markdown-style';
import { hangsOnLine } from '../cm-hangs';

// The parse never finishes within its budget: ensureSyntaxTree gives up and the
// editor is left with the partial tree that syntaxTree(state) returns.
vi.mock('@codemirror/language', async () => ({
  ...(await vi.importActual<typeof import('@codemirror/language')>('@codemirror/language')),
  ensureSyntaxTree: () => null,
}));

beforeAll(() => {
  const emptyRect = { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) } as DOMRect;
  Range.prototype.getClientRects = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => emptyRect;
});

let view: EditorView;
afterEach(() => { view?.destroy(); document.body.innerHTML = ''; });

/** `¦` marks the caret. */
function mount(src: string): void {
  const doc = src.replace('¦', '');
  const parent = document.createElement('div');
  document.body.appendChild(parent);
  view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      selection: { anchor: src.indexOf('¦') },
      extensions: [markdown({ extensions: MARKDOWN_EXTENSIONS }), history(), formatKeysExtension(), lineIndicatorExtension(), markdownStyleExtension()],
    }),
  });
}
const key = (k: string, mods: { altKey?: boolean } = {}): boolean =>
  runScopeHandlers(view, new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...mods }), 'editor');
const doc = (): string => view.state.doc.toString();

describe('a hung fence the partial parse has not paired', () => {
  const body = Array.from({ length: 3000 }, (_, i) => `line ${i}`).join('\n');
  const src = `a\n\n:::¦grid\nx\n${body}\n:::\n\nb`;

  test('the opening fence hangs, and no pair is known', () => {
    mount(src);
    expect(hangsOnLine(view.state, view.state.selection.main.head).map((h) => h.kind)).toEqual(['fence-open']);
  });
  test('Backspace at the text start leaves the document unchanged', () => {
    mount(src);
    expect(key('Backspace')).toBe(true);
    expect(doc()).toBe(src.replace('¦', ''));
  });
  test('Alt-Up leaves the document unchanged', () => {
    mount(src);
    expect(key('ArrowUp', { altKey: true })).toBe(true);
    expect(doc()).toBe(src.replace('¦', ''));
  });
});
