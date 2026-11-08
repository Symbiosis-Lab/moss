/**
 * Collapsed-pane edge affordance, RIGHT edge — the "back to split" accelerator
 * the EDITOR paints while it is full-width.
 *
 * Why this lives in the editor webview and nowhere else: in editor-full-width
 * the preview webview — which is also the shell document — is natively hidden
 * (`PreviewPlacement::Hidden`) and the action panel is sized to the whole
 * window. The shell cannot paint one pixel there. So the editor owns this edge,
 * and must tear it down the INSTANT full-width ends, because in split mode
 * those same pixels belong to the shell's `.moss-webview-divider` — a different
 * gesture and a different cursor.
 *
 * One payoff from the editor owning it: hover is detected from a `pointermove`
 * listener on the document rather than by an element, so the editor's 6px
 * scrollbar at that same edge stays grabbable. Only the revealed tab takes
 * `pointer-events: auto` (`.is-armed`, granted after the dwell has elapsed —
 * never during it, or an invisible target would steal a scrollbar grab).
 *
 * The price of that is a pointer position nobody else reports, so this module
 * remembers the last one for the life of the editor. Every route into
 * full-width leaves the pointer parked — a right-snap drag releases on mouseup,
 * ⌘⇧E moves no mouse — and an element that appears under a stationary cursor
 * gets no event of any kind. Waiting for one would fade the tab out from under
 * that cursor 1.2s later, and here there is no `:hover` to catch it.
 *
 * The tab is redundancy for symmetry, not the primary exit: the titlebar layout
 * button and ⌘⇧E stay. That is what keeps a hover-only edge legal under "stable
 * chrome, never hover-revealed triggers" and what satisfies WCAG 2.2 SC 2.5.8's
 * "Equivalent" exception, which asks for a different control, not a shortcut.
 *
 * Wiring: listens to `editor:full-width-changed` and emits `editor:request-split`
 * on click, over the host's bus: the same event the layout button already emits.
 *
 * Everything painted (geometry, the muted fill, the dwell, reduced motion) is
 * in `styles/shared/edge-restore.css`, shared byte-for-byte with the shell's
 * left edge.
 */

import { host } from "../host";
import { t } from "../i18n/index";

/** SC 2.5.8's floor. The zone never widens — see the stylesheet's header. */
const ZONE_PX = 24;
/** Keep clear of the editor's nav band (breadcrumb + create/layout buttons). */
const NAV_BAND_PX = 48;
/** moss's standing hover-intent dwell. The CSS carries the same number for the
 *  fade; this timer only decides when the tab may be CLICKED. */
const DWELL_MS = 150;
/** Just-in-time reveal hold — see divider.ts, which owns the left edge's. */
const REVEAL_HOLD_MS = 1200;
/** A fast flick to the edge right after a scroll is travel, not intent. */
const SCROLL_SUPPRESS_MS = 400;

const CHEVRON_LEFT = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>`;

let stripEl: HTMLElement | null = null;
let inZone = false;
let dwellTimer: ReturnType<typeof setTimeout> | null = null;
let revealTimer: ReturnType<typeof setTimeout> | null = null;
let suppressUntil = 0;
/** Where the pointer last was, tracked for the LIFE of the editor rather than
 *  only while the strip is mounted. Every route into full-width ends with the
 *  pointer parked — a divider right-snap releases on mouseup, ⌘⇧E moves no
 *  mouse at all — so the strip is created under a cursor that then sends no
 *  `pointermove`. Without a seeded position the 1.2s hold lapses and fades the
 *  tab out from directly under that cursor: the one rule the spec states by
 *  name, and the right edge has no `:hover` to rescue it, because the strip is
 *  `pointer-events: none` by design. */
let lastPointerX = -1;
let lastPointerY = -1;

function clearTimer(t: ReturnType<typeof setTimeout> | null): null {
  if (t !== null) clearTimeout(t);
  return null;
}

/** Is (x, y) inside the live edge zone? Suppression is deliberately NOT part of
 *  this: it answers "where is the pointer", not "should the tab appear". */
function inEdgeZone(x: number, y: number): boolean {
  return x >= 0 && x >= window.innerWidth - ZONE_PX && y >= NAV_BAND_PX;
}

function trackPointer(e: PointerEvent): void {
  lastPointerX = e.clientX;
  lastPointerY = e.clientY;
}

function onPointerMove(e: PointerEvent): void {
  if (!stripEl) return;
  const hot = inEdgeZone(e.clientX, e.clientY) && Date.now() >= suppressUntil;
  if (hot === inZone) return;
  inZone = hot;

  if (hot) {
    // The class starts the CSS dwell (`transition-delay`); the timer below only
    // hands the tab its pointer events once that dwell has actually elapsed.
    stripEl.classList.add('is-hot');
    dwellTimer = clearTimer(dwellTimer);
    dwellTimer = setTimeout(() => {
      dwellTimer = null;
      stripEl?.classList.add('is-armed');
    }, DWELL_MS);
  } else {
    stripEl.classList.remove('is-hot', 'is-armed', 'is-pressed');
    dwellTimer = clearTimer(dwellTimer);
    // A pointer that leaves also ends a just-in-time hold that outlasted its
    // timer only because the pointer was here.
    if (revealTimer === null) stripEl.classList.remove('is-revealed');
  }
}

function onScroll(): void {
  suppressUntil = Date.now() + SCROLL_SUPPRESS_MS;
  if (!stripEl) return;
  leaveZone();
}

/** Drop every pointer-derived class. Shared by the scroll suppressor and by the
 *  two events that mean "the pointer is no longer anywhere in this document".
 *
 *  Those two matter more than they look. `onPointerMove` is the ONLY thing that
 *  clears `.is-hot` / `.is-armed`, and it needs a move to do it — so a cursor
 *  that enters the zone and then leaves the WINDOW (every intermediate position
 *  still inside the 24px band, so no transition is ever observed) left the tab
 *  painted and clickable indefinitely. That is the same shape as the bug this
 *  whole change is about: state held on a belief that no event will correct.
 *  `pointerleave` on the document and `blur` on the window are the events that
 *  say the belief has expired. */
function leaveZone(): void {
  if (!stripEl) return;
  inZone = false;
  dwellTimer = clearTimer(dwellTimer);
  stripEl.classList.remove('is-hot', 'is-armed', 'is-pressed');
}

/** Animate the tab in and hold it ~1.2s, marking the place the preview went. */
function reveal(): void {
  if (!stripEl) return;
  // Seed from the last known pointer position rather than waiting for a move
  // that a parked pointer will never send. The tab is already visible for the
  // hold, so it is armed straight away — a dwell would be asking the user to
  // earn something they can already see.
  if (lastPointerX >= 0 && inEdgeZone(lastPointerX, lastPointerY)) {
    inZone = true;
    stripEl.classList.add('is-hot', 'is-armed');
  }
  stripEl.classList.add('is-revealed');
  revealTimer = clearTimer(revealTimer);
  revealTimer = setTimeout(() => {
    revealTimer = null;
    // Ends UNCONDITIONALLY, unlike the first version, which kept the hold while
    // `inZone` was true and so could hold forever on a seed that was wrong (the
    // pointer parks elsewhere and sends no `pointermove` to correct it).
    //
    // Nothing is lost by ending it here: `inZone` sets `.is-hot`, and `.is-hot`
    // paints the tab on its own for exactly as long as the pointer is in the
    // zone. That class IS this edge's `:hover`, and it is cleared by a real
    // event — a move out of the zone, a scroll, or the pointer leaving the
    // document — rather than by a timer that guessed.
    //
    // Handing over does mean handing over the CLICK too. If the pointer arrived
    // late in the hold, the dwell timer may still be pending: the tab is fully
    // painted by `.is-revealed`, and dropping that class would take
    // `pointer-events` away for the rest of the dwell, leaving a visible target
    // that swallows nothing for ~150ms. There is no dwell left to earn — the
    // dwell exists so an INVISIBLE target cannot steal a scrollbar grab, and
    // this one has been on screen for over a second — so arm it now.
    if (inZone) {
      dwellTimer = clearTimer(dwellTimer);
      stripEl?.classList.add('is-armed');
    }
    stripEl?.classList.remove('is-revealed');
  }, REVEAL_HOLD_MS);
}

function mount(): void {
  if (stripEl) return;
  const strip = document.createElement('div');
  strip.className = 'moss-edge-restore moss-edge-restore--right';
  strip.id = 'moss-edge-restore-right';

  const tab = document.createElement('button');
  tab.type = 'button';
  tab.className = 'moss-edge-restore__tab';
  // No data-tooltip, on either edge. moss's portal tooltip has no dwell — it
  // paints on the first `mouseover` — so a bubble would appear the moment the
  // pointer crossed the invisible column, before the tab it names had begun to
  // fade in, and for a pointer merely passing through. `aria-label` says the
  // same thing to a screen reader and paints nothing.
  tab.setAttribute('aria-label', t('toolbar.back_to_split'));
  tab.innerHTML = CHEVRON_LEFT;
  // Capture the pointer on press so the release lands here wherever it happens.
  // Without it a press that drags 30px down — still inside the 24px column but
  // off the 64px tab — never fires `pointerup` on the tab (a mouse pointer gets
  // no implicit capture), and the pressed fill stays latched until the pointer
  // leaves the zone entirely. `pointercancel` covers a system gesture takeover,
  // which had no handler at all.
  const release = (): void => { strip.classList.remove('is-pressed'); };
  tab.addEventListener('pointerdown', (e) => {
    strip.classList.add('is-pressed');
    // Best-effort: capture is an optimisation on top of the pointercancel /
    // pointerup pair, and it throws on an id the engine has no record of.
    try { tab.setPointerCapture(e.pointerId); } catch { /* no capture available */ }
  });
  tab.addEventListener('pointerup', release);
  tab.addEventListener('pointercancel', release);
  tab.addEventListener('click', () => { host.bus.emit('editor:request-split'); });

  strip.appendChild(tab);
  document.body.appendChild(strip);
  stripEl = strip;

  document.addEventListener('pointermove', onPointerMove);
  document.addEventListener('wheel', onScroll, { passive: true });
  document.addEventListener('scroll', onScroll, { passive: true, capture: true });
  // …and the two ways the pointer stops being in this document without ever
  // sending a move that says so. See leaveZone().
  document.addEventListener('pointerleave', leaveZone);
  window.addEventListener('blur', leaveZone);

  reveal();
}

function unmount(): void {
  if (!stripEl) return;
  document.removeEventListener('pointermove', onPointerMove);
  document.removeEventListener('wheel', onScroll);
  document.removeEventListener('scroll', onScroll, { capture: true });
  document.removeEventListener('pointerleave', leaveZone);
  window.removeEventListener('blur', leaveZone);
  dwellTimer = clearTimer(dwellTimer);
  revealTimer = clearTimer(revealTimer);
  stripEl.remove();
  stripEl = null;
  inZone = false;
  suppressUntil = 0;
}

/**
 * Wire the right edge. Call once from the editor's init; the webview is created
 * once and persists across show/hide, so this registers a single listener.
 */
export function initEdgeRestore(): void {
  // Registered here, not in mount(): the pointer's position has to be known
  // BEFORE the strip exists, because the gesture that creates it is also the
  // gesture that leaves the pointer standing still. Two numbers per move.
  document.addEventListener('pointermove', trackPointer);
  host.bus.on<{ fullWidth: boolean }>('editor:full-width-changed', ({ fullWidth }) => {
    if (fullWidth) mount(); else unmount();
  });
}

/** Test hook: the live strip, or null when the editor is not full-width. */
export function edgeRestoreElement(): HTMLElement | null {
  return stripEl;
}
