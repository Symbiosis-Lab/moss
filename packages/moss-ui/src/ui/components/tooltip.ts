/**
 * Tooltip — portal tooltip rendered at document.body level
 *
 * Avoids overflow clipping from ancestor containers by using a single
 * shared tooltip element appended to <body>. Uses event delegation
 * (mouseover/mouseout on document) so dynamically-added elements work
 * without re-initialization.
 *
 * Usage:
 *   <button data-tooltip="Save changes">Save</button>
 *   <button data-tooltip="Muted hint" data-tooltip-style="muted">Info</button>
 *   <button data-tooltip="Bold" data-tooltip-kbd="⌘B">B</button>
 *
 * Call initTooltips() once at startup, destroyTooltips() on teardown.
 */

import { zTier } from "../shared/z-tier";

/** The requestable placements (`data-tooltip-position=`; absent = "left"). */
export const TOOLTIP_PLACEMENTS = ["left", "right", "top", "bottom"] as const;
export type TooltipPlacement = (typeof TOOLTIP_PLACEMENTS)[number];

export interface TooltipPosition {
  top: number;
  left: number;
  /**
   * Side of the trigger where the tooltip is placed — CSS class `arrow-{side}`
   * — or `null` when the clamp moved the box off the trigger, in which case no
   * class is added and the arrow (a transparent border until a class colours
   * one edge) is simply not drawn.
   *
   * The arrow's cross-axis is pinned to the box edge that faces the trigger
   * (`left: 100%`, `top: 100%`, …), which describes the trigger only while
   * that edge is still `GAP` away from it. The main-axis clamp breaks exactly
   * that: a `left`-placed tooltip in a narrow panel flips right and is then
   * pulled back until the box can cover its own trigger. An absent arrow tells
   * no lie; one pinned to a corner of a box that overlaps its trigger does.
   */
  arrowSide: TooltipPlacement | null;
  /**
   * Where the arrow sits along the tooltip's own edge, measured in px from the
   * box's start corner — x for the top/bottom placements, y for left/right.
   * `show()` writes it to `--moss-tooltip-arrow`, which tooltip.css reads.
   */
  arrowOffset: number;
}

const GAP = 8;

/**
 * Breathing room reserved from the edges of the viewport/webview the tooltip
 * is clamped against: 8, the same as `ctx-menu.ts`'s `VIEWPORT_EDGE_PADDING`
 * and the editor's slash menu. `offsetWidth`/`offsetHeight` (what
 * `tooltipRect` is built from) don't count the CSS box-shadow bleed or the
 * arrow pseudo-element, both of which paint a few px past the measured box —
 * a clamp with zero margin lets that bleed land outside the editor webview's
 * own OS rectangle, where there is no canvas for it to paint into.
 */
const EDGE_MARGIN = 8;

/**
 * How close the arrow may come to a corner of the tooltip, in px: its own
 * half-width (border: 4px) plus the box's 4px corner radius, so the whole
 * triangle still meets a straight edge. Both numbers are tooltip.css's.
 */
const ARROW_INSET = 8;

/**
 * Where the arrow lands along the edge it sits on, given the trigger's centre
 * and the box the clamp actually produced.
 *
 * The box centres on the trigger and is THEN clamped into the visible band, so
 * "half the tooltip" only names the trigger when the clamp did not fire. Every
 * trigger within half a tooltip-width of an edge would get an arrow pointing at
 * empty space, and the narrower the surface the more triggers qualify: the
 * action panel's ＋ button sat 82px adrift in a 280px panel.
 * Measuring the offset is the same answer `crates/moss-build/src/js-src/site/nav/hint-place.ts`
 * reached for the built site's hint pill, whose header names the general
 * lesson — a static anchor on a measured box is wrong somewhere for every
 * static choice.
 *
 * A trigger the box cannot reach pins the arrow to the near corner: the
 * closest true thing it can say, and what a popover library does with an arrow
 * it cannot fully place. The two insets cannot cross on a laid-out tooltip —
 * `padding: 4px 8px` alone puts both axes past 16px.
 */
function arrowOffset(triggerCenter: number, boxStart: number, boxExtent: number): number {
  return Math.min(Math.max(triggerCenter - boxStart, ARROW_INSET), boxExtent - ARROW_INSET);
}

/**
 * Whether the box still sits wholly on the side of the trigger its arrow
 * claims — the only condition under which that arrow tells the truth, since
 * the arrow is pinned to the box edge facing the trigger.
 *
 * Asked of the trigger, not of the pre-clamp wish. Comparing the clamped
 * coordinate to the wanted one calls ANY movement a detachment, which drops a
 * true arrow for a clamp shorter than `GAP`: a "top" placement with no room
 * above flips below, and a bottom clamp landing anywhere in the 8px between
 * the trigger and the wished-for gap leaves the box correctly below with a
 * tighter gap — and no arrow.
 */
function clearsTrigger(
  boxStart: number,
  boxExtent: number,
  triggerStart: number,
  triggerEnd: number,
  before: boolean
): boolean {
  return before ? boxStart + boxExtent <= triggerStart : boxStart >= triggerEnd;
}

/**
 * The width the tooltip must stay inside — the VISIBLE band, not the webview
 * viewport.
 *
 * At rest the preview webview keeps its full window width and is shifted right,
 * so only the left `100% - --preview-width-inset` is ever on screen; `body` is
 * narrowed to that band and `contain: layout` (the app shell's stylesheet) makes it the
 * containing block for this fixed-position tooltip. Clamping against
 * `window.innerWidth` therefore permits a `left` up to `inset` px into the strip
 * that runs off-screen — the tooltip is "on screen" by the numbers and invisible
 * in fact. `body.clientWidth` is the band, and it is also the box the `left` we
 * write is measured from, so the two agree by construction.
 *
 * Falls back to `innerWidth` for the editor webview and for tests, where body
 * has no inset and may report 0 before layout.
 */
function containerWidth(): number {
  return document.body?.clientWidth || window.innerWidth;
}

/** The tier a trigger already sits at, read off the live cascade rather than
 * a parallel table: tokens.css's --z-* scale is the one registry, and every
 * surface that participates already sets its own z-index from it, so this
 * only ever reads that, never restates it. */
function ambientTier(el: Element): number {
  let node: Element | null = el;
  while (node && node !== document.documentElement) {
    const z = getComputedStyle(node).zIndex;
    if (z !== "auto") {
      const n = parseInt(z, 10);
      if (!Number.isNaN(n)) return n;
    }
    node = node.parentElement;
  }
  return 0;
}

/** Pure function for unit-testable positioning logic. */
export function computeTooltipPosition(
  triggerRect: DOMRect,
  tooltipRect: { width: number; height: number },
  position: TooltipPlacement,
  viewport: { width: number; height: number }
): TooltipPosition {
  // "top"/"bottom": centered on the trigger, flipping to the other side when
  // there is no room. The placements for buttons in a HORIZONTAL row (the
  // insert bar, the chip bar, titlebar clusters) — a side-placed tooltip there
  // covers the neighbouring buttons. Pick the one pointing AWAY from the
  // nearer chrome edge: bottom for top-of-surface rows, top for bottom bars.
  if (position === "top" || position === "bottom") {
    let left = triggerRect.left + triggerRect.width / 2 - tooltipRect.width / 2;
    if (left + tooltipRect.width > viewport.width - EDGE_MARGIN) {
      left = viewport.width - EDGE_MARGIN - tooltipRect.width;
    }
    if (left < EDGE_MARGIN) left = EDGE_MARGIN;

    const above = triggerRect.top - GAP - tooltipRect.height;
    const below = triggerRect.bottom + GAP;
    // The preferred side wins unless it lacks room; then flip.
    const useAbove = position === "top"
      ? above >= EDGE_MARGIN
      : below + tooltipRect.height > viewport.height - EDGE_MARGIN;
    let top = useAbove ? above : below;
    if (top + tooltipRect.height > viewport.height - EDGE_MARGIN) {
      top = viewport.height - EDGE_MARGIN - tooltipRect.height;
    }
    if (top < EDGE_MARGIN) top = EDGE_MARGIN;
    return {
      top,
      left,
      arrowSide: clearsTrigger(top, tooltipRect.height, triggerRect.top, triggerRect.bottom, useAbove)
        ? (useAbove ? "top" : "bottom")
        : null,
      arrowOffset: arrowOffset(triggerRect.left + triggerRect.width / 2, left, tooltipRect.width),
    };
  }

  let left: number;
  let arrowSide: "left" | "right";

  if (position === "left") {
    left = triggerRect.left - GAP - tooltipRect.width;
    arrowSide = "left";

    // Flip to right if off-screen left (reserving EDGE_MARGIN — see its doc
    // comment for why zero margin lets the arrow/shadow bleed past the edge).
    if (left < EDGE_MARGIN) {
      left = triggerRect.right + GAP;
      arrowSide = "right";
    }
  } else {
    left = triggerRect.right + GAP;
    arrowSide = "right";

    // Flip to left if off-screen right
    if (left + tooltipRect.width > viewport.width - EDGE_MARGIN) {
      left = triggerRect.left - GAP - tooltipRect.width;
      arrowSide = "left";
    }
  }

  // Clamp horizontally to the viewport, reserving EDGE_MARGIN on each edge. A
  // flip (left→right or right→left) can still overflow the OPPOSITE edge in a
  // narrow container (e.g. a chip near the left of the editor panel with a
  // wide tooltip flips right and runs off the panel's right edge). Clamp the
  // right overflow first so left wins when the tooltip is wider than the
  // viewport — the vertical clamp below does the same, bottom first.
  if (left + tooltipRect.width > viewport.width - EDGE_MARGIN) {
    left = viewport.width - EDGE_MARGIN - tooltipRect.width;
  }
  if (left < EDGE_MARGIN) left = EDGE_MARGIN;

  // Vertically center on trigger
  let top =
    triggerRect.top + triggerRect.height / 2 - tooltipRect.height / 2;

  // Clamp to viewport, reserving EDGE_MARGIN on each edge. Bottom overflow
  // first, so TOP wins when the tooltip is taller than the viewport — the
  // same precedence as the horizontal clamp above, and for the same reason:
  // whichever edge is clamped last is the one that survives, and the text
  // starts at the top-left. Losing the tail of a too-tall tooltip is
  // recoverable; losing its first line is not.
  if (top + tooltipRect.height > viewport.height - EDGE_MARGIN) {
    top = viewport.height - EDGE_MARGIN - tooltipRect.height;
  }
  if (top < EDGE_MARGIN) top = EDGE_MARGIN;

  return {
    top,
    left,
    // The flip above already answered "which side fits"; this asks the later
    // question of whether the clamp then moved the box off that side.
    arrowSide: clearsTrigger(
      left,
      tooltipRect.width,
      triggerRect.left,
      triggerRect.right,
      arrowSide === "left"
    )
      ? arrowSide
      : null,
    arrowOffset: arrowOffset(triggerRect.top + triggerRect.height / 2, top, tooltipRect.height),
  };
}

// ---------------------------------------------------------------------------
// Singleton state
// ---------------------------------------------------------------------------

/** Stable id assigned to the singleton tooltipEl at creation time. */
const TOOLTIP_ID = "moss-tooltip-singleton";

let tooltipEl: HTMLDivElement | null = null;
let currentTrigger: Element | null = null;

/**
 * How many surfaces currently own the screen. `show()` is inert while this is
 * above zero — see `suppressTooltips`.
 */
/**
 * Surfaces currently on screen, each with the elements it paints over the
 * page. A Set rather than a counter: a release must be idempotent AND must
 * speak only for its own surface, and both fall out of `delete` for free.
 * A counter cannot say either — `destroyTooltips()` resetting it to zero
 * while a surface still held one would hand that surface's debt to whoever
 * released next.
 */
const suppressors = new Set<{ root: readonly Element[]; tier: number }>();

/**
 * Whether `trigger` sits inside a surface that is currently up. Triggers
 * INSIDE the covering surface keep their tooltips — the surface is not
 * covering them. A trigger OUTSIDE it is still spared when its own ambient
 * tier already outranks the surface's (see `suppressTooltips`): a toast's own
 * tier (200) already exceeds a modal's (51), so no call site needs to exempt
 * toasts by name.
 */
function suppressed(trigger: Element): boolean {
  const triggerTier = ambientTier(trigger);
  for (const s of suppressors) {
    if (s.root.some((el) => el === trigger || el.contains(trigger))) continue;
    if (triggerTier > s.tier) continue; // '>' not '>=' — see tooltip.test.ts, the zero-tier tie must stay suppressed
    return true;
  }
  return false;
}

// Watches the shown trigger's `data-tooltip` attribute so text that changes
// WHILE the pointer stays put (a live progress percentage, a streaming
// status line) doesn't freeze at whatever it read on mouseover.
let attrObserver: MutationObserver | null = null;

// Store handler references so we can removeEventListener on destroy
let handleMouseOver: ((e: Event) => void) | null = null;
let handleMouseOut: ((e: Event) => void) | null = null;
let handleFocusIn: ((e: Event) => void) | null = null;
let handleFocusOut: ((e: Event) => void) | null = null;
let handleDismiss: (() => void) | null = null;
let handleKeyDown: ((e: Event) => void) | null = null;

/**
 * A tooltip closes with the surface it was opened from. Owners hide in ways
 * no event reports: a class fading a bar to opacity 0 under a resting
 * pointer, a re-render removing the trigger. So while a tooltip shows, each
 * frame looks at its trigger, and the tooltip goes when the trigger is
 * detached, or stops being painted or live after having been both: a trigger
 * caught mid-entrance at opacity 0 is not a closing owner.
 */
let ownerWatch = 0;

function triggerLive(trigger: Element): boolean {
  if (trigger.getClientRects().length === 0) return false;
  if (getComputedStyle(trigger).pointerEvents === "none") return false;
  const check = (trigger as Element & { checkVisibility?: (o: object) => boolean }).checkVisibility;
  return check ? check.call(trigger, { opacityProperty: true, visibilityProperty: true }) : true;
}

function watchOwner(trigger: Element) {
  cancelAnimationFrame(ownerWatch);
  let seenLive = false;
  const tick = () => {
    if (currentTrigger !== trigger) return;
    const live = trigger.isConnected && triggerLive(trigger);
    if (!trigger.isConnected || (seenLive && !live)) { hide(); return; }
    seenLive ||= live;
    ownerWatch = requestAnimationFrame(tick);
  };
  ownerWatch = requestAnimationFrame(tick);
}

function hide() {
  cancelAnimationFrame(ownerWatch);
  if (tooltipEl) {
    tooltipEl.classList.remove("visible", "muted", ...TOOLTIP_PLACEMENTS.map((p) => `arrow-${p}`));
  }
  // Remove aria-describedby from the previous trigger on hide/retarget.
  if (currentTrigger) {
    currentTrigger.removeAttribute("aria-describedby");
  }
  currentTrigger = null;
  attrObserver?.disconnect();
}

/** Paint the trigger's text (and optional `data-tooltip-kbd` shortcut chip)
 * into the tooltip. The plain path stays textContent — the chip is built as a
 * DOM node, never via innerHTML, so attribute text is inert either way. */
function renderContent(trigger: Element, text: string) {
  if (!tooltipEl) return;
  tooltipEl.textContent = text;
  const kbd = trigger.getAttribute("data-tooltip-kbd");
  if (kbd) {
    const chip = document.createElement("kbd");
    chip.className = "moss-tooltip-kbd";
    chip.textContent = kbd;
    tooltipEl.appendChild(chip);
  }
}

/** Re-read `data-tooltip`/`data-tooltip-kbd` from the currently-shown trigger
 * and repaint in place, without moving or re-showing the tooltip. */
function refreshText(trigger: Element) {
  if (!tooltipEl) return;
  const text = trigger.getAttribute("data-tooltip");
  if (!text) {
    hide();
    return;
  }
  renderContent(trigger, text);
}

function show(trigger: Element) {
  if (!tooltipEl || suppressed(trigger)) return;

  const text = trigger.getAttribute("data-tooltip");
  if (!text) return;

  // If retargeting from one trigger to another, remove aria-describedby from
  // the previous trigger before setting currentTrigger to the new one.
  if (currentTrigger && currentTrigger !== trigger) {
    currentTrigger.removeAttribute("aria-describedby");
  }

  currentTrigger = trigger;
  renderContent(trigger, text);

  // Set aria-describedby on the trigger pointing to the stable singleton id.
  trigger.setAttribute("aria-describedby", TOOLTIP_ID);

  // Style variant
  const style = trigger.getAttribute("data-tooltip-style");
  tooltipEl.classList.toggle("muted", style === "muted");

  // Position — read preferred side from data attribute, default LEFT
  const triggerRect = trigger.getBoundingClientRect();
  const pos = computeTooltipPosition(
    triggerRect,
    { width: tooltipEl.offsetWidth, height: tooltipEl.offsetHeight },
    TOOLTIP_PLACEMENTS.find((p) => p === trigger.getAttribute("data-tooltip-position")) ?? "left",
    // Width is the visible band (see containerWidth); height is unaffected —
    // the inset is horizontal only, and body's height is content-driven, so
    // clientHeight is not a trustworthy stand-in for the viewport here.
    { width: containerWidth(), height: window.innerHeight }
  );

  tooltipEl.style.top = `${pos.top}px`;
  tooltipEl.style.left = `${pos.left}px`;
  // Read-through elevation: paint above whatever the trigger's own ancestry
  // already sits at, so a tooltip whose trigger lives inside a toast/overlay
  // still outranks that surface instead of being buried under its fixed
  // --z-popup floor.
  tooltipEl.style.zIndex = String(Math.max(zTier("--z-popup", 100), ambientTier(trigger) + 1));
  tooltipEl.style.setProperty("--moss-tooltip-arrow", `${pos.arrowOffset}px`);
  tooltipEl.classList.remove(...TOOLTIP_PLACEMENTS.map((p) => `arrow-${p}`));
  tooltipEl.classList.add("visible");
  if (pos.arrowSide) tooltipEl.classList.add(`arrow-${pos.arrowSide}`);

  attrObserver?.disconnect();
  attrObserver = new MutationObserver(() => refreshText(trigger));
  attrObserver.observe(trigger, { attributes: true, attributeFilter: ["data-tooltip", "data-tooltip-kbd"] });
  watchOwner(trigger);
}

/**
 * Hold tooltips off while `root` covers the page, hiding any tooltip already
 * up outside it. Returns the release, which is idempotent.
 *
 * Scoped, not global: a tooltip on a trigger INSIDE the surface is not being
 * covered by it, and muting those deletes shipped behaviour. `copy-button.ts`
 * in icon-only mode carries its whole visible name in `data-tooltip`
 * (`aria-label` is screen-reader-only by that file's own note), and delivers
 * its "Copied" acknowledgement by swapping the attribute and letting the
 * live-refresh observer repaint the showing pill. Both ship inside
 * two modals via `createCodeWithCopy`, and a global mute would leave that
 * button nameless and its ack unpainted. The settings modal is thick with the
 * same pattern — a glyph-only catalog tab, the 8px deploy dot, the plugin
 * install progress that has no other home.
 *
 * There is deliberately no "a press hides the tooltip" rule to go with this,
 * standard though that is elsewhere: it would delete that same ack. A surface that opens on a press therefore needs its own
 * call here — it gets no dismissal for free.
 *
 * Why yield at all: a tooltip is the least important thing on screen, and the
 * z-scale sorts it against these surfaces inconsistently in both directions —
 * `.ctx-menu` (`--z-toast` + 50) and the `.pop`/`.omenu`/`.dropdown` tier
 * (`--z-overlay`) bury a `--z-popup` tooltip, while modal content
 * (`--z-modal` + 1) ends up under one. Raising the tooltip above them would
 * answer "the hint is behind the menu" by putting the hint in front of the
 * menu.
 */
export function suppressTooltips(root: Element | readonly Element[]): () => void {
  const roots = Array.isArray(root) ? root : [root as Element];
  // The surface's own ambient tier, read once at arm time — this is what lets
  // `suppressed()` spare a trigger that already outranks the surface (a
  // toast's tooltip staying up while an unrelated modal opens) without a
  // per-surface exemption list.
  const token = { root: roots, tier: Math.max(0, ...roots.map(ambientTier)) };
  suppressors.add(token);
  // Only the tooltip this surface actually covers goes; one on a trigger
  // inside it stays up.
  if (currentTrigger && suppressed(currentTrigger)) hide();
  return () => {
    suppressors.delete(token);
  };
}

/** Create the singleton tooltip element and attach event listeners. */
export function initTooltips(): void {
  // Idempotent — don't create duplicates
  if (tooltipEl) return;

  tooltipEl = document.createElement("div");
  tooltipEl.className = "moss-tooltip";
  // Assign a stable id so aria-describedby can reference it.
  tooltipEl.id = TOOLTIP_ID;
  document.body.appendChild(tooltipEl);

  handleMouseOver = (e: Event) => {
    const target = (e as MouseEvent).target;
    if (!(target instanceof Element)) return;

    const trigger = target.closest("[data-tooltip]");
    if (!trigger) return;

    show(trigger);
  };

  handleMouseOut = (e: Event) => {
    if (!currentTrigger) return;

    const related = (e as MouseEvent).relatedTarget;

    // If pointer moved to a child of the current trigger, keep showing
    if (
      related instanceof Node &&
      currentTrigger.contains(related)
    ) {
      return;
    }

    hide();
  };

  // Mirror mouseover/mouseout with focusin/focusout for keyboard accessibility.
  handleFocusIn = (e: Event) => {
    const target = (e as FocusEvent).target;
    if (!(target instanceof Element)) return;

    const trigger = target.closest("[data-tooltip]");
    if (!trigger) return;

    show(trigger);
  };

  handleFocusOut = (e: Event) => {
    if (!currentTrigger) return;
    hide();
  };

  // Escape dismisses the tooltip (keyboard accessibility).
  handleKeyDown = (e: Event) => {
    if ((e as KeyboardEvent).key === "Escape") {
      hide();
    }
  };

  handleDismiss = () => hide();

  document.addEventListener("mouseover", handleMouseOver, true);
  document.addEventListener("mouseout", handleMouseOut, true);
  document.addEventListener("focusin", handleFocusIn, true);
  document.addEventListener("focusout", handleFocusOut, true);
  document.addEventListener("keydown", handleKeyDown, true);
  document.addEventListener("scroll", handleDismiss, true);
  window.addEventListener("resize", handleDismiss);
}

/** Remove tooltip element and detach all event listeners. */
export function destroyTooltips(): void {
  if (tooltipEl && tooltipEl.parentNode) {
    tooltipEl.parentNode.removeChild(tooltipEl);
  }
  // Remove aria-describedby from any trigger that still has it.
  if (currentTrigger) {
    currentTrigger.removeAttribute("aria-describedby");
  }
  tooltipEl = null;
  currentTrigger = null;
  attrObserver?.disconnect();
  attrObserver = null;

  if (handleMouseOver) {
    document.removeEventListener("mouseover", handleMouseOver, true);
    handleMouseOver = null;
  }
  if (handleMouseOut) {
    document.removeEventListener("mouseout", handleMouseOut, true);
    handleMouseOut = null;
  }
  if (handleFocusIn) {
    document.removeEventListener("focusin", handleFocusIn, true);
    handleFocusIn = null;
  }
  if (handleFocusOut) {
    document.removeEventListener("focusout", handleFocusOut, true);
    handleFocusOut = null;
  }
  if (handleKeyDown) {
    document.removeEventListener("keydown", handleKeyDown, true);
    handleKeyDown = null;
  }
  if (handleDismiss) {
    document.removeEventListener("scroll", handleDismiss, true);
    window.removeEventListener("resize", handleDismiss);
    handleDismiss = null;
  }
  // Teardown of the whole system: a surface that outlives it must not hold
  // the next init hostage. Its release stays safe — deleting a token twice
  // is a no-op, and it can no longer speak for a surface it does not own.
  suppressors.clear();
}
