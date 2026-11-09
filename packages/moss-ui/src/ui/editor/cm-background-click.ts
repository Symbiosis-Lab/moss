// Click-anywhere caret placement — the editor pane's background must not be
// a dead zone.
//
// CM6 registers its pointer handlers on the contentDOM only, so a click whose
// target is one of the boxes AROUND the text — the mount container (the
// composer's `.moss-composer__body` carries a 14px padding band CM6 never
// sees), the scroller, `.cm-editor` itself — never reaches CM6. What made
// those clicks work anyway was the browser's mousedown focus-fixup ("focus
// the nearby editable"), which is unspecified behavior that varies across
// engines and WebKit builds — and in the field it failed: a user clicked four
// times into an empty document and no caret appeared. Place the caret
// explicitly (posAtCoords → setSelection) instead of trusting the engine's
// heuristic.
//
// Deliberately narrow: only clicks whose TARGET is one of those background
// boxes are forwarded. Clicks inside the contentDOM are CM6's own business,
// and clicks on other chrome mounted in the container (insert bar, doc-state
// cluster, heading, gutter glyphs, seal scrim) pass through untouched.
//
// Pinned by packages/moss-ui/src/ui/editor/__tests__/cm-editor-deadzone.test.ts (jsdom,
// forwarding logic) and a browser test of caret hit zones
// (real engine, hit-zone geometry + behavior).
import { EditorView } from '@codemirror/view';

/**
 * Forward background mousedowns in `container` to `view` as caret placement.
 * Returns the uninstaller; call it before the view is destroyed.
 */
export function installBackgroundClickCaret(
  view: EditorView,
  container: HTMLElement,
): () => void {
  const onMousedown = (event: MouseEvent): void => {
    if (event.button !== 0) return;
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    const isBackground =
      target === container || target === view.dom || target === view.scrollDOM;
    if (!isBackground) return;
    // Scrollbar guard: some engines dispatch a scrollbar-track mousedown with
    // the scrolling element as target. clientWidth/Height exclude scrollbars,
    // so a point beyond them is a scrollbar drag, not a caret click. (A
    // clientWidth of 0 means no layout — jsdom — where there are no
    // scrollbars to guard against.)
    const rect = target.getBoundingClientRect();
    if (
      target.clientWidth > 0 &&
      (event.clientX - rect.left - target.clientLeft > target.clientWidth ||
        event.clientY - rect.top - target.clientTop > target.clientHeight)
    ) {
      return;
    }
    const pos = view.posAtCoords({ x: event.clientX, y: event.clientY }, false);
    // preventDefault so the mousedown doesn't start a native drag-selection on
    // the background element or hand focus decisions back to the engine.
    event.preventDefault();
    view.focus();
    view.dispatch({
      selection: { anchor: pos },
      scrollIntoView: true,
      userEvent: 'select.pointer',
    });
  };

  container.addEventListener('mousedown', onMousedown);
  return () => container.removeEventListener('mousedown', onMousedown);
}
