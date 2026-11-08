// Quiet chrome: once the author starts typing, the
// editor band (the insert bar fades with it as its own descendant — opacity
// multiplies), the path bar and the file tree fade to nothing over 200ms —
// opacity only, so no rect moves — and return on pointer movement or Escape.
// Instant under prefers-reduced-motion, via an inline transitionDuration
// override rather than a second CSS media block: that keeps the one
// mechanism visible to both a jsdom unit test (no cascade) and the render
// gate (a real engine).
//
// The faded containers live OUTSIDE the CM6 view — `.editor-band` and
// `.nav-grid`'s `.nav-bc-strip`/`.nav-body` are siblings of #moss-editor-cm
// in editor-main.ts's shell — so this is a ViewPlugin that reaches out to
// `document`/`window`, the same shape cm-insert-bar.ts's own (now removed)
// dim-while-typing pulse used.

import { EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';
import type { Extension } from '@codemirror/state';

export type QuietChromeState = 'visible' | 'faded';
export type QuietChromeEvent = 'type' | 'pointermove' | 'escape';
export const FADED_CLASS = 'moss-quiet-chrome-faded';

const FADE_SELECTORS = ['.editor-band', '.nav-bc-strip', '.nav-body'];

/** Pure transition table — no DOM, no CM6 view. */
export function nextQuietChromeState(
  state: QuietChromeState,
  event: QuietChromeEvent,
): QuietChromeState {
  if (event === 'type') return 'faded';
  return 'visible'; // pointermove, escape
}

function applyFaded(faded: boolean): void {
  const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
  for (const sel of FADE_SELECTORS) {
    const el = document.querySelector<HTMLElement>(sel);
    if (!el) continue;
    el.style.transitionDuration = reduced ? '0s' : '';
    el.classList.toggle(FADED_CLASS, faded);
  }
}

export function quietChromeExtension(): Extension {
  return ViewPlugin.fromClass(
    class {
      private chromeState: QuietChromeState = 'visible';
      private onPointerMove = () => this.transition('pointermove');
      private onKeyDown = (e: KeyboardEvent) => {
        if (e.key === 'Escape') this.transition('escape');
      };
      constructor(_view: EditorView) {
        window.addEventListener('pointermove', this.onPointerMove);
        window.addEventListener('keydown', this.onKeyDown, true);
      }
      update(u: ViewUpdate) {
        if (u.docChanged && u.transactions.some((tr) => tr.isUserEvent('input.type'))) {
          this.transition('type');
        }
      }
      private transition(event: QuietChromeEvent) {
        const next = nextQuietChromeState(this.chromeState, event);
        if (next === this.chromeState) return;
        this.chromeState = next;
        applyFaded(next === 'faded');
      }
      destroy() {
        window.removeEventListener('pointermove', this.onPointerMove);
        window.removeEventListener('keydown', this.onKeyDown, true);
        applyFaded(false);
      }
    },
  );
}
