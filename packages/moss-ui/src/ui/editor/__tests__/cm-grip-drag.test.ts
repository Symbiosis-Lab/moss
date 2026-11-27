import { describe, test, expect, afterEach, beforeAll, vi } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { EditorView } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { history } from '@codemirror/commands';
import { forceParsing } from '@codemirror/language';
import { MARKDOWN_EXTENSIONS } from '../cm-editor';
import { formatKeysExtension, lineIndicatorExtension } from '../cm-format-keys';
import { markdownStyleExtension } from '../cm-markdown-style';

// jsdom paints nothing: every line is given a 10px row so gaps have places.
beforeAll(() => {
  const emptyRect = { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) } as DOMRect;
  Range.prototype.getClientRects = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => emptyRect;
});

let view: EditorView;
afterEach(() => { view?.destroy(); document.body.innerHTML = ''; vi.restoreAllMocks(); });

const SRC = ':::callout\nx\n:::\n\nb\n\nc';
const row = (pos: number) => view.state.doc.lineAt(pos).number * 10;

function mount(readOnly = false): void {
  const parent = document.createElement('div');
  document.body.appendChild(parent);
  view = new EditorView({
    parent,
    state: EditorState.create({
      doc: SRC,
      extensions: [markdown({ extensions: MARKDOWN_EXTENSIONS }), history(), formatKeysExtension(), lineIndicatorExtension(), markdownStyleExtension(), EditorState.readOnly.of(readOnly)],
    }),
  });
  forceParsing(view, SRC.length, 1e9);
  view.coordsAtPos = (pos) => ({ left: 0, right: 0, top: row(pos) - 10, bottom: row(pos) }) as DOMRect;
}

const pointer = (target: Element, type: string, y = 0) =>
  target.dispatchEvent(Object.assign(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: 0, clientY: y }), { isPrimary: true, pointerId: 1 }));
const grip = () => view.dom.querySelector('.cm-hang[data-grip]')!;
/** Press the first grip, travel past the threshold, then run `between` before releasing at `releaseY`. */
function drag(releaseY: number, between?: () => void): void {
  pointer(grip(), 'pointerdown', 0);
  pointer(view.dom, 'pointermove', 20);
  between?.();
  pointer(view.dom, 'pointermove', releaseY);
  pointer(view.dom, 'pointerup', releaseY);
}

describe('dragging a grip', () => {
  test('a block dropped past the last sibling moves, in one undo step', () => {
    mount();
    drag(85);
    expect(view.state.doc.toString()).toBe('b\n\nc\n\n:::callout\nx\n:::');
  });

  test('a document that changed mid-drag is left as it is: the siblings were read from the old one', () => {
    mount();
    drag(85, () => view.dispatch({ changes: { from: 0, insert: 'Z\n\n' } }));
    expect(view.state.doc.toString()).toBe('Z\n\n' + SRC);
    expect(view.dom.querySelector('.cm-grip-lifted')).toBeNull();
  });

  test('a read-only editor does not drag', () => {
    mount(true);
    drag(85);
    expect(view.state.doc.toString()).toBe(SRC);
    expect(view.dom.classList.contains('cm-grippable')).toBe(false);
  });

  test('destroying the view mid-drag lets go of the Esc listener', () => {
    mount();
    const remove = vi.spyOn(document, 'removeEventListener');
    pointer(grip(), 'pointerdown', 0);
    view.destroy();
    expect(remove).toHaveBeenCalledWith('keydown', expect.any(Function), true);
  });

  test('losing the pointer cancels the drag', () => {
    mount();
    pointer(grip(), 'pointerdown', 0);
    pointer(view.dom, 'pointermove', 20);
    expect(view.dom.querySelector('.cm-grip-lifted')).not.toBeNull();
    view.dom.dispatchEvent(new Event('lostpointercapture'));
    expect(view.dom.querySelector('.cm-grip-lifted')).toBeNull();
    pointer(view.dom, 'pointerup', 85);
    expect(view.state.doc.toString()).toBe(SRC);
  });
});
