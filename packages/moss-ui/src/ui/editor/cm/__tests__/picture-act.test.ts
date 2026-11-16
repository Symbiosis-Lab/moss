import { afterEach, describe, expect, test, vi } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { forceParsing } from '@codemirror/language';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { focusPointExtension } from '../focus-point';
import { pictureActExtension } from '../picture-act';
import { blockEmbedsExtension } from '../../cm-block-embeds';
import { pictureAtCaretField } from '../picture-context';
import { MARKDOWN_EXTENSIONS } from '../source-extensions';
import { HERO_FRAME } from '../../session/instrument-events';
import { EDITOR_PICTURE_ACT, type PictureAct } from '../../session/editor-position';

const handlers = new Map<string, (payload: unknown) => void>();
const host = {
  emit: () => {},
  on: <T,>(name: string, handler: (payload: T) => void) => {
    handlers.set(name, (p) => handler(p as T));
    return () => { handlers.delete(name); };
  },
};
const IMAGE = { url: '/photo.jpg', width: 2000, height: 1000 };

let view: EditorView | null = null;
afterEach(() => { view?.destroy(); view = null; document.body.innerHTML = ''; handlers.clear(); });

function open(doc: string) {
  const replace = vi.fn(async () => {});
  const reveal = vi.fn();
  const resolveImage = () => IMAGE;
  view = new EditorView({
    state: EditorState.create({ doc, extensions: [markdown({ extensions: MARKDOWN_EXTENSIONS }), pictureAtCaretField,
      focusPointExtension({ path: () => 'p.md', resolveImage, host }), blockEmbedsExtension({ resolveImage }),
      pictureActExtension({ host, replace, reveal })] }),
    parent: document.body,
  });
  forceParsing(view, doc.length, 10_000);
  /** A menu row: the caret goes to the line, then the act follows. */
  const row = (line: number, act: PictureAct, picture = 0) => {
    view!.dispatch({ selection: { anchor: view!.state.doc.line(line).from } });
    handlers.get(EDITOR_PICTURE_ACT)!({ line, act, picture });
  };
  const selected = () => view!.state.sliceDoc(view!.state.selection.main.from, view!.state.selection.main.to);
  return { view, replace, reveal, row, selected };
}

const settle = async () => {
  await new Promise((r) => setTimeout(r, 0));
  for (let i = 0; i < 2; i++) await new Promise<void>((r) => requestAnimationFrame(() => r()));
};

describe('a picture row in the editor', () => {
  test('acts on the picture clicked of two on a line, and on none when the line no longer has it', () => {
    const { replace, reveal, row } = open('Intro\n\n![First](a.png) ![Second](b.png)\n\nWords.');
    row(3, 'replace', 1);
    expect(replace).toHaveBeenCalledTimes(1);
    expect((replace.mock.calls[0] as unknown[])[1]).toMatchObject({ target: 'b.png' });
    row(3, 'reveal', 0);
    expect(reveal).toHaveBeenLastCalledWith('a.png');
    row(3, 'replace', 2);
    row(5, 'reveal', 0);
    expect(replace).toHaveBeenCalledTimes(1);
    expect(reveal).toHaveBeenCalledTimes(1);
  });

  test("Edit description selects the words a picture is described by, or puts the caret where they go", () => {
    const { row, selected, view } = open('![A room](a.png)\n\n![Dusk|](b.png)\n\n![[c.png|Dawn|cover top]]\n\n![](d.png)\n\n![[e.png]]');
    row(1, 'description');
    expect(selected()).toBe('A room');
    row(3, 'description');
    expect(selected()).toBe('Dusk');
    row(5, 'description');
    expect(selected()).toBe('Dawn');
    row(7, 'description');
    expect(view.state.selection.main.head).toBe(view.state.doc.line(7).from + 2);
    row(9, 'description');
    expect(view.state.doc.line(9).text).toBe('![[e.png|]]');
    expect(view.state.selection.main.head).toBe(view.state.doc.line(9).to - 2);
  });

  test.each([
    ['![[e.png]]', '![[e.png|Dawn]]'],
    ['![[c.png|40%]]', '![[c.png|Dawn|40%]]'],
    ['![[c.png|cover top]]', '![[c.png|Dawn|cover top]]'],
    ['![[c.png|wide|Old]]', '![[c.png|wide|Dawn]]'],
    ['![Dusk|40%](b.png)', '![Dawn|40%](b.png)'],
    ['![40%](b.png)', '![Dawn|40%](b.png)'],
  ])('typing after Edit description on %s keeps the target and the words moss reads', (written, typed) => {
    const { row, view } = open(written);
    row(1, 'description');
    const { from, to } = view.state.selection.main;
    view.dispatch({ changes: { from, to, insert: 'Dawn' } });
    expect(view.state.doc.toString()).toBe(typed);
  });

  test("Set focus point puts the keyboard on the cropped picture's dot once it is drawn", async () => {
    const { row } = open('Intro\n\n![A room|](a.png)\n\nWords.');
    row(3, 'focus');
    handlers.get(HERO_FRAME)!({ line: 3, width: 500, height: 500 });
    await settle();
    expect(document.activeElement).toBe(document.querySelector('.cm-focus-point-dot'));
  });

  test('asked for, the dot is drawn where this width shows the picture whole', async () => {
    const { row, view } = open('Intro\n\n![A room|](a.png)\n\nWords.');
    view.dispatch({ selection: { anchor: view.state.doc.line(3).from } });
    handlers.get(HERO_FRAME)!({ line: 3, width: 1000, height: 500 });
    await settle();
    expect(document.querySelector('.cm-focus-point-dot')?.isConnected ?? false).toBe(false);
    row(3, 'focus');
    await settle();
    expect(document.activeElement).toBe(document.querySelector('.cm-focus-point-dot'));
  });
});
