// Floating overlay primitive used by the chip bar.
//
// A single overlay can be open at a time; callers wire that mutual exclusion
// themselves by closing the previous handle before opening a new one. The
// primitive owns:
//   - DOM lifecycle (create root, append to <body>, remove on close)
//   - Positioning (below an anchor, or at a cursor point) with viewport clamping
//   - Outside-click dismissal (and dismissal when the webview loses focus)
//   - The internal click-stopPropagation rule (so clicks inside don't bubble
//     to the bar/document and self-close)
//
// What the primitive does NOT do:
//   - Decide what's inside (caller fills it via `build`)
//   - Track "what's open" beyond its own handle (the chip bar still maintains
//     `activeOverlay` so it can answer "is the edit popover for key X open?"
//     for click-toggle behavior)
//   - Manage focus inside the overlay (caller can rAF-focus after `open`
//     returns; in practice every caller does its own input.focus())

import { clampIntoViewport } from "../shared/ui/ctx-menu";
import { armOutsideDismiss } from "../shared/ui/dismiss-outside";

/** Pixel gap between an overlay and the bottom edge of its anchor. */
const OVERLAY_ANCHOR_GAP = 4;

export interface OverlayHandle {
  /** The overlay's root DOM element. Caller appends content into this. */
  readonly el: HTMLElement;
  /** Close the overlay. Idempotent. Fires `onClose` once. */
  close(): void;
}

export interface OpenOverlayOptions {
  /** CSS class on the overlay root element. */
  className: string;
  /**
   * Keep the overlay open across one webview blur — see `suppressBlur` in
   * `shared/ui/dismiss-outside.ts`. The cover field's Browse button sets this
   * while the system file dialog is up.
   */
  suppressBlur?(): boolean;
  /** Build the overlay's contents into the root element. */
  build(root: HTMLElement): void;
  /**
   * Where to place the overlay, plus the positioning anchor when below.
   *
   * - `below` aligns the overlay to the anchor's left edge and sits below
   *   its bottom edge. The same `anchor` is also treated as "inside" for
   *   outside-click — clicking it again doesn't dismiss (the chip's click
   *   handler takes the toggle responsibility).
   * - `cursor` positions at the given point. `outsideAnchor` (optional) is
   *   treated as "inside" for outside-click — typically the chip that
   *   spawned the menu, so right-clicking it again doesn't fight with the
   *   contextmenu handler's own re-open call.
   */
  position:
    | { kind: 'below'; anchor: HTMLElement }
    | { kind: 'cursor'; x: number; y: number; outsideAnchor?: HTMLElement };
  /** Fired exactly once when the overlay closes (button, outside-click, Escape, programmatic). */
  onClose?(): void;
}

export function openOverlay(opts: OpenOverlayOptions): OverlayHandle {
  const root = document.createElement('div');
  root.className = opts.className;
  // Clicks INSIDE the overlay must not bubble to the document/outside-click
  // handler — otherwise typing into a popover input would close it.
  //
  // We stop both `mousedown` and `click`. The mousedown stop is stronger
  // than the previous `click`-only setup: a document-level `mousedown`
  // listener added later (e.g., a global focus tracker or dismiss-on-click
  // toolbar) would otherwise see clicks INSIDE any open overlay. No such
  // listener exists today; if you add one, register it in capture phase or
  // scope it the way our outside-click handler does (`anchor.contains`).
  root.addEventListener('mousedown', (e) => e.stopPropagation());
  root.addEventListener('click', (e) => e.stopPropagation());

  opts.build(root);
  document.body.appendChild(root);

  // Outside-click scoping: clicks on this element are treated as "inside"
  // and don't dismiss. For 'below', the positioning anchor doubles as the
  // outside-click anchor. For 'cursor', the optional outsideAnchor (or null,
  // meaning "any click outside the overlay closes").
  const outsideAnchor: HTMLElement | null =
    opts.position.kind === 'below' ? opts.position.anchor : (opts.position.outsideAnchor ?? null);

  if (opts.position.kind === 'below') {
    positionBelow(opts.position.anchor, root);
  } else {
    positionAtCursor(opts.position.x, opts.position.y, root);
  }

  let closed = false;
  // Outside-click plus the two gestures that never reach this document —
  // a click on a window drag region, and a click in another webview. See
  // dismiss-outside.ts.
  const disarmOutside = armOutsideDismiss({
    inside: [root, outsideAnchor],
    covers: [root],
    onDismiss: () => handle.close(),
    suppressBlur: opts.suppressBlur,
  });

  const handle: OverlayHandle = {
    el: root,
    close() {
      if (closed) return;
      closed = true;
      disarmOutside();
      root.remove();
      opts.onClose?.();
    },
  };
  return handle;
}

function positionBelow(anchor: HTMLElement, el: HTMLElement): void {
  const r = anchor.getBoundingClientRect();
  el.style.position = 'fixed';
  el.style.left = `${r.left}px`;
  el.style.top = `${r.bottom + OVERLAY_ANCHOR_GAP}px`;
  requestAnimationFrame(() => clampIntoViewport(el));
}

function positionAtCursor(x: number, y: number, el: HTMLElement): void {
  el.style.position = 'fixed';
  el.style.left = `${x}px`;
  el.style.top = `${y}px`;
  requestAnimationFrame(() => clampIntoViewport(el));
}
