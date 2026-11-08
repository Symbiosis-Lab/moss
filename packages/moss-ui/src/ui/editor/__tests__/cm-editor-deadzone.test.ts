// packages/moss-ui/src/ui/editor/__tests__/cm-editor-deadzone.test.ts
//
// Background clicks in the editor pane must place the caret. The field bug:
// clicking into an empty document — four times — produced no caret, because
// the click landed on a box outside CM6's contentDOM (a CSS dead zone), and
// CM6 never saw the pointer event. CmEditor now forwards mousedowns whose
// target is one of the pane's background boxes (container / .cm-editor /
// .cm-scroller) to posAtCoords → setSelection.
//
// jsdom has no layout, so posAtCoords is stubbed; the real hit-zone geometry
// needs a browser test that measures the caret hit zone.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { CmEditor } from '../cm-editor';

function mount(content = 'hello world'): { editor: CmEditor; container: HTMLElement } {
  document.body.innerHTML = '<div id="moss-editor-cm"></div>';
  const container = document.getElementById('moss-editor-cm')!;
  const editor = new CmEditor({ content, container, onChange: () => {} });
  return { editor, container };
}

function mousedownOn(target: EventTarget, button = 0): MouseEvent {
  const ev = new MouseEvent('mousedown', {
    bubbles: true,
    cancelable: true,
    button,
    clientX: 40,
    clientY: 500,
  });
  target.dispatchEvent(ev);
  return ev;
}

describe('CmEditor dead-zone click forwarding', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('a mousedown on the scroller background places the caret and focuses', () => {
    const { editor } = mount();
    const view = editor.getView();
    vi.spyOn(view, 'posAtCoords').mockReturnValue(5);
    const focus = vi.spyOn(view, 'focus');

    const ev = mousedownOn(view.scrollDOM);

    expect(ev.defaultPrevented).toBe(true);
    expect(focus).toHaveBeenCalled();
    expect(view.state.selection.main.head).toBe(5);
    editor.destroy();
  });

  it('a mousedown on the container itself (e.g. the composer body padding band) is forwarded', () => {
    const { editor, container } = mount();
    const view = editor.getView();
    vi.spyOn(view, 'posAtCoords').mockReturnValue(11);

    const ev = mousedownOn(container);

    expect(ev.defaultPrevented).toBe(true);
    expect(view.state.selection.main.head).toBe(11);
    editor.destroy();
  });

  it('a mousedown on .cm-editor between scroller edges is forwarded', () => {
    const { editor } = mount();
    const view = editor.getView();
    vi.spyOn(view, 'posAtCoords').mockReturnValue(0);
    view.dispatch({ selection: { anchor: 3 } });

    const ev = mousedownOn(view.dom);

    expect(ev.defaultPrevented).toBe(true);
    expect(view.state.selection.main.head).toBe(0);
    editor.destroy();
  });

  it('clicks inside the contentDOM stay CM6-owned (no double dispatch)', () => {
    const { editor } = mount();
    const view = editor.getView();
    const posAtCoords = vi.spyOn(view, 'posAtCoords');

    mousedownOn(view.contentDOM);

    // CM6's own mousedown handler may preventDefault; what matters is that the
    // forwarder stayed out of it.
    expect(posAtCoords).not.toHaveBeenCalled();
    editor.destroy();
  });

  it('clicks on other chrome mounted in the container pass through untouched', () => {
    const { editor, container } = mount();
    const view = editor.getView();
    const posAtCoords = vi.spyOn(view, 'posAtCoords');
    const bar = document.createElement('div');
    bar.className = 'insert-bar';
    container.appendChild(bar);

    const ev = mousedownOn(bar);

    expect(ev.defaultPrevented).toBe(false);
    expect(posAtCoords).not.toHaveBeenCalled();
    editor.destroy();
  });

  it('non-primary buttons are ignored', () => {
    const { editor } = mount();
    const view = editor.getView();
    const posAtCoords = vi.spyOn(view, 'posAtCoords');

    const ev = mousedownOn(view.scrollDOM, 2);

    expect(ev.defaultPrevented).toBe(false);
    expect(posAtCoords).not.toHaveBeenCalled();
    editor.destroy();
  });

  it('destroy removes the listener', () => {
    const { editor, container } = mount();
    const view = editor.getView();
    const posAtCoords = vi.spyOn(view, 'posAtCoords');
    editor.destroy();

    const ev = mousedownOn(container);

    expect(ev.defaultPrevented).toBe(false);
    expect(posAtCoords).not.toHaveBeenCalled();
  });
});
