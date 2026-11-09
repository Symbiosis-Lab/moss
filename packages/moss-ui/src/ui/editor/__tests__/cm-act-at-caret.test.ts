// packages/moss-ui/src/ui/editor/__tests__/cm-act-at-caret.test.ts
import { describe, test, expect, vi, beforeEach } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { TaskList } from '@lezer/markdown';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';
import { actAtCaretExtension, actAtCaret, modK, taskAt } from '../cm-act-at-caret';
import { markdownStyleExtension } from '../cm-markdown-style';
import { blockEmbedsExtension } from '../cm-block-embeds';
import { commentExtension, openCommentPopover } from '../cm-comment';

const openCardAtSpy = vi.fn().mockReturnValue(true);
vi.mock('../cm-reference-card', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../cm-reference-card')>()),
  openCardAt: (...args: unknown[]) => openCardAtSpy(...args),
}));

describe('click on a rendered block selects its source line', () => {
  test('a click on the picture below an image line selects that line, shown as text', () => {
    const doc = 'before\n![[a.png]]\nafter';
    const state = EditorState.create({
      doc,
      extensions: [markdown({ extensions: [wikilinkConfig] }), blockEmbedsExtension({ resolveImage: () => ({ url: 'x' }) }), actAtCaretExtension()],
    });
    const view = new EditorView({ state, parent: document.body });
    const row = view.dom.querySelector('.cm-lp-image-row') as HTMLElement;
    expect(row).not.toBeNull();
    row.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
    expect(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to)).toBe('![[a.png]]');
    view.destroy();
  });
});

describe('Mod-Enter act-at-caret', () => {
  test('Mod-Enter on a comment pin opens the popover (dispatches openCommentPopover)', () => {
    const doc = 'before {>>note<<} after';
    const state = EditorState.create({
      doc, selection: { anchor: doc.indexOf('note') },
      extensions: [markdown(), commentExtension(), blockEmbedsExtension({}), actAtCaretExtension()],
    });
    const view = new EditorView({ state });
    const dispatchSpy = vi.spyOn(view, 'dispatch');
    expect(actAtCaret(view)).toBe(true);
    const dispatched = dispatchSpy.mock.calls.some(([spec]) => {
      const effects = ([] as unknown[]).concat((spec as { effects?: unknown }).effects ?? []);
      return effects.some((e) => (e as { is: (t: unknown) => boolean }).is(openCommentPopover));
    });
    expect(dispatched).toBe(true);
    view.destroy();
  });
});

describe('Mod-k opens the link card at the caret', () => {
  beforeEach(() => { openCardAtSpy.mockClear(); });
  test.each([
    ['its label', 2],
    ['its brackets', 5],
    ['its URL', 10],
  ])('Mod-k with the caret in %s opens the link card', (_where, anchor) => {
    const state = EditorState.create({
      doc: '[text](https://x)', selection: { anchor },
      extensions: [markdown(), blockEmbedsExtension({}), actAtCaretExtension()],
    });
    const view = new EditorView({ state });
    expect(modK(view)).toBe(true);
    expect(openCardAtSpy).toHaveBeenCalledWith(view, 0); // the link's own start
    view.destroy();
  });
  test.each([
    ['an inline picture\'s brackets', 'see ![[a.png]] here', 5, 4],
    ['an inline picture\'s path', 'see ![[a.png]] here', 9, 4],
    ['a block picture line', '![a](x.png)', 8, 5],
  ])('Mod-k with the caret in %s opens the picture card', (_where, doc, anchor, probe) => {
    const state = EditorState.create({ doc, selection: { anchor }, extensions: [markdown({ extensions: [wikilinkConfig] }), actAtCaretExtension()] });
    const view = new EditorView({ state });
    expect(modK(view)).toBe(true);
    expect(openCardAtSpy).toHaveBeenCalledWith(view, probe);
    view.destroy();
  });
  test('Mod-k with a selection and no link falls through to formatMark', () => {
    const state = EditorState.create({
      doc: 'hello world', selection: { anchor: 0, head: 5 },
      extensions: [markdown(), blockEmbedsExtension({}), actAtCaretExtension()],
    });
    const view = new EditorView({ state });
    expect(modK(view)).toBe(false); // this module declines — cm-format-keys.ts's own Mod-k runs next
    expect(openCardAtSpy).not.toHaveBeenCalled();
    view.destroy();
  });
});

describe('a task toggles from its own characters', () => {
  const taskView = (doc: string, anchor: number) => new EditorView({
    state: EditorState.create({
      doc, selection: { anchor },
      extensions: [markdown({ extensions: [TaskList] }), markdownStyleExtension(), actAtCaretExtension()],
    }),
    parent: document.body,
  });
  const press = (el: Element, init: MouseEventInit = {}) => {
    const ev = new MouseEvent('mousedown', { bubbles: true, cancelable: true, ...init });
    el.dispatchEvent(ev);
    return ev;
  };

  test('taskAt finds a task from its marker, its text and either edge; prose is not a task', () => {
    const state = EditorState.create({ doc: '- [x] done\n\ntodo: [ ] buy', extensions: [markdown({ extensions: [TaskList] })] });
    for (const pos of [2, 3, 4, 5, 8, 10]) expect(taskAt(state, pos)).toEqual({ markerFrom: 2, checked: true });
    expect(taskAt(state, 14)).toBeNull();
    const noSpace = EditorState.create({ doc: '-[ ] not a task', extensions: [markdown({ extensions: [TaskList] })] });
    expect(taskAt(noSpace, 3)).toBeNull();
  });

  test('a press on [ ] toggles the item in one transaction and leaves the caret where it was', () => {
    const view = taskView('- [ ] todo', 8); // in "todo"
    const dispatch = vi.spyOn(view, 'dispatch');
    const box = view.dom.querySelector('.cm-task-toggle') as HTMLElement;
    expect(box.textContent).toBe('[ ]'); // the characters are on screen
    const ev = press(box);
    expect(view.state.doc.toString()).toBe('- [x] todo');
    expect(view.state.selection.main.head).toBe(8);
    expect(ev.defaultPrevented).toBe(true); // the browser does not also place a caret on it
    expect(dispatch).toHaveBeenCalledTimes(1);
    press(view.dom.querySelector('.cm-task-toggle')!);
    expect(view.state.doc.toString()).toBe('- [ ] todo');
    view.destroy();
  });

  test.each([2, 3, 4, 5, 6])('the caret keeps its place at %i, inside or beside the brackets', (anchor) => {
    const view = taskView('- [ ] todo', anchor);
    press(view.dom.querySelector('.cm-task-toggle')!);
    expect(view.state.doc.toString()).toBe('- [x] todo');
    expect(view.state.selection.main.head).toBe(anchor);
    view.destroy();
  });

  test('only the primary button toggles', () => {
    const view = taskView('- [ ] todo', 8);
    press(view.dom.querySelector('.cm-task-toggle')!, { button: 2 });
    expect(view.state.doc.toString()).toBe('- [ ] todo');
    view.destroy();
  });

  test('a read-only editor is not edited by a press on [ ] or by Mod-Enter', () => {
    const view = new EditorView({
      state: EditorState.create({
        doc: '- [ ] todo', selection: { anchor: 8 },
        extensions: [markdown({ extensions: [TaskList] }), markdownStyleExtension(), actAtCaretExtension(), EditorState.readOnly.of(true)],
      }),
      parent: document.body,
    });
    press(view.dom.querySelector('.cm-task-toggle')!);
    expect(actAtCaret(view)).toBe(false);
    expect(view.state.doc.toString()).toBe('- [ ] todo');
    view.destroy();
  });

  test('Mod-Enter at the caret toggles it too, caret unmoved', () => {
    const view = taskView('- [ ] todo', 8);
    expect(actAtCaret(view)).toBe(true);
    expect(view.state.doc.toString()).toBe('- [x] todo');
    expect(view.state.selection.main.head).toBe(8);
    view.destroy();
  });
});
