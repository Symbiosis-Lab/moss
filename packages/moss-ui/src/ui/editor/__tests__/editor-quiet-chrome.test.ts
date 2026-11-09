import { describe, test, expect, beforeEach, vi } from 'vitest';
import { EditorView } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { nextQuietChromeState, quietChromeExtension, FADED_CLASS } from '../editor-quiet-chrome';

describe('nextQuietChromeState', () => {
  test('typing fades', () => {
    expect(nextQuietChromeState('visible', 'type')).toBe('faded');
  });
  test('pointer movement restores', () => {
    expect(nextQuietChromeState('faded', 'pointermove')).toBe('visible');
  });
  test('Escape restores', () => {
    expect(nextQuietChromeState('faded', 'escape')).toBe('visible');
  });
});

describe('quietChromeExtension', () => {
  let band: HTMLElement, bc: HTMLElement, tree: HTMLElement, host: HTMLElement;
  beforeEach(() => {
    document.body.innerHTML =
      '<div class="editor-band"></div><div class="nav-bc-strip"></div>' +
      '<div class="nav-body"></div><div id="host"></div>';
    band = document.querySelector('.editor-band')!;
    bc = document.querySelector('.nav-bc-strip')!;
    tree = document.querySelector('.nav-body')!;
    host = document.getElementById('host')!;
  });

  test('a typed transaction fades all three containers', () => {
    const view = new EditorView({
      state: EditorState.create({ doc: '', extensions: [quietChromeExtension()] }),
      parent: host,
    });
    view.dispatch({ changes: { from: 0, insert: 'a' }, userEvent: 'input.type' });
    expect(band.classList.contains(FADED_CLASS)).toBe(true);
    expect(bc.classList.contains(FADED_CLASS)).toBe(true);
    expect(tree.classList.contains(FADED_CLASS)).toBe(true);
    view.destroy();
  });

  test('window pointermove restores', () => {
    const view = new EditorView({
      state: EditorState.create({ doc: '', extensions: [quietChromeExtension()] }),
      parent: host,
    });
    view.dispatch({ changes: { from: 0, insert: 'a' }, userEvent: 'input.type' });
    window.dispatchEvent(new Event('pointermove'));
    expect(band.classList.contains(FADED_CLASS)).toBe(false);
    view.destroy();
  });

  test('reduced motion sets the transition to 0s instead of animating', () => {
    // Full MediaQueryList shape: CM6's own DOMObserver calls matchMedia too
    // (a print-mode listener), and a bare `{ matches }` stub breaks on its
    // `.addListener` call. Same convention as versions-surface.test.ts.
    vi.stubGlobal('matchMedia', (q: string) => ({
      matches: q.includes('reduced-motion'), media: q,
      addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
      onchange: null, dispatchEvent: () => false,
    }));
    const view = new EditorView({
      state: EditorState.create({ doc: '', extensions: [quietChromeExtension()] }),
      parent: host,
    });
    view.dispatch({ changes: { from: 0, insert: 'a' }, userEvent: 'input.type' });
    expect(band.style.transitionDuration).toBe('0s');
    view.destroy();
    vi.unstubAllGlobals();
  });
});
