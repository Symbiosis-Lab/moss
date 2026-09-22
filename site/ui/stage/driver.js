// Turns player.js's verbs into actions on the harvested editor: typing, clicking or right-clicking
// a named target, moving the pointer overlay. Knows nothing about scenes, markers, or when
// playback starts or stops — see stage/README.md's module table.
//
// `typeInto`/`setTextInto` type into a named input that isn't the harvested editor's own
// `__editor.type()`/`setDoc()` API — the site's own `<input>` fields (a rename prompt, a
// properties search box) that real scenes drive (see their own comments below).
import { findTarget } from './harvest.js';

const PRESS_DIP_SCALE = 0.88;
const RING_EXPAND_SCALE = 2.4;

// Pace (stage/README.md, "Pace") — every timing beat the driver uses, in ONE table, each row with
// the one-line reason the README gives. A demonstration is watched, not caused, so these are not
// interface-transition numbers (100-500ms): the viewer has to find the pointer, follow it, see the
// press, and take in what appeared.
const PACE = {
  // Travel: clamp(500, 450 + 0.5 x distance_px, 1000) ms, ease-in-out. Deliberate human pointing
  // takes 0.8-1.5s; slower than about a second drags. Scaled with distance so short hops aren't
  // sluggish.
  travelMinMs: 500,
  travelBaseMs: 450,
  travelPerPxMs: 0.5,
  travelMaxMs: 1000,
  // Dwell on the target before pressing: eye re-aim (~200ms) plus one fixation (~200ms). A
  // right-click's result is less expected, so it gets longer.
  dwellLeftMs: 400,
  dwellRightMs: 600,
  // Press: click feedback reads at 100-200ms, so the dot's dip is ~150ms; the ring's own fade
  // continues ~500ms, overlapping the hold below rather than blocking it.
  pressDipMs: 150,
  pressRingMs: 500,
  // Hold after the result appears: ~200ms to look at each new item, capped at five because a
  // viewer samples a menu rather than reading it. `reveals` is a number on the step (default 2);
  // the last step of a scene holds too — nothing here special-cases "last", since every click/
  // context step already carries its own hold regardless of position.
  holdBaseMs: 400,
  holdPerRevealMs: 200,
  holdRevealCap: 5,
  defaultReveals: 2,
  // Gap between a double-click's two presses: fast enough to read as one gesture, slow enough for
  // each press's own dip/ring to register on its own — a double click should LOOK like two
  // presses (stage/README.md, "The pointer"), not one blurred one.
  dblclickGapMs: 180,
  // How long a step's own target may take to render before giving up: not a beat the viewer
  // watches, so it is generous rather than paced — a previous step's own effect (a menu opening, a
  // mode swap) may still be rendering when the next step goes looking for its target.
  targetWaitMs: 4000,
  // Fixed per-character cadence for typeInto, into the site's own plain `<input>` fields — no
  // per-character randomization: that realism lives entirely inside the harvested editor's own
  // `__editor.type()` and isn't worth reimplementing for a field typing never needs to look human.
  typeIntoCharMs: 55,
};

/** Travel duration for a glide of `distancePx` (stage/README.md, "Pace"). Pure function — no DOM,
 * no timers — so its clamp boundaries are unit-testable without a browser. */
export function travelDurationMs(distancePx) {
  const raw = PACE.travelBaseMs + PACE.travelPerPxMs * distancePx;
  return Math.min(PACE.travelMaxMs, Math.max(PACE.travelMinMs, raw));
}

/** Hold duration once a step's result has appeared (stage/README.md, "Pace"). `reveals` is the
 * step's own count of what just appeared; missing/undefined falls back to PACE.defaultReveals.
 * Pure function, same reason as travelDurationMs above. */
export function holdAfterResultMs(reveals) {
  const n = reveals ?? PACE.defaultReveals;
  return PACE.holdBaseMs + PACE.holdPerRevealMs * Math.min(n, PACE.holdRevealCap);
}

function dwellMs(button) {
  return button === 'right' ? PACE.dwellRightMs : PACE.dwellLeftMs;
}

/** Resolves `true` once `ms` elapses, or `false` the instant `signal` aborts — the same
 * true-means-completed / false-means-interrupted convention every other driver step already uses
 * (glideTo, press). Every dwell/hold below is one of these, so none of them can outlive the load
 * that started them. */
function sleep(ms, signal) {
  if (signal?.aborted) return Promise.resolve(false);
  return new Promise((resolve) => {
    const timer = setTimeout(() => { cleanup(); resolve(true); }, ms);
    const onAbort = () => { cleanup(); resolve(false); };
    function cleanup() {
      clearTimeout(timer);
      signal?.removeEventListener('abort', onAbort);
    }
    signal?.addEventListener('abort', onAbort, { once: true });
  });
}

/** Waits, bounded and abortable, for a named target to exist in `doc`. A step's target may not be
 * there yet because the previous step's own effect — a menu opening, a mode swap — is still
 * rendering; this lives here, once, rather than as a sleep verb in a scene, so every click/context
 * step benefits instead of only the scenes that happen to need it (stage/README.md keeps the verb
 * table free of timing hacks on purpose). Resolves the element as soon as it appears; rejects with
 * an AbortError if `signal` aborts first, or after PACE.targetWaitMs. */
function waitForTarget(resolveTarget, doc, name, signal) {
  const immediate = resolveTarget(doc, name);
  if (immediate) return Promise.resolve(immediate);
  return new Promise((resolve, reject) => {
    if (signal?.aborted) { reject(new DOMException('Aborted', 'AbortError')); return; }
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`Target "${name}" did not appear in time`));
    }, PACE.targetWaitMs);
    const observer = new MutationObserver(() => {
      const el = resolveTarget(doc, name);
      if (el) { cleanup(); resolve(el); }
    });
    const onAbort = () => { cleanup(); reject(new DOMException('Aborted', 'AbortError')); };
    function cleanup() {
      observer.disconnect();
      clearTimeout(timer);
      signal?.removeEventListener('abort', onAbort);
    }
    signal?.addEventListener('abort', onAbort, { once: true });
    observer.observe(doc.documentElement, { childList: true, subtree: true, attributes: true });
  });
}

/** waitForTarget, but resolves `null` instead of rejecting on an abort — every driver method below
 * treats "the reader interrupted us while we waited" exactly like any other interrupted step. A
 * genuine timeout or an unknown-target-name error still propagates: those are real failures. */
async function awaitTarget(resolveTarget, doc, name, signal) {
  try {
    return await waitForTarget(resolveTarget, doc, name, signal);
  } catch (error) {
    if (error?.name === 'AbortError') return null;
    throw error;
  }
}

// A previous step's own effect can still be moving `el` into its final position when THIS step
// starts: waitForTarget above only confirms the target EXISTS, not that its layout has settled —
// discovered on the collapse-tree scene, whose target (#divider, the tree's bottom border) exists
// in the DOM from boot but visibly moves for ~140ms while the preceding "tree" scene's rows mount
// (measured: an instrumented rect poll against a real build, top drifting ~42px -> ~72px over
// several frames). glideTo's own getBoundingClientRect() runs ONCE, at the moment this resolves,
// so a rect read mid-drift sends the pointer to glide toward and visibly rest at a stale position
// — even though the actual dispatch still lands correctly (dispatchDblClick/dispatchContextMenu
// call centerOf(el) fresh, at press time, not at glide start). Bounded at RECT_SETTLE_MAX_FRAMES
// so a target that is (unexpectedly) still moving forever cannot hang playback.
const RECT_SETTLE_STABLE_FRAMES = 2;
const RECT_SETTLE_MAX_FRAMES = 20; // ~330ms at 60fps — comfortably past the ~140ms measured above
function waitForRectStable(el, signal) {
  return new Promise((resolve) => {
    let lastKey = null;
    let stableCount = 0;
    let frame = 0;
    function tick() {
      if (signal?.aborted) { resolve(); return; }
      const r = el.getBoundingClientRect();
      const key = `${r.top},${r.left},${r.width},${r.height}`;
      stableCount = key === lastKey ? stableCount + 1 : 0;
      lastKey = key;
      frame += 1;
      if (stableCount >= RECT_SETTLE_STABLE_FRAMES || frame >= RECT_SETTLE_MAX_FRAMES) { resolve(); return; }
      requestAnimationFrame(tick);
    }
    requestAnimationFrame(tick);
  });
}

/**
 * @param {{
 *   frame: HTMLIFrameElement,
 *   frameWrap: HTMLElement,
 *   pointerEl: HTMLElement,
 *   getEditor: () => object | null,
 * }} deps
 */
export function createDriver({ frame, frameWrap, pointerEl, getEditor }) {
  // `pointerEl` must already be un-hidden when this runs: `hidden` renders as `display:none` (the
  // UA default, never overridden here), and `offsetWidth` reads 0 for a display:none element — so
  // calling this while still hidden silently reads `size` as 0 below, which doesn't fail loudly,
  // it just offsets the glide target by +size/2 in both axes (the pointer's TOP-LEFT corner lands
  // where its CENTER should have). Found on the collapse-tree scene: confirmed with a temporary
  // instrumentation dump of this function's own inputs against a real build, size read 0 and the
  // pointer landed ~9px below-and-right of the tree divider's true center — invisible on a normal
  // button-sized target (9px inside a much bigger hit box) but enough to miss the divider, only
  // 12px tall. glideTo (below) now un-hides before calling this.
  function pointerTargetXY(el) {
    const frameRect = frame.getBoundingClientRect();
    const hostRect = frameWrap.getBoundingClientRect();
    const targetRect = el.getBoundingClientRect();
    // CSS owns the pointer's size (moss-stage.css); measuring it here keeps that the only place
    // that does, instead of a second constant here that CSS could silently drift from.
    const size = pointerEl.offsetWidth;
    return {
      x: frameRect.left - hostRect.left + targetRect.left + targetRect.width / 2 - size / 2,
      y: frameRect.top - hostRect.top + targetRect.top + targetRect.height / 2 - size / 2,
    };
  }

  // The iframe-local center of `el`, for a real MouseEvent dispatched inside the harvested
  // document — a different coordinate space from pointerTargetXY above, which positions the host
  // page's own pointer overlay relative to frameWrap instead.
  function centerOf(el) {
    const rect = el.getBoundingClientRect();
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  }

  function hidePointer() {
    pointerEl.hidden = true;
  }

  // The pointer's last animated-to position, in frameWrap-local px — parsed back out of its own
  // inline transform (glideTo's `from`) rather than tracked as separate state, so there is exactly
  // one place (the transform) that can disagree with itself. Used only to scale travel duration by
  // distance (stage/README.md, "Pace"); defaults to (0, 0), the pointer's initial position.
  function currentPointerXY() {
    const match = /translate\(([-\d.]+)px,\s*([-\d.]+)px\)/.exec(pointerEl.style.transform || '');
    return match ? { x: parseFloat(match[1]), y: parseFloat(match[2]) } : { x: 0, y: 0 };
  }

  /** Glides the pointer to `el`'s center, scaled and eased per PACE.travel* (stage/README.md,
   * "Pace"). The Web Animations API, not a CSS transition plus a matching sleep, is the pointer's
   * one owner of how long that takes: the click below awaits the animation's own `.finished`
   * instead of racing a separately-tracked duration. Resolves false, without moving the pointer to
   * its target position, if `signal` aborts mid-glide. */
  async function glideTo(el, signal) {
    // Un-hide before measuring the target (pointerTargetXY's own comment): reading
    // pointerEl.offsetWidth while still `hidden` (display:none) would read 0.
    pointerEl.hidden = false;
    const { x, y } = pointerTargetXY(el);
    const from = currentPointerXY();
    const distance = Math.hypot(x - from.x, y - from.y);
    const animation = pointerEl.animate(
      [{ transform: `translate(${from.x}px, ${from.y}px)` }, { transform: `translate(${x}px, ${y}px)` }],
      { duration: travelDurationMs(distance), easing: 'ease-in-out', fill: 'forwards' },
    );
    const onAbort = () => animation.cancel();
    signal?.addEventListener('abort', onAbort, { once: true });
    try {
      await animation.finished;
    } catch {
      return false; // cancelled mid-glide
    } finally {
      signal?.removeEventListener('abort', onAbort);
    }
    animation.commitStyles();
    animation.cancel(); // release the effect now that the inline style carries the final position
    return true;
  }

  /** Waits out the dwell before a press (stage/README.md, "Pace") — longer before a right-click,
   * whose result is less expected. Resolves false, same convention as every other step here, if
   * `signal` aborts mid-dwell. */
  function dwell(button, signal) {
    return sleep(dwellMs(button), signal);
  }

  // The dip/ring Animation objects the last press() call created (Web Animations API, from
  // dot.animate()/ring.animate() below) — tracked explicitly so a back-to-back press can cancel
  // exactly those two rather than querying `element.getAnimations()`, which returns EVERY
  // animation affecting the element, including the declarative CSS `moss-stage-hollow` keyframe a
  // right-click's own `data-button="right"` mutation (below) just triggered on the same `dot`.
  // Calling `.cancel()` on that CSSAnimation object the instant it starts is what silently killed
  // the hollow look entirely (confirmed with an instrumented `getAnimations()`/MutationObserver
  // probe in both engines: the hollow keyframe reports `playState: "running"` one synchronous
  // call after `data-button` is set, then never renders a single hollow frame afterwards) — not
  // "too early", but never at all, which is what an owner watching for it reports as "too early"
  // (no hollow at the press they expected, so the next thing they see — the solid dot already
  // mid-glide toward the NEXT target — reads as the look never having arrived on time). Tracking
  // our own two animations here fixes that without weakening the "a still-running previous press
  // is cancelled first" guarantee the comment below still describes.
  let activeDip = null;
  let activeGlow = null;

  /** Presses the pointer (stage/README.md, "The pointer"): records which button on the pointer
   * element itself (`data-button="left"|"right"`) — moss-stage.css keys every visual treatment,
   * including a right-click’s hollow dotted dot, off that attribute alone. The attribute is cleared and reflowed before
   * being reapplied so that CSS keyframe restarts
   * even when back-to-back presses use the same button, which a bare reassignment to an unchanged
   * attribute value would not trigger. Restarts the dot's dip (never a grow, only ~88% and back)
   * and the ring's expand-and-fade via the Web Animations API; a still-running previous press is
   * cancelled first so back-to-back presses each get a full animation instead of blending into
   * one. Resolves once the dip completes (PACE.pressDipMs) — the ring keeps fading in the
   * background through the hold that follows (stage/README.md, "Pace": "during the hold"), so this
   * does not block on it. An abort cancels both animations early rather than leaving them running
   * past the load that started them. */
  function press(button, signal) {
    // The right-click look is a CSS keyframe keyed off
    // `[data-button="right"]` and only restarts on a genuine attribute mutation — reassigning the
    // SAME value again is a no-op the browser doesn't restyle for. Only that one case (two
    // right-clicks back to back) needs the clear-and-reflow trick; every other press already
    // mutates the attribute on its own (including left, which no CSS animation keys off at all),
    // so this stays a single DOM mutation per press there — load-bearing for
    // checkVersionsAfterChainAndPointer's own press count in check-docs-stage.mjs, which counts
    // one MutationObserver record per press.
    if (button === 'right' && pointerEl.dataset.button === 'right') {
      pointerEl.removeAttribute('data-button');
      void pointerEl.offsetWidth;
    }
    pointerEl.dataset.button = button;
    const dot = pointerEl.querySelector('.moss-stage__pointer-dot');
    const ring = pointerEl.querySelector('.moss-stage__pointer-ring');
    // Cancel only the WAAPI animations THIS module created for the previous press — never
    // `dot.getAnimations()` (see the comment on activeDip/activeGlow above).
    activeDip?.cancel();
    activeGlow?.cancel();
    const dip = dot.animate(
      [{ transform: 'scale(1)' }, { transform: `scale(${PRESS_DIP_SCALE})`, offset: 0.35 }, { transform: 'scale(1)' }],
      { duration: PACE.pressDipMs, easing: 'ease-out' },
    );
    const glow = ring.animate(
      [{ transform: 'scale(1)', opacity: 0.7 }, { transform: `scale(${RING_EXPAND_SCALE})`, opacity: 0 }],
      { duration: PACE.pressRingMs, easing: 'ease-out' },
    );
    activeDip = dip;
    activeGlow = glow;
    return new Promise((resolve) => {
      const onAbort = () => { dip.cancel(); glow.cancel(); resolve(false); };
      signal?.addEventListener('abort', onAbort, { once: true });
      dip.finished.then(() => {
        signal?.removeEventListener('abort', onAbort);
        resolve(true);
      }, () => {}); // dip.cancel() (onAbort) already resolved false; swallow the resulting rejection
    });
  }

  /** Holds on the result once a step's press has fired (stage/README.md, "Pace"), scaled by the
   * step's own `reveals`. Resolves false, same convention as every other step here, if `signal`
   * aborts mid-hold. */
  function hold(reveals, signal) {
    return sleep(holdAfterResultMs(reveals), signal);
  }

  /** Dispatches a real `contextmenu` MouseEvent at `el`'s center — how the harvested app's own
   * menus open (gestures.md); a synthetic `.click()` has no right-click equivalent to call. */
  function dispatchContextMenu(el) {
    const { x, y } = centerOf(el);
    el.dispatchEvent(new MouseEvent('contextmenu', {
      bubbles: true, cancelable: true, clientX: x, clientY: y, button: 2,
    }));
  }

  /** Dispatches a real `dblclick` MouseEvent at `el`'s center (`detail: 2`, bubbling) — how the
   * harvested app's tree divider collapses/reopens the tree (gestures.md, "Collapse the tree"),
   * which listens for `dblclick` directly rather than counting `click`'s own `detail`. Tried
   * without any preceding `click` events first (checked interactively against `#divider`'s own
   * listener, which only reads the `dblclick` event); it worked, so none are dispatched here —
   * `.click()` synthesizes `detail: 0`, not the `1` then `2` a real double-click's own two clicks
   * carry, and building that pair only to throw it away would be unearned complexity for a target
   * that never reads it (stage/README.md's own module boundary: script only the gesture the
   * harvested interface really performs). */
  function dispatchDblClick(el) {
    const { x, y } = centerOf(el);
    el.dispatchEvent(new MouseEvent('dblclick', {
      bubbles: true, cancelable: true, clientX: x, clientY: y, detail: 2,
    }));
  }

  /** The one press-sequence every click/context/dblclick verb and its instant twin is built from:
   * find the named target, then glide → dwell → press(es) → the real DOM event → hold on the
   * result (stage/README.md, "Pace") — or, when `instant` is set (reduced motion, or every scene
   * but the last in an `after` chain), skip straight from finding the target to `dispatch` with no
   * animation at all, same as every `*Instant` method did before this was one function. `presses`
   * (default 1) inserts `PACE.dblclickGapMs` between presses when there is more than one, so a
   * double-click keeps each press's own dip/ring rather than blurring into one (stage/README.md,
   * "The pointer"). `dispatch` fires at the moment the real gesture it stands for would: before
   * the sole press for a single click or right-click (matching `click`/`context`'s own event,
   * which the browser fires the same moment as the visual press), or once after both presses for a
   * double-click (a real `dblclick` event only fires after the second physical click — see the
   * call sites' own comments for the target-specific event constructors). Every animated phase is
   * abortable on its own, exactly as before extraction; the instant path only ever refuses via the
   * signal already being aborted when the animated path's own callers check it. */
  async function pressSequence(name, { button, presses = 1, dispatch, signal, reveals, instant = false } = {}) {
    if (!instant && signal?.aborted) return false;
    const el = await awaitTarget(findTarget, frame.contentDocument, name, signal);
    if (!el) return false;
    if (instant) {
      dispatch?.(el);
      return true;
    }
    // The target exists (waitForTarget/awaitTarget above), but a previous step's own effect may
    // still be moving it into its final position (waitForRectStable's own comment) — settle
    // before glideTo takes its one-time reading of where to send the pointer.
    await waitForRectStable(el, signal);
    if (signal?.aborted) { hidePointer(); return false; }
    const arrived = await glideTo(el, signal);
    if (!arrived) { hidePointer(); return false; }
    if (!(await dwell(button, signal))) { hidePointer(); return false; }
    if (presses === 1) dispatch?.(el);
    for (let i = 0; i < presses; i++) {
      const pressed = await press(button, signal);
      if (!pressed) { hidePointer(); return false; }
      if (i < presses - 1 && !(await sleep(PACE.dblclickGapMs, signal))) { hidePointer(); return false; }
    }
    if (presses > 1) dispatch?.(el);
    const held = await hold(reveals, signal);
    hidePointer();
    return held;
  }

  return {
    setText(text) {
      getEditor()?.setDoc(text ?? '');
    },
    // Types into a named target a character at a time, on PACE.typeIntoCharMs's fixed cadence, for
    // the site's own `<input>` fields (a rename prompt, a properties search box) that aren't the
    // harvested editor's own typing surface — `setText` above only reaches `__editor.setDoc()`.
    // Commits with a real Enter keydown/keyup, since typing a name and pressing Enter is one
    // gesture, not two.
    async typeInto(name, text, { signal } = {}) {
      const el = await awaitTarget(findTarget, frame.contentDocument, name, signal);
      if (!el) return false;
      el.focus();
      el.value = '';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      for (const ch of text) {
        if (signal?.aborted) return false;
        el.value += ch;
        el.dispatchEvent(new Event('input', { bubbles: true }));
        if (!(await sleep(PACE.typeIntoCharMs, signal))) return false;
      }
      const opts = { bubbles: true, cancelable: true, key: 'Enter' };
      el.dispatchEvent(new KeyboardEvent('keydown', opts));
      el.dispatchEvent(new KeyboardEvent('keyup', opts));
      return true;
    },
    // Instant equivalent of typeInto: sets the value and commits in one step, no per-character cadence.
    setTextInto(name, text) {
      const el = findTarget(frame.contentDocument, name);
      if (!el) return;
      el.value = text ?? '';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      const opts = { bubbles: true, cancelable: true, key: 'Enter' };
      el.dispatchEvent(new KeyboardEvent('keydown', opts));
      el.dispatchEvent(new KeyboardEvent('keyup', opts));
    },
    click(name, opts = {}) {
      return pressSequence(name, { ...opts, button: 'left', dispatch: (el) => el.click() });
    },
    clickInstant(name, opts = {}) {
      return pressSequence(name, { ...opts, button: 'left', dispatch: (el) => el.click(), instant: true });
    },
    // A right-click — how the harvested app's own menus open (gestures.md); a synthetic `.click()`
    // has no right-click equivalent to call, so this dispatches a real `contextmenu` MouseEvent at
    // the target's center instead.
    context(name, opts = {}) {
      return pressSequence(name, { ...opts, button: 'right', dispatch: dispatchContextMenu });
    },
    contextInstant(name, opts = {}) {
      return pressSequence(name, { ...opts, button: 'right', dispatch: dispatchContextMenu, instant: true });
    },
    // A double-click — how the harvested app's tree divider collapses/reopens the tree
    // (gestures.md, "Collapse the tree"), which listens for `dblclick` directly rather than
    // counting `click`'s own `detail`. Dispatches a real `dblclick` MouseEvent (`detail: 2`) after
    // both presses; no preceding `click` events are dispatched (checked interactively against
    // `#divider`'s own listener, which only reads `dblclick`). `.click()` synthesizes `detail: 0`,
    // not the `1` then `2` a real double-click's own two clicks carry, and building that pair only
    // to throw it away would be unearned complexity for a target that never reads it.
    dblclick(name, opts = {}) {
      return pressSequence(name, { ...opts, button: 'left', presses: 2, dispatch: dispatchDblClick });
    },
    dblclickInstant(name, opts = {}) {
      return pressSequence(name, { ...opts, dispatch: dispatchDblClick, instant: true });
    },
  };
}
