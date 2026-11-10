import { afterEach, describe, expect, test, vi } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { history, undo } from '@codemirror/commands';
import { pictureAtCaretField } from '../picture-context';
import { heroWordsExtension } from '../hero-words';
import { MARKDOWN_EXTENSIONS } from '../source-extensions';
import { HERO_ALIGN, HERO_WORDS } from '../../session/instrument-events';

const wire = { sent: [] as [string, unknown][], handlers: new Map<string, (e: { payload: unknown }) => void>() };
const host = {
  emit: (name: string, payload: unknown) => { wire.sent.push([name, payload]); },
  on: <T,>(name: string, handler: (payload: T) => void) => {
    wire.handlers.set(name, (e) => handler(e.payload as T));
    return () => { wire.handlers.delete(name); };
  },
};

const DOC = ['intro', ':::hero {image=a.jpg}', 'Title', ':::', ':::grid', ':::'].join('\n');

let view: EditorView | null = null;
afterEach(() => { view?.destroy(); view = null; wire.sent.length = 0; wire.handlers.clear(); });

function open(caretLine: number) {
  view = new EditorView({
    state: EditorState.create({ doc: DOC, extensions: [markdown({ extensions: MARKDOWN_EXTENSIONS }), history(), pictureAtCaretField, heroWordsExtension(() => 'p.md', host)] }),
    parent: document.body,
  });
  const goto = (n: number) => view!.dispatch({ selection: { anchor: view!.state.doc.line(n).from } });
  goto(caretLine);
  const drop = (payload: unknown) => wire.handlers.get(HERO_ALIGN)!({ payload });
  return { view, sent: wire.sent, drop, goto };
}

describe('hero words instrument (editor half)', () => {
  test('tells the shell when the caret reaches a hero opening line, and when it leaves', () => {
    const { sent, goto } = open(1);
    expect(sent).toEqual([]);
    goto(2);
    goto(3);
    goto(5);
    expect(sent.map(([, p]) => p)).toEqual([{ path: 'p.md', line: 2 }, { path: 'p.md', line: null }]);
  });

  test('says nothing more while the caret stays on the line', () => {
    const { sent, view } = open(2);
    view.dispatch({ selection: { anchor: view.state.doc.line(2).from + 3 } });
    expect(sent).toHaveLength(1);
  });

  test('closing the editor takes the box away', () => {
    const { sent, view } = open(2);
    view.destroy();
    expect(sent.at(-1)).toEqual([HERO_WORDS, { path: 'p.md', line: null }]);
  });

  test('a drop rewrites only the align attribute, as one undoable edit', () => {
    const { view, drop } = open(2);
    drop({ line: 2, align: 'center' });
    expect(view.state.doc.line(2).text).toBe(':::hero {image=a.jpg align=center}');
    expect(view.state.doc.line(3).text).toBe('Title');
    drop({ line: 2, align: 'start' });
    expect(view.state.doc.line(2).text).toBe(':::hero {image=a.jpg}');
    undo(view);
    expect(view.state.doc.line(2).text).toBe(':::hero {image=a.jpg align=center}');
    undo(view);
    expect(view.state.doc.line(2).text).toBe(':::hero {image=a.jpg}');
  });

  test('an alignment the shell has no business sending is not written', () => {
    const { view, drop } = open(2);
    for (const align of ['x"}', 'end image=evil.jpg', undefined, 'END']) drop({ line: 2, align });
    expect(view.state.doc.toString()).toBe(DOC);
  });

  test('a page that opens on its hero draws no box until the caret moves', () => {
    const doc = [':::hero {image=a.jpg}', 'Title', ':::', 'body'].join('\n');
    view = new EditorView({
      state: EditorState.create({ doc, extensions: [markdown({ extensions: MARKDOWN_EXTENSIONS }), pictureAtCaretField, heroWordsExtension(() => 'p.md', host)] }),
      parent: document.body,
    });
    document.dispatchEvent(new Event('visibilitychange'));
    view.dispatch({ changes: { from: doc.length, insert: '\nmore' } });
    expect(wire.sent.filter(([, p]) => (p as { line: unknown }).line !== null)).toEqual([]);
    view.dispatch({ selection: { anchor: 3 } });
    expect(wire.sent.at(-1)).toEqual([HERO_WORDS, { path: 'p.md', line: 1 }]);
  });

  test('a drop for a line the caret is no longer on is ignored', () => {
    const { view, drop, goto } = open(2);
    goto(3);
    drop({ line: 2, align: 'end' });
    expect(view.state.doc.toString()).toBe(DOC);
  });
});
