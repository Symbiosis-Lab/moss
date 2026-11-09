import { describe, test, expect, afterEach, beforeAll } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { EditorView, runScopeHandlers } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { history, undo } from '@codemirror/commands';
import { ensureSyntaxTree } from '@codemirror/language';
import { MARKDOWN_EXTENSIONS } from '../cm-editor';
import { formatKeysExtension, blockUnitAt } from '../cm-format-keys';

// jsdom implements no layout; CM6's deferred measure pass calls Range.getClientRects after a test and needs a harmless answer, not a throw.
beforeAll(() => {
  const emptyRect = { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) } as DOMRect;
  Range.prototype.getClientRects = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => emptyRect;
});

let view: EditorView;
afterEach(() => { view?.destroy(); document.body.innerHTML = ''; });

/** `|` marks the caret; `«` `»` mark a selection. */
function mount(src: string): void {
  const a = src.indexOf('«'), b = src.indexOf('»');
  const sel = a >= 0 ? { anchor: a, head: b - 1 } : { anchor: src.indexOf('¦') };
  const doc = src.replace(/[«»¦]/g, '');
  const parent = document.createElement('div');
  document.body.appendChild(parent);
  view = new EditorView({
    parent,
    state: EditorState.create({ doc, selection: sel, extensions: [markdown({ extensions: MARKDOWN_EXTENSIONS }), history(), formatKeysExtension()] }),
  });
  ensureSyntaxTree(view.state, doc.length, 1000);
}
const press = (key: 'ArrowUp' | 'ArrowDown'): boolean =>
  runScopeHandlers(view, new KeyboardEvent('keydown', { key, altKey: true, bubbles: true, cancelable: true }), 'editor');
const doc = (): string => view.state.doc.toString();
/** The document with the caret drawn back in as `|`. */
const withCaret = (): string => { const h = view.state.selection.main.head; return doc().slice(0, h) + '¦' + doc().slice(h); };
/** The unit under the caret, as text. */
const unit = (): string | null => {
  const u = blockUnitAt(view.state, view.state.selection.main.head);
  return u && doc().slice(u.from, u.to);
};

describe('blockUnitAt: the innermost block, as whole lines', () => {
  test.each([
    ['nested list item, with its children', '- a\n- b¦\n  - c\n    - d\n- e', '- b\n  - c\n    - d'],
    ['a child is its own unit', '- a\n- b\n  - c¦\n    - d\n- e', '  - c\n    - d'],
    ['wrapped and lazy lines belong to the item', '- a¦\n  wrapped\nlazy\n- b', '- a\n  wrapped\nlazy'],
    ['a caret on a continuation line', '- a\n  wrap¦ped\n- b', '- a\n  wrapped'],
    ['a fence, fence to fence', 'intro\n\n```js\nlet a¦;\n\nlet b;\n```\n\nouter', '```js\nlet a;\n\nlet b;\n```'],
    ['a $$ block', 'p\n\n$$\nx¦^2\n$$\n\nq', '$$\nx^2\n$$'],
    ['a quote, all its > lines', 'p\n\n> one\n> tw¦o\n\nq', '> one\n> two'],
    ['a callout', 'p\n\n> [!note]\n> bo¦dy\n\nq', '> [!note]\n> body'],
    ['a table, all its rows', 'p\n\n| a | b |\n|---|---|\n| 1 | 2¦ |\n\nq', '| a | b |\n|---|---|\n| 1 | 2 |'],
    ['a paragraph, its lines', 'one\n\ntwo\nthr¦ee\n\nfour', 'two\nthree'],
    ['a heading and its section, sub-headings included', '# A\n\ntext\n\n## A1¦\n\nsub\n\n# B', '## A1\n\nsub'],
    ['a top heading takes its sub-sections', '# A¦\n\ntext\n\n## A1\n\nsub\n\n# B\n\nmore', '# A\n\ntext\n\n## A1\n\nsub'],
    ['a setext heading', 'Title¦\n=====\n\nbody\n\nNext\n=====', 'Title\n=====\n\nbody'],
    ['a blank line is nobody\'s', 'a\n¦\nb', null],
  ])('%s', (_name, src, want) => {
    mount(src);
    expect(unit()).toBe(want);
  });

  test('an item holding a fence is moved as the item', () => {
    mount('- a\n  ```\n  c¦ode\n  ```\n- b');
    expect(unit()).toBe('- a\n  ```\n  code\n  ```');
  });
});

describe('Alt-ArrowDown / Alt-ArrowUp', () => {
  test('a nested item moves with its children past its sibling, staying in its list and depth', () => {
    mount('- a\n  - b¦\n    - c\n  - d\n- e');
    expect(press('ArrowDown')).toBe(true);
    expect(doc()).toBe('- a\n  - d\n  - b\n    - c\n- e');
    expect(withCaret()).toBe('- a\n  - d\n  - b¦\n    - c\n- e');
    press('ArrowUp');
    expect(doc()).toBe('- a\n  - b\n    - c\n  - d\n- e');
  });

  test('the caret keeps its offset within the moved text', () => {
    mount('- one\n- t¦wo\n  - kid\n- three');
    press('ArrowUp');
    expect(withCaret()).toBe('- t¦wo\n  - kid\n- one\n- three');
  });

  test('a quote swaps with the next block, the blank line between them untouched', () => {
    mount('> q¦1\n> q2\n\npara');
    press('ArrowDown');
    expect(doc()).toBe('para\n\n> q1\n> q2');
    expect(withCaret()).toBe('para\n\n> q¦1\n> q2');
  });

  test('a fence swaps with the previous paragraph', () => {
    mount('para\n\n```\nco¦de\n```');
    press('ArrowUp');
    expect(doc()).toBe('```\ncode\n```\n\npara');
  });

  test('a table moves past a `$$` block', () => {
    mount('| a |\n|---|\n| 1¦ |\n\n$$\nx\n$$');
    press('ArrowDown');
    expect(doc()).toBe('$$\nx\n$$\n\n| a |\n|---|\n| 1 |');
  });

  test('a paragraph swaps with the adjacent block and keeps single-newline neighbours single', () => {
    mount('one¦\n\ntwo\nthree');
    press('ArrowDown');
    expect(doc()).toBe('two\nthree\n\none');
  });

  test('a heading section with its sub-heading swaps with the next section of the same level', () => {
    mount('# A¦\n\ntext\n\n## A1\n\nsub\n\n# B\n\nmore');
    press('ArrowDown');
    expect(doc()).toBe('# B\n\nmore\n\n# A\n\ntext\n\n## A1\n\nsub');
    expect(withCaret()).toBe('# B\n\nmore\n\n# A¦\n\ntext\n\n## A1\n\nsub');
  });

  test('a sub-heading swaps only within its parent section', () => {
    mount('# A\n\n## A1¦\n\nx\n\n## A2\n\ny\n\n# B');
    press('ArrowDown');
    expect(doc()).toBe('# A\n\n## A2\n\ny\n\n## A1\n\nx\n\n# B');
    // The last node under A: the next sibling would be in B's parent, so it stays.
    mount('# A\n\n## A1\n\nx\n\n## A2¦\n\ny\n\n# B');
    press('ArrowDown');
    expect(doc()).toBe('# A\n\n## A1\n\nx\n\n## A2\n\ny\n\n# B');
  });

  test('a block does not cross a heading', () => {
    mount('# A\n\np¦\n\n# B');
    press('ArrowDown');
    press('ArrowUp');
    expect(doc()).toBe('# A\n\np\n\n# B');
  });

  test('ordered lists keep their numbers', () => {
    mount('1. a¦\n2. b\n3. c');
    press('ArrowDown');
    expect(doc()).toBe('2. b\n1. a\n3. c');
  });

  test('a selection moves the union of the units its lines touch', () => {
    mount('- a\n- «b\n  - c\n- d»\n- e\n- f');
    press('ArrowDown');
    expect(doc()).toBe('- a\n- e\n- b\n  - c\n- d\n- f');
    expect(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to)).toBe('b\n  - c\n- d');
  });

  test('a child selected together with its parent moves once, with it', () => {
    mount('- a\n- «b\n  - c»\n- d');
    press('ArrowDown');
    expect(doc()).toBe('- a\n- d\n- b\n  - c');
  });

  test('a selection spanning two levels does nothing but still takes the key', () => {
    mount('- «a\n\npara»');
    expect(press('ArrowDown')).toBe(true);
    expect(doc()).toBe('- a\n\npara');
  });

  test.each([
    ['a loose list', '- a\n\n- b¦\n\n- c', 'ArrowUp', '- b\n\n- a\n\n- c'],
    ['a list in a quote', '> - a\n> - b¦', 'ArrowUp', '> - b\n> - a'],
    ['a list in a callout', '> [!note]\n> - a¦\n> - b', 'ArrowDown', '> [!note]\n> - b\n> - a'],
    ['the last heading section, no trailing newline', '# A\n\nx\n\n# B¦\n\ny', 'ArrowUp', '# B\n\ny\n\n# A\n\nx'],
    ['the first heading section down to the end of the file', '# A¦\n\nx\n\n# B\n\ny', 'ArrowDown', '# B\n\ny\n\n# A\n\nx'],
  ] as const)('%s', (_n, src, key, want) => {
    mount(src);
    press(key);
    expect(doc()).toBe(want);
  });

  test.each([
    ['the first item up', '- a¦\n- b', 'ArrowUp', '- a\n- b'],
    ['the last item down', '- a\n- b¦', 'ArrowDown', '- a\n- b'],
    ['the only block', 'solo¦', 'ArrowUp', 'solo'],
    ['the first top-level block up', 'one¦\n\ntwo', 'ArrowUp', 'one\n\ntwo'],
    ['a first heading section up', '# A¦\n\nx\n\n## B', 'ArrowUp', '# A\n\nx\n\n## B'],
    ['a blank line', 'a\n¦\nb', 'ArrowDown', 'a\n\nb'],
  ] as const)('at the boundary, %s does nothing but still takes the key', (_n, src, key, want) => {
    mount(src);
    expect(press(key)).toBe(true);
    expect(doc()).toBe(want);
  });

  test('one move is one undo step', () => {
    mount('- a¦\n- b\n- c');
    press('ArrowDown');
    expect(doc()).toBe('- b\n- a\n- c');
    undo(view);
    expect(doc()).toBe('- a\n- b\n- c');
  });
});
