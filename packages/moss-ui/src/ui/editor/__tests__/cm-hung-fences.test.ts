import { describe, test, expect, afterEach, beforeAll } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { EditorView, runScopeHandlers } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { history, undo } from '@codemirror/commands';
import { ensureSyntaxTree } from '@codemirror/language';
import { collectShortcodeBlocks, shortcodeFences } from '@symbiosis-lab/moss-syntax/cm6';
import { MARKDOWN_EXTENSIONS } from '../cm-editor';
import { formatKeysExtension, lineIndicatorExtension, toggleHeading } from '../cm-format-keys';
import { markdownStyleExtension } from '../cm-markdown-style';
import { hangsOnLine } from '../cm-hangs';

beforeAll(() => {
  const emptyRect = { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) } as DOMRect;
  Range.prototype.getClientRects = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => emptyRect;
});

let view: EditorView;
afterEach(() => { view?.destroy(); document.body.innerHTML = ''; });

/** `¦` marks the caret. */
function mount(src: string, parsed = true): void {
  const doc = src.replace('¦', '');
  const parent = document.createElement('div');
  document.body.appendChild(parent);
  view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      selection: { anchor: src.includes('¦') ? src.indexOf('¦') : 0 },
      extensions: [markdown({ extensions: MARKDOWN_EXTENSIONS }), history(), formatKeysExtension(), lineIndicatorExtension(), markdownStyleExtension()],
    }),
  });
  if (parsed) ensureSyntaxTree(view.state, doc.length, 1000);
}
const key = (k: string, mods: { altKey?: boolean; ctrlKey?: boolean } = {}): boolean =>
  runScopeHandlers(view, new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...mods }), 'editor');
const doc = (): string => view.state.doc.toString();
const hangsAt = (needle: string) => hangsOnLine(view.state, doc().indexOf(needle)).map((h) => [h.kind, h.level, doc().slice(h.from, h.to)]);

describe('pairing, as the build pairs it', () => {
  test.each([
    ['a paired block', ':::grid\nx\n:::', 1],
    ['nested blocks', ':::grid\n::::buttons\nx\n::::\n:::', 1],
    ['a closer of another colon count closes the innermost block', ':::a\n::::b\nx\n:::\n::::', 1],
    ['an unpaired opening fence', ':::grid\nx', 0],
    ['an unpaired closing fence', 'x\n:::', 0],
  ])('%s', (_n, src, tops) => {
    mount(src);
    expect(collectShortcodeBlocks(view.state)).toHaveLength(tops);
    expect(shortcodeFences(view.state)).toHaveLength(tops ? src.split('\n').filter((l) => /^:+/.test(l)).length : 0);
  });
});

describe('what hangs', () => {
  test('the colon run of an opening fence, at its nesting depth; the name stays in the text', () => {
    mount(':::grid\n::::buttons {x=1}\nx\n::::\n:::');
    expect(hangsAt(':::grid')).toEqual([['fence-open', 1, ':::']]);
    expect(hangsAt('::::buttons')).toEqual([['fence-open', 2, '::::']]);
  });
  test('a closing fence hangs whole', () => {
    mount(':::grid\n::::buttons\nx\n::::\n:::');
    expect(hangsOnLine(view.state, doc().indexOf('::::\n') ).map((h) => [h.kind, h.level, h.to - h.from])).toEqual([['fence-close', 2, 4]]);
    expect(hangsOnLine(view.state, doc().length).map((h) => [h.kind, h.level])).toEqual([['fence-close', 1]]);
  });
  test('an unpaired fence stays in the text column', () => {
    mount('para\n\n:::grid\n\nmore');
    expect(hangsAt(':::grid')).toEqual([]);
  });
  test('a heading still hangs beside a block', () => {
    mount('# h\n\n:::grid\nx\n:::');
    expect(hangsAt('# h')).toEqual([['heading', 1, '# ']]);
  });
});

describe('Backspace unwraps a block', () => {
  test('at the start of the opening fence text: both fences go, the body stays, one undo restores', () => {
    mount('a\n\n:::¦grid\nx\ny\n:::\n\nb');
    expect(key('Backspace')).toBe(true);
    expect(doc()).toBe('a\n\nx\ny\n\nb');
    undo(view);
    expect(doc()).toBe('a\n\n:::grid\nx\ny\n:::\n\nb');
  });
  test('on the closing fence', () => {
    mount('a\n\n:::grid\nx\n:::¦');
    expect(key('Backspace')).toBe(true);
    expect(doc()).toBe('a\n\nx');
    undo(view);
    expect(doc()).toBe('a\n\n:::grid\nx\n:::');
  });
  test('an empty block', () => {
    mount(':::¦grid\n:::');
    key('Backspace');
    expect(doc()).toBe('');
  });
  test('removes the block\'s own dividers and keeps a nested block whole', () => {
    mount(':::¦grid\na\n+++\n::::buttons\n+++\nb\n::::\n+++\nc\n:::');
    key('Backspace');
    expect(doc()).toBe('a\n::::buttons\n+++\nb\n::::\nc');
  });
  test('the old --- divider of a grid goes too', () => {
    mount(':::¦grid\na\n---\nb\n:::');
    key('Backspace');
    expect(doc()).toBe('a\nb');
  });
  test('an unpaired fence is not a hang: Backspace is left to CM6', () => {
    mount(':::¦grid\nx');
    expect(key('Backspace')).toBe(false);
  });
});

describe('a fence has no line type', () => {
  test('the line-type keys change nothing and still take the key', () => {
    mount(':::¦grid\nx\n:::');
    for (const level of [1, 2, 3]) expect(toggleHeading(view, level)).toBe(true);
    expect(key('1', { altKey: true, ctrlKey: true })).toBe(true);
    expect(doc()).toBe(':::grid\nx\n:::');
  });
  test('a fence marker carries no menu target', () => {
    mount(':::grid\nx\n:::');
    expect(view.dom.querySelector('.cm-hang')).not.toBeNull();
    expect(view.dom.querySelector('.cm-hang[data-indicator]')).toBeNull();
  });
});

describe('the rail', () => {
  test('every line of a block draws as many rails as blocks enclose it', () => {
    mount('p\n:::grid\na\n::::buttons\nb\n::::\nc\n:::\nq');
    const depths = [...view.dom.querySelectorAll('.cm-line')].map((l) => l.classList.contains('cm-sc-rail') ? (l as HTMLElement).style.getPropertyValue('--sc-depth') : '');
    expect(depths).toEqual(['', '1', '1', '2', '2', '2', '1', '1', '']);
  });
});

describe('Alt-arrow still moves a block whole, from a hung fence', () => {
  test('Alt-Up on the opening fence', () => {
    mount('p\n\n:::¦grid\nx\n:::');
    expect(key('ArrowUp', { altKey: true })).toBe(true);
    expect(doc()).toBe(':::grid\nx\n:::\n\np');
  });
  test('a block whose partner is in a list is not moved as a block', () => {
    const src = '- :::grid\n\nx\n\n:::';
    mount(src);
    key('ArrowDown', { altKey: true });
    expect(doc()).toBe(src);
  });
});

describe('a block whose close the parse has not reached', () => {
  const body = Array.from({ length: 3000 }, (_, i) => `line ${i}`).join('\n');
  const src = `a\n\n:::¦grid\nx\n${body}\n:::\n\nb`;

  test('the field has not paired it yet, so the fence hangs on its own', () => {
    mount(src, false);
    expect(collectShortcodeBlocks(view.state)).toHaveLength(0);
    expect(hangsAt(':::grid')).toEqual([['fence-open', 1, ':::']]);
  });
  test('Backspace still unwraps the whole block', () => {
    mount(src, false);
    expect(key('Backspace')).toBe(true);
    expect(doc()).toBe(`a\n\nx\n${body}\n\nb`);
  });
  test('Alt-Up still moves the whole block', () => {
    mount(src, false);
    expect(key('ArrowUp', { altKey: true })).toBe(true);
    expect(doc()).toBe(`:::grid\nx\n${body}\n:::\n\na\n\nb`);
  });
});
