/**
 * Outside-dismissal for floating surfaces (context menus, overlays, ghosts).
 *
 * "Click anywhere outside and it closes" sounds like one line of code, and in
 * a plain web page it is. In moss it is not, because three of the places a
 * user naturally clicks never reach the surface's own `document` listener:
 *
 *  1. **Window drag regions.** Tauri's injected drag script listens for
 *     `mousedown` on `document` and calls `stopImmediatePropagation()` when
 *     the target is a drag region (the editor breadcrumb strip, the shell
 *     titlebar, the composer titlebar and the launcher's App Settings
 *     titlebar are marked as such). Its listener is registered before any
 *     app code, so a bubble-phase document listener added later never runs for
 *     those clicks. `dragRegionCapture` below catches exactly those events in
 *     the capture phase, which runs first.
 *
 *  2. **A different webview.** moss's window hosts several documents: the app
 *     shell (chrome + the preview iframe) and the action panel (the editor).
 *     Clicking the shell titlebar or the preview dispatches
 *     nothing at all into the editor, so the only signal that crosses the seam
 *     is losing focus — hence the `window` `blur` listener.
 *
 *  3. **The preview iframe.** The previewed page is a separate document inside
 *     the shell webview, so its `mousedown` never bubbles into the shell — and
 *     for a surface OPENED from inside that iframe (the preview's right-click
 *     menu) blur cannot stand in, because the opening right-click already
 *     moved focus into the iframe and the shell window has no focus left to
 *     lose. The bridge posts what the DOM cannot deliver: a
 *     `moss-preview-pointerdown` on any press in the previewed page, and
 *     `moss-escape-key`.
 *
 * The ordinary outside-click listener stays in the **bubble** phase on
 * purpose. A `stopPropagation()` on the way up is a deliberate "this click
 * does not dismiss" contract that surfaces rely on — the editor's undo/redo
 * buttons hold an uncommitted chip popover open exactly that way. Capturing
 * everything would silently void it.
 */

import { suppressTooltips } from '../../components/tooltip';

/** Why the surface was dismissed. Callers that restore focus on close should
 *  do so only on `'outside-click'`: for the other two reasons focus has
 *  already left this document, and calling `focus()` would yank the user back
 *  out of whatever they just clicked. */
export type DismissReason = 'outside-click' | 'webview-blur' | 'preview-interaction';

/** Message types the preview bridge posts up when the user acts inside the
 *  previewed page. `moss-escape-key` predates this and is also consumed by
 *  crates/moss-build/src/js-src/site/immersive-mode.ts; `moss-preview-pointerdown` exists only for
 *  dismissal. A page inside the preview could forge either, and the worst it
 *  can do is close a menu it is already underneath. */
const PREVIEW_INTERACTION_TYPES = new Set(['moss-preview-pointerdown', 'moss-escape-key']);

export interface OutsideDismissOptions {
  /** Subtrees that count as "inside" — a mousedown there never dismisses.
   *  Nullish entries are ignored, so callers can pass optional anchors. */
  inside: Array<HTMLElement | null | undefined>;
  /** Runs at most once per armed session; the caller closes the surface. */
  onDismiss(reason: DismissReason): void;
  /**
   * Consulted before a **blur** dismissal only: return `true` and this blur
   * is ignored.
   *
   * For the one gesture where leaving the webview is the point rather than a
   * departure — a native file dialog opened from inside the surface, which
   * blurs the webview by existing and would otherwise close the very popover
   * the picked file is meant to land in.
   *
   * It is asked HERE, not inside `onDismiss`: `dismiss` sets `fired` and
   * disarms *before* calling back, so a caller that refused there would leave
   * the surface armed-but-fired — permanently un-dismissable. An outside
   * click still dismisses normally throughout; only this one blur is spent.
   */
  suppressBlur?(): boolean;
  /**
   * The elements this surface actually PAINTS. Tooltips outside them are held
   * off for the armed lifetime; `[]` means the surface covers nothing and no
   * tooltip is held off.
   *
   * Deliberately NOT defaulted to `inside`, and deliberately required. The two
   * lists answer different questions: `inside` names what a press may land on
   * without dismissing, so it usually includes the ANCHOR — and the anchor is
   * the one trigger whose tooltip is certainly covered, since the surface opens
   * onto it. Scoping the hold to `inside` would spare the anchor's own hint.
   * Requiring the field makes that a compile error at every call site instead
   * of a silent hole at one.
   *
   * A `--z-popup` tooltip loses to every tier that arms here (`.ctx-menu` at
   * `--z-toast` + 50, the `.pop`/`.omenu`/`.dropdown` tier at `--z-overlay`),
   * so it would only be something to look past. Holding for the whole armed
   * lifetime rather than just the opening press is what stops a pointer
   * re-crossing the anchor from resurrecting the tooltip underneath it.
   *
   * The one `[]` is the chip bar's provisional ghost
   * (`armProvisionalDismiss`), which arms click-away for an
   * IN-FLOW chip rather than a floating surface: it covers nothing, and its
   * pending state can outlast a hover by minutes.
   */
  covers: Array<HTMLElement | null | undefined>;
}

const DRAG_REGION_ATTR = 'data-tauri-drag-region';

/**
 * True when Tauri's drag script will swallow this `mousedown` before it
 * bubbles. Mirrors `isDragRegion` in Tauri's own drag script: a bare
 * (or `"true"`) attribute matches only when the element IS the target, while
 * `"deep"` matches anywhere in its subtree, and `"false"` opts back out.
 * Deliberately a mirror rather than an approximation — a broader test would
 * dismiss on clicks that DO still bubble, double-handling them.
 */
export function isSwallowedByDragRegion(ev: MouseEvent): boolean {
  if (ev.button !== 0) return false;
  const path = ev.composedPath();
  const target = path[0];
  for (const node of path) {
    if (!(node instanceof HTMLElement)) continue;
    const attr = node.getAttribute(DRAG_REGION_ATTR);
    if (attr === null) continue;
    if (attr === 'false') return false;
    if (attr === 'deep') return true;
    return node === target;
  }
  return false;
}

/**
 * Arm outside-dismissal: an outside mousedown, a drag-region click, a webview
 * blur, and a press or Escape inside the preview iframe.
 *
 * The one entry point on purpose: an ordinary outside listener hand-rolled
 * beside a blind-spots-only variant would be two expressions of one idea,
 * and they drift.
 *
 * Registration is deferred one tick so the very interaction that opened the
 * surface (still propagating when this is called) cannot immediately close it.
 *
 * Returns an idempotent disarm function, safe to call from `onDismiss`.
 */
export function armOutsideDismiss(opts: OutsideDismissOptions): () => void {
  return arm(opts);
}

function arm(opts: OutsideDismissOptions): () => void {
  let fired = false;
  // Scoped to what the surface paints: a tooltip on a trigger inside it is
  // not one it is covering.
  const painted = opts.covers.filter((el): el is HTMLElement => !!el);
  const releaseTooltips = painted.length === 0 ? () => {} : suppressTooltips(painted);

  const isInside = (target: Node | null): boolean => {
    if (!target) return true;
    for (const el of opts.inside) {
      if (el?.contains(target)) return true;
    }
    return false;
  };

  const dismiss = (reason: DismissReason): void => {
    if (fired) return;
    fired = true;
    disarm();
    opts.onDismiss(reason);
  };

  const onMousedown = (ev: MouseEvent): void => {
    if (isInside(ev.target as Node | null)) return;
    dismiss('outside-click');
  };
  // Capture phase, and only for the events the bubble listener can never see.
  const onDragRegionCapture = (ev: MouseEvent): void => {
    if (!isSwallowedByDragRegion(ev)) return;
    if (isInside(ev.target as Node | null)) return;
    dismiss('outside-click');
  };
  const onBlur = (): void => {
    if (opts.suppressBlur?.()) return;
    dismiss('webview-blur');
  };
  // Only the shell webview ever receives these; elsewhere it is a no-op
  // listener. `inside` cannot contain the iframe's content, so no containment
  // check applies — a press in the previewed page is always outside.
  const onPreviewMessage = (ev: MessageEvent): void => {
    const type = (ev.data as { type?: unknown } | null)?.type;
    if (typeof type !== 'string' || !PREVIEW_INTERACTION_TYPES.has(type)) return;
    dismiss('preview-interaction');
  };

  const timerId = window.setTimeout(() => {
    document.addEventListener('mousedown', onMousedown);
    document.addEventListener('mousedown', onDragRegionCapture, true);
    window.addEventListener('blur', onBlur);
    window.addEventListener('message', onPreviewMessage);
  }, 0);

  function disarm(): void {
    releaseTooltips();
    window.clearTimeout(timerId);
    document.removeEventListener('mousedown', onMousedown);
    document.removeEventListener('mousedown', onDragRegionCapture, true);
    window.removeEventListener('blur', onBlur);
    window.removeEventListener('message', onPreviewMessage);
  }

  return disarm;
}
