/**
 * The adaptive breadcrumb spine: measure the segments, allocate their widths,
 * and publish the offset vector that tree-row placement consumes.
 *
 * This is one record with one lifetime — mounted or not — replacing six fields
 * that lived on the tree and were torn down together but declared apart. It
 * moved as a record rather than as an algorithm-plus-fields precisely so the
 * teardown stays in one place; splitting it the other way would have made
 * `dispose()` a cross-file obligation.
 *
 * The tooltip sync lives here because it is a CONSEQUENCE of allocation, not a
 * separate concern: whether a label ellipsizes is knowable only after this
 * class has pinned each segment's width, and an iconified segment is one this
 * class demoted.
 *
 * What stays with the tree is everything downstream of the offsets — row
 * re-placement, home-pair alignment, connector rails. This owns the numbers;
 * the tree owns what it draws with them.
 */

/** Fixed indent step for depths past the deepest meaningful spine column. */
const STRUCTURAL_INDENT = 28;

/** Absolute per-segment cap. Disabled (Infinity): the proportional cap
 *  (CAP_FRACTION=0.45) already stops one ancestor ballooning, while a fixed
 *  533px ceiling truncated segments on any wide layout for no gain. */
const SEG_CAP_ABS = Infinity;

/**
 * Watchdog window (ms) for the debounce timer. WKWebView can starve an
 * ALREADY-ARMED `setTimeout` indefinitely while the action-panel webview is
 * not the OS-focused one — the normal case during a divider drag (verified
 * live: one stalled callback took 42 SECONDS). ResizeObserver delivery is
 * never suspended in the same stall, so a pending timer older than this window
 * is treated as starved rather than imminent. Far above a healthy coalescing
 * tick (<<1ms), far below anything a user perceives.
 */
const TIMER_STALL_MS = 100;

/**
 * Delay (ms) for the one guaranteed post-mount reallocation. Chosen to outlast
 * the Tauri `set_size()` IPC roundtrip (~10ms on localhost) plus the
 * ResizeObserver delivery window.
 */
const INIT_SETTLE_MS = 100;

/** Read a numeric CSS custom property (px) off a resolved style, or `dflt`.
 *  NOTE: pass a property whose value is a CONCRETE length, not an aliased
 *  `var(...)`. For an unregistered custom property, getPropertyValue returns
 *  the specified token stream UNSUBSTITUTED, so `parseFloat('var(--x)')` → NaN
 *  → dflt. This is why the spine reads `--moss-traffic-light-inset` (concrete,
 *  inherited) rather than `.nav-grid`'s `--ft-tl` alias. */
export function readPxVar(cs: CSSStyleDeclaration | null, name: string, dflt: number): number {
  const n = cs ? parseFloat(cs.getPropertyValue(name)) : NaN;
  return Number.isFinite(n) ? n : dflt;
}

export class Spine {
  /** Shared rounded prefix-offset array O[] from the allocator (px). */
  private offsets: number[] | null = null;
  /** Hysteresis state carried across resize frames. */
  private state: SpineState = { iconified: 0 };
  private observer: ResizeObserver | null = null;
  /**
   * Debounce handle. Deliberately NOT requestAnimationFrame: the action panel
   * is a Tauri CHILD webview and WKWebView throttles rAF in a webview that is
   * not actively compositing, so a scheduled rAF can sit un-fired indefinitely
   * (verified live: rAF never fired, setTimeout did).
   */
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** `performance.now()` when `timer` was armed — monotonic, so a system clock
   *  adjustment cannot silently defeat the watchdog. */
  private timerArmedAt = 0;
  private initTimer: ReturnType<typeof setTimeout> | null = null;

  /** @param reallocate what the tree does when a re-allocation is due. */
  constructor(private readonly reallocate: () => void) {}

  /**
   * Resolve the inline-start offset for a tree row at `depth`.
   *
   * O[] has one entry per active-path segment plus a terminal O[N] = the LEAF's
   * own content right edge. Rows align to the spine only for INTERMEDIATE
   * columns: using O[N] as a sibling-subtree indent pushes those rows out to
   * the leaf's width instead of one indent step (bug #1 — measured: leaf at
   * ~240px put depth-2 children at 297px). Past the deepest meaningful column
   * we indent STRUCTURALLY. With no allocation yet (jsdom, hidden panel) this
   * degrades to the prior fixed indent.
   */
  offsetAt(depth: number): number {
    const o = this.offsets;
    if (!o || o.length === 0) return depth * STRUCTURAL_INDENT;
    const lastCol = o.length - 2; // deepest real breadcrumb column
    if (lastCol < 0) return depth * STRUCTURAL_INDENT; // degenerate (≤1 seg)
    if (depth <= lastCol) return o[depth];
    return o[lastCol] + (depth - lastCol) * STRUCTURAL_INDENT;
  }

  /** True once an allocation has produced offsets. */
  get hasOffsets(): boolean {
    return this.offsets !== null;
  }

  /** A snapshot of the current offsets, for change detection across a re-run. */
  snapshot(): number[] | null {
    return this.offsets;
  }

  /**
   * Run the allocator against `segEls` and write the result: each segment's
   * inline width (flex:0 0 W), its demotion class, and the offset vector.
   *
   * A 0-width panel (jsdom, detached, hidden) is a no-op that KEEPS the
   * last-good vector, so the tree never consumes garbage offsets.
   */
  apply(strip: HTMLElement, segEls: HTMLElement[]): void {
    const grid = strip.closest('.nav-grid') as HTMLElement | null;
    const panelW = grid?.getBoundingClientRect().width ?? 0;
    if (panelW === 0 || segEls.length === 0) return; // jsdom/hidden → fixed-indent fallback

    const cs = grid ? getComputedStyle(grid) : null;
    const ftTl = readPxVar(cs, '--moss-traffic-light-inset', 76);
    const ftMin = readPxVar(cs, '--ft-seg-min', 72);
    // The titlebar buttons sit ON this row; navReserve says how much they own.
    const navRight = grid?.parentElement?.querySelector('.nav-right') as HTMLElement | null;
    const ftPr = navReserve(navRight?.getBoundingClientRect().width ?? 0);
    // On the STRIP, not the grid: --ft-pr is the strip's padding-inline-end,
    // and the grid is what the ResizeObserver watches.
    strip.style.setProperty('--ft-pr', `${ftPr}px`);
    const L = Math.max(0, panelW - ftTl - ftPr);
    // A degenerate L from a near-zero (collapsing/animating) panel must not
    // reach the allocator: that path writes widths the tree then consumes.
    if (L <= 0) return;

    const iconFloor = segIconFloor(readPxVar(cs, '--ft-base', 8));
    const content = measureSegmentContent(segEls);
    const input = content.map((c) => ({ content: c, floor: ftMin, icon: iconFloor }));
    const result = allocateSpine(L, input, SEG_CAP_ABS, panelW, this.state, this.offsets ?? undefined);
    this.offsets = result.offset;
    this.state = result.state;
    for (let i = 0; i < segEls.length; i++) {
      segEls[i].style.flex = `0 0 ${result.width[i]}px`;
      // The allocator demotes a PREFIX of the row, so index < iconified is the
      // whole membership test. The class hides the label (file-tree.css); the
      // width above already reflects the smaller box.
      segEls[i].classList.toggle('is-iconified', i < result.state.iconified);
    }
  }

  /**
   * Add or remove each segment's `data-tooltip`. A fully-visible segment gets
   * none — echoing its own visible name is noise. An icon-only segment always
   * gets one: it shows none of its name, which is a stronger case than an
   * ellipsized label, and truncation probing cannot see it (a hidden label
   * reports scrollWidth 0). moss convention: `data-tooltip`, never `title`.
   */
  syncTooltips(segEls: HTMLElement[]): void {
    for (const el of segEls) {
      const textEl = el.querySelector<HTMLElement>('.seg-name');
      const full = el.dataset.fullName;
      if (full && el.classList.contains('is-iconified')) el.dataset.tooltip = full;
      else if (textEl && full && isTruncated(textEl)) el.dataset.tooltip = full;
      else delete el.dataset.tooltip;
      // Crumbs run horizontally in the titlebar strip: a side tooltip lands on
      // the neighbouring crumb, and above is the window edge — go below.
      el.dataset.tooltipPosition = 'bottom';
    }
  }

  /** Watch `container` for width changes. Guarded for jsdom. */
  observe(container: HTMLElement): void {
    if (typeof ResizeObserver === 'undefined') return;
    // Custom properties are mutated on DEEPER nodes (segs + rows), never on the
    // observed node, so there is no ResizeObserver feedback loop.
    this.observer = new ResizeObserver(() => this.schedule());
    this.observer.observe(container);
  }

  /**
   * Coalesce ResizeObserver bursts into one re-allocation, with a watchdog.
   *
   * Treating "the handle is truthy" as "a fire is imminent" — the old guard —
   * silently dropped every notification for the whole duration of a WKWebView
   * timer stall. Gating on elapsed wall-clock time instead bounds staleness to
   * TIMER_STALL_MS: a younger pending timer is a normal in-flight debounce, an
   * older one is starved, so cancel it and reallocate now.
   */
  schedule(): void {
    if (this.timer !== null) {
      if (performance.now() - this.timerArmedAt < TIMER_STALL_MS) return;
      clearTimeout(this.timer);
      this.timer = null;
      this.reallocate();
      return;
    }
    this.timerArmedAt = performance.now();
    this.timer = setTimeout(() => {
      this.timer = null;
      this.reallocate();
    }, 0);
  }

  /**
   * Arm the one guaranteed post-mount reallocation. The initial render runs
   * before Tauri's `set_size()` reaches the web content process, so `apply`
   * sees panelW=0 and bails; ResizeObserver is the authoritative reactive path
   * but its notification can race the debounce and be coalesced away. This
   * calls back directly rather than through `schedule()` so the debounce
   * cannot suppress it, and if it fires too early `apply`'s own panelW guard
   * bails safely.
   */
  armInitialSettle(): void {
    this.initTimer = setTimeout(() => {
      this.initTimer = null;
      this.reallocate();
    }, INIT_SETTLE_MS);
  }

  /** Unmount. One teardown for all six, which is why they live together. */
  dispose(): void {
    this.observer?.disconnect();
    this.observer = null;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    this.timerArmedAt = 0;
    if (this.initTimer !== null) clearTimeout(this.initTimer);
    this.initTimer = null;
    this.offsets = null;
    this.state = { iconified: 0 };
  }
}

/**
 * Measure each segment's natural content width via getBoundingClientRect —
 * fractional and authoritative, NOT scrollWidth (integer-truncated, which
 * over-truncates the ellipsis).
 *
 * Measured as the segment WANTS to look, never as it currently looks: an
 * iconified seg hides its label, so leaving the class on reports the icon
 * width as the natural width and a segment that measures 30px never earns its
 * label back, whatever the panel does. Demotion is re-decided from the
 * untouched content width on every pass.
 *
 * Ceiled to whole pixels. The natural width is fractional (56.17px), but the
 * allocator rounds cumulative offsets to the NEAREST integer, which rounds a
 * width DOWN whenever the fraction is < .5. That sub-pixel deficit is enough
 * to trip text-overflow:ellipsis, and because the "…" glyph then needs room it
 * drops whole characters ("site" → "si…") on a wide, mostly-empty panel. Every
 * segment is pinned to exactly this width, so none has sub-pixel headroom —
 * rounding up gives each the one pixel the text needs.
 */
function measureSegmentContent(segEls: HTMLElement[]): number[] {
  for (const el of segEls) {
    el.style.flex = '0 0 auto';
    el.classList.remove('is-iconified');
  }
  return segEls.map((el) => Math.ceil(el.getBoundingClientRect().width));
}

/**
 * A label is clipped when its natural content width exceeds the width it was
 * allocated. The 1px tolerance absorbs sub-pixel rounding: a seam can land a
 * fraction short of the ceiled content width even when the label is meant to
 * fit. The measured element is the `.seg-name` span (the one carrying
 * text-overflow:ellipsis), not the flex container — a flex container shrinks
 * its child to fit and so never reports scrollWidth > clientWidth. jsdom
 * returns 0 for both unless a test stubs the getters.
 */
function isTruncated(textEl: HTMLElement): boolean {
  return textEl.scrollWidth - textEl.clientWidth > 1;
}

// ── The allocator ────────────────────────────────────────────────────────────
//
// Pure spine allocator for the adaptive breadcrumb/tree column widths. This
// section touches no DOM and does no I/O — it operates on plain numbers and
// arrays; only the `Spine` class above reads the document.
//
// The single shared artifact is a rounded prefix-offset array O[0..N]:
//   O[d] = round(Σ_{k<d} alloc[k]),  O[0] = 0.
// O[N] is the leaf's right edge — the content extent Σalloc, NOT L. Surplus
// beyond every segment's desire stays as trailing empty space rather than being
// folded into the leaf, so O[N] <= L and is usually strictly less.
// No consumer reads O[N] as a spine offset: offsetAt stops at
// lastCol = O.length - 2 and indents structurally past it.
// Breadcrumb segment d has inline-start edge O[d] and width O[d+1]-O[d];
// tree row at depth d uses padding-inline-start offset O[d]. Both read the
// SAME O[], so the spine aligns by construction.
//

/** Breadcrumb right gutter when nothing shares its row (px). editor.css
 *  declares the same value as the `--ft-pr` default; this is the JS side. */
export const FT_PR_BASE = 14;

/** Clear space between the last crumb and the titlebar buttons (px). Without it
 *  a reservation of exactly the buttons' width leaves the crumb's right edge
 *  kissing the first button — which reads as a bug even though nothing
 *  overlaps, and which a "right edge <= buttons' left edge" assertion would
 *  happily call correct. */
export const FT_NAV_GUTTER = 8;

/**
 * Right-hand reservation for the breadcrumb strip, given the measured width of
 * the titlebar button cluster (`.nav-right`) that sits ON that row.
 *
 * Pure so the render gate can call the shipped arithmetic instead of a copy of
 * it: the gate measures a real `.nav-right` in a real engine, asks this
 * function for the reservation, and feeds it to `allocateSpine` — the same two
 * functions `applySpineAllocation` uses.
 *
 * A 0 width means "not measurable" (jsdom, hidden webview), not "no buttons",
 * so it reserves the bare gutter rather than inventing space.
 *
 * Without it the last crumb paints underneath the buttons, and `iconifyCount`
 * gets a budget larger than the row really has, so it demotes ancestors to
 * icon-only too late. The demotion ladder exists so a tight row sheds a label
 * instead of overflowing — an inflated budget defeats it, quietly, at exactly
 * the widths where it matters most.
 *
 * The caller measures `.nav-right` at allocation time and never observes it, and
 * that is sound only because the cluster's width is stable. It holds three fixed
 * buttons; the save-status anchor also inside it is taken OUT of flow by
 * editor.css precisely so a failed save's badge cannot widen it. If anything ever
 * puts a variable-width element back INTO that row, this reservation starts
 * moving with it — and since it feeds `offset[]`, which tree-row indentation is
 * measured from, the whole file tree would shift sideways when it changed.
 *
 * Measuring is free at that site (it is already reading a rect, four custom
 * properties and the whole segment content pass), and what it reads now only
 * changes when the panel does, which is already a reallocation trigger.
 */
export function navReserve(navWidth: number): number {
  return navWidth > 0 ? FT_PR_BASE + navWidth + FT_NAV_GUTTER : FT_PR_BASE;
}

/** Width an icon-only breadcrumb segment needs on top of `--ft-base` (which is
 *  its padding-inline-start): the 18px chevron/dot box, that icon's 3px trailing
 *  margin, `.seg`'s own 4px padding-inline-end, and a pixel of slack — 26. Added
 *  to the live `--ft-base` rather than baked in as one number, because
 *  fullscreen raises that inset from 8px to 16px.
 *
 *  Leaving out the margin and the end padding still LOOKED right — the chevron
 *  was fully visible — but chromium counted the trailing 3px as overflow, so
 *  every demoted crumb was a scroll container by a hair. The render gate
 *  ("a demoted breadcrumb fits its chevron") is what caught it; webkit did not. */
export const FT_ICON_BOX = 26;

/** The `icon` floor for one segment, given the live `--ft-base` inset. Lives here
 *  rather than at the call site so a gate can build the allocator's inputs the
 *  way the shipping code does without importing the DOM-bound tree module. */
export function segIconFloor(ftBase: number): number {
  return ftBase + FT_ICON_BOX;
}

/** Input for one breadcrumb segment (root..leaf, index 0 = root). */
export interface SegInput {
  /** Measured natural content width in px (icon + label + padding). */
  content: number;
  /** Minimum legible width in px (icon + a few chars). */
  floor: number;
  /**
   * Width the segment needs with its label hidden — the icon alone. When the
   * legible floors no longer fit, ancestors are demoted to this instead of
   * being clipped toward zero. Defaults to `floor`, which reproduces the
   * pre-demotion behaviour exactly for callers that don't supply it.
   */
  icon?: number;
}

/** Hysteresis state carried across resize frames (kept out of the pure core). */
export interface SpineState {
  /**
   * How many leftmost ancestors are showing their icon only. The leaf is never
   * counted — it is the one segment that always keeps a readable label.
   */
  iconified: number;
}

export interface SpineResult {
  /** Rounded prefix offsets, length N+1; offset[N] === round(L). */
  offset: number[];
  /** Per-segment widths (offset[i+1]-offset[i]); length N. Convenience. */
  width: number[];
  /** Next hysteresis state. */
  state: SpineState;
  /** True when the degenerate collapse/truncate path was taken. */
  degenerate: boolean;
}

/**
 * Round CUMULATIVE offsets (not individual widths) so the seam never drifts
 * off L and per-seam error stays <= 1px without accumulating.
 * Returns N+1 offsets; the last equals round(L) (pinned to guard float drift).
 */
export function roundCumulative(alloc: number[], L: number): number[] {
  const offset: number[] = [0];
  let acc = 0;
  for (let i = 0; i < alloc.length; i++) {
    acc += alloc[i];
    offset.push(Math.round(acc));
  }
  // Pin the final edge exactly on L (guards float drift in the running sum).
  offset[offset.length - 1] = Math.round(L);
  return offset;
}

const CAP_FRACTION = 0.45;

/**
 * Extra room a narrower demotion level must gain back before an already-demoted
 * ancestor is allowed to show its label again. Without it, a panel parked on the
 * exact boundary flickers a label in and out on every sub-pixel resize tick.
 */
const DEMOTE_HYSTERESIS = 12;

/**
 * How many leftmost ancestors must hide their label for the row to fit.
 *
 * Ancestors are demoted root-first, because the leaf is what the user is
 * looking at and the root is the crumb they can most afford to read as an icon.
 * Returns `n - 1` (every ancestor iconified) when even that doesn't fit — the
 * caller then falls through to the degenerate path.
 */
function iconifyCount(
  floor: number[],
  icon: number[],
  L: number,
  prev: number,
): number {
  const n = floor.length;
  const total = (k: number): number => {
    let sum = 0;
    for (let i = 0; i < n; i++) sum += i < k ? icon[i] : floor[i];
    return sum;
  };
  for (let k = 0; k <= n - 1; k++) {
    // Coming back UP (fewer icons than last frame) has to clear the boundary by
    // DEMOTE_HYSTERESIS; going down is immediate, so the row never overflows.
    const need = k < prev ? L - DEMOTE_HYSTERESIS : L;
    if (total(k) <= need) return k;
  }
  // Nothing fits at any rung — demote every ancestor whose icon is actually
  // narrower than its label. Stopping at the first that isn't keeps the count
  // honest: it is also the set the caller hides labels on, so a segment that
  // gains no width must not be reported as demoted.
  let k = 0;
  while (k < n - 1 && icon[k] < floor[k]) k++;
  return k;
}

/** The effective per-segment cap respects both bounds and never falls below the floor. */
function capEff(floor: number, panelW: number, capAbs: number): number {
  return Math.max(floor, Math.min(CAP_FRACTION * panelW, capAbs));
}

/**
 * Allocate the panel content width L across N segments (root..leaf).
 *
 * Normal case (Σfloor <= L): reserve every floor, then hand the surplus out
 * RIGHT-TO-LEFT (leaf first, root last) so the current/leaf segment is
 * satisfied before any ancestor and the leftmost truncates first.
 * Every segment (leaf included) stops at its content width — surplus beyond all
 * desires is left as trailing empty space (the strip's own drag region), NOT
 * folded into the leaf. The JS offset stays the single source of truth.
 *
 * Too narrow for that (Σfloor > L): ancestors hide their labels root-first and
 * shrink to their icon width, which is the editor's answer to the site
 * breadcrumb's "…" fold. It cannot borrow the fold itself — a folded crumb has
 * no column, and the tree's row indents come from these very columns — so it
 * keeps every column and starves each one down to its icon instead.
 *
 * @param L        panel content width (panel - inline-start origin - end gutter), px
 * @param input    per-segment { content, floor, icon? }, index 0 = root, N-1 = leaf
 * @param capAbs   absolute max width any segment may grow to, px
 * @param panelW   full panel width, px (drives the panel-relative cap)
 * @param prev     previous hysteresis state
 * @param lastGood last-good offset array, returned verbatim when L<=0 (panel hidden)
 */
export function allocateSpine(
  L: number,
  input: SegInput[],
  capAbs: number,
  panelW: number,
  prev: SpineState,
  lastGood?: number[],
): SpineResult {
  const n = input.length;

  // ── Guard: panel hidden/collapsed/zero — keep the last-good vector ──────
  // Never write garbage offsets the tree would consume.
  if (n === 0 || L <= 0) {
    const offset = lastGood ?? [0];
    const width = offset.slice(1).map((o, i) => o - offset[i]);
    return { offset, width, state: prev, degenerate: true };
  }

  // ── Demotion ladder ─────────────────────────────────────────────────────
  // A narrow panel used to shave every crumb toward its 40px floor and then to
  // zero, which reads as a row of two-character stubs and finally as nothing at
  // all. Instead, ancestors give up their LABEL — root first — and keep only
  // their icon, so each one still holds a column (the tree indents off these
  // offsets, so a column may never disappear) and the leaf stays readable.
  const legible = input.map((s) => s.floor);
  const iconOnly = input.map((s) => Math.min(s.icon ?? s.floor, s.floor));
  const iconified = iconifyCount(legible, iconOnly, L, prev.iconified);
  const floor = legible.map((f, i) => (i < iconified ? iconOnly[i] : f));
  const sumFloor = floor.reduce((a, b) => a + b, 0);

  const desired = input.map((s, i) => {
    // An iconified segment has no label to reveal, so growing it past its icon
    // would just push the leaf leftward for nothing.
    if (i < iconified) return floor[i];
    const cap = capEff(s.floor, panelW, capAbs);
    return Math.min(Math.max(s.content, s.floor), cap);
  });

  // ── Degenerate: even all floors don't fit. Protect the leaf; take the ───
  // deficit from the LEFT. Truncate-in-place (no "…"-collapse).
  if (sumFloor > L) {
    const alloc = floor.slice();
    let deficit = sumFloor - L;
    // Walk root→leaf, shrinking the leftmost survivors first (below floor,
    // overflow:hidden clips), but never the leaf below what remains.
    for (let i = 0; i < n - 1 && deficit > 0; i++) {
      const take = Math.min(alloc[i], deficit); // can shrink an ancestor to 0
      alloc[i] -= take;
      deficit -= take;
    }
    // Whatever deficit is left comes off the leaf (truncates the current label).
    if (deficit > 0) alloc[n - 1] = Math.max(0, alloc[n - 1] - deficit);
    const sum = alloc.reduce((a, b) => a + b, 0);
    const offset = roundCumulative(alloc, Math.min(L, sum));
    const width = offset.slice(1).map((o, i) => o - offset[i]);
    return { offset, width, state: { iconified }, degenerate: true };
  }

  // ── Normal case ────────────────────────────────────────────────────────
  // Reserve every floor, then hand the surplus out right-to-left. Each segment
  // grows only up to its own desire (content clamped to cap) — never past it,
  // so NO segment (leaf included) swallows leftover space: content-fit, don't
  // grow past content. Any surplus that remains after every desire is satisfied
  // is left as trailing empty space in the strip — the breadcrumb's own bare
  // area, which the caller marks as a window drag region (grab-to-move). This
  // does NOT truncate the current filename: the leaf still gets its full content
  // width up to the panel cap; only the empty padding BEYOND the filename is gone.
  const alloc = floor.slice();
  let surplus = L - sumFloor;
  for (let i = n - 1; i >= 0 && surplus > 0; i--) {
    const grant = Math.min(desired[i] - floor[i], surplus);
    alloc[i] += grant;
    surplus -= grant;
  }
  // The content extent the row actually occupies (Σ alloc), <= L. The
  // breadcrumb's trailing empty space is L - contentExtent; roundCumulative
  // rounds the segment seams against contentExtent so widths reflect real
  // content, and the caller treats offset[N] as the leaf's right edge.
  const contentExtent = alloc.reduce((a, b) => a + b, 0);
  const offset = roundCumulative(alloc, contentExtent);
  const width = offset.slice(1).map((o, i) => o - offset[i]);
  return { offset, width, state: { iconified }, degenerate: false };
}
