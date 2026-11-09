import { describe, it, expect } from 'vitest';
import {
  roundCumulative,
  allocateSpine,
  navReserve,
  segIconFloor,
  FT_PR_BASE,
  FT_NAV_GUTTER,
  FT_ICON_BOX,
} from '../spine';

// Helper: build N segs with given content widths and a uniform floor.
const segs = (content: number[], floor = 40) =>
  content.map((c) => ({ content: c, floor }));

// FT_SEG_CAP_ABS is now Infinity: the proportional 0.45*panelW cap is the
// sole upper bound so segments can use full available space on wide panels.
const ABS_MAX = Infinity;
const noState = { iconified: 0 };

describe('roundCumulative', () => {
  it('returns N+1 offsets starting at 0', () => {
    const o = roundCumulative([10, 20, 30], 60);
    expect(o.length).toBe(4);
    expect(o[0]).toBe(0);
  });

  it('lands the final offset exactly on L (no seam drift)', () => {
    const o = roundCumulative([10.4, 20.4, 29.2], 60);
    expect(o[o.length - 1]).toBe(60);
  });

  it('bounds per-seam rounding error to <= 1px and never accumulates', () => {
    const alloc = [10.49, 10.49, 10.49, 10.49, 10.49, 10.55];
    const L = Math.round(alloc.reduce((a, b) => a + b, 0));
    const o = roundCumulative(alloc, L);
    expect(o[o.length - 1]).toBe(L);
    // Each width is the integer diff of rounded offsets — within 1px of its ideal.
    for (let i = 0; i < alloc.length; i++) {
      const width = o[i + 1] - o[i];
      expect(Math.abs(width - alloc[i])).toBeLessThanOrEqual(1);
    }
  });

  it('handles a single segment', () => {
    expect(roundCumulative([42.7], 43)).toEqual([0, 43]);
  });
});

describe('allocateSpine — normal case', () => {
  it('everything fits with room to spare: every seg at content, surplus stays trailing', () => {
    // panel 600 → cap = max(40, min(0.45*600=270, Infinity)) = 270.
    // content [60,80,100] sums to 240 << L 580. After R-to-L each seg reaches
    // its content; the 340 surplus is left as trailing drag space (not folded).
    const r = allocateSpine(580, segs([60, 80, 100]), ABS_MAX, 600, noState);
    expect(r.degenerate).toBe(false);
    expect(r.width[0]).toBe(60);
    expect(r.width[1]).toBe(80);
    expect(r.width[2]).toBe(100); // leaf at content, NOT expanded to the cap
    expect(r.offset[r.offset.length - 1]).toBe(240); // 60+80+100
  });

  it('squeeze: rightmost satisfied first, leftmost floored', () => {
    // floors = 3*40 = 120; L = 220 → surplus 100 all goes to the leaf first.
    const r = allocateSpine(220, segs([200, 200, 200]), ABS_MAX, 500, noState);
    expect(r.degenerate).toBe(false);
    expect(r.width[2]).toBe(140); // leaf: floor 40 + all 100 surplus
    expect(r.width[1]).toBe(40);  // ancestor floored
    expect(r.width[0]).toBe(40);  // ancestor floored
  });

  it('partial squeeze: surplus walks right-to-left, leftmost loses last', () => {
    // floors 120, L = 320 → surplus 200. Leaf needs 200-40=160, takes 160.
    // remaining 40 → parent (index1) takes min(160,40)=40. root stays floored.
    const r = allocateSpine(320, segs([200, 200, 200]), ABS_MAX, 500, noState);
    expect(r.width[2]).toBe(200); // leaf at desire
    expect(r.width[1]).toBe(80);  // parent: 40 floor + 40 surplus
    expect(r.width[0]).toBe(40);  // root floored
  });

  it('caps each segment at cap_eff (dual-bounded, floor-safe)', () => {
    // panel 400 → cap = max(40, min(180, 240)) = 180. Content 300 clamps to 180.
    // Both segs capped at 180; leaf also folds leftover surplus up to its cap (180).
    const r = allocateSpine(2000, segs([300, 300]), ABS_MAX, 400, noState);
    expect(r.width[0]).toBe(180);
    expect(r.width[1]).toBe(180); // leaf at cap (leftover grant capped at 180-180=0)
  });

  it('cap is never below floor even on a narrow panel', () => {
    // panel 80 → 0.45*80 = 36 < floor 40. cap_eff must be >= 40.
    // floors 80, L 80 → exactly fits at floors.
    const r = allocateSpine(80, segs([10, 10]), ABS_MAX, 80, noState);
    expect(r.degenerate).toBe(false);
    expect(r.width[0]).toBe(40);
    expect(r.width[1]).toBe(40);
  });

  it('MONOTONICITY: no offset decreases as L grows (panel-relative cap)', () => {
    const input = segs([120, 90, 150, 70]);
    let prev: number[] | null = null;
    for (let L = 200; L <= 900; L += 7) {
      const panel = L + 90; // panel tracks L (origin + gutter)
      const r = allocateSpine(L, input, ABS_MAX, panel, noState);
      if (prev) {
        for (let d = 0; d < r.offset.length; d++) {
          expect(r.offset[d]).toBeGreaterThanOrEqual(prev[d] - 1); // <=1px snap tolerance
        }
      }
      prev = r.offset;
    }
  });

  // ── New cases proving the breadcrumb-seg-cap fix ─────────────────────────

  it('full-width long path: segments NOT truncated when panel is wide', () => {
    // Reproduces the bug: panelW 900, single ancestor+leaf with long content.
    // With FT_SEG_CAP_ABS=240 the old code capped both at 240; with Infinity
    // each is bounded only by 0.45*900=405. Content 300 < 405, so no truncation.
    // L = 900 - 76 - 14 = 810 (simulating typical panel insets).
    const input = segs([300, 250]); // ancestor=300px, leaf=250px — both fit at full-width
    const r = allocateSpine(810, input, ABS_MAX, 900, noState);
    expect(r.degenerate).toBe(false);
    // Both segments must receive AT LEAST their natural content width (no truncation).
    expect(r.width[0]).toBeGreaterThanOrEqual(300);
    expect(r.width[1]).toBeGreaterThanOrEqual(250);
  });

  it('narrow panel still truncates without overflow', () => {
    // panelW 300 → cap = 0.45*300 = 135. Two segs with content 300 each.
    // floor=40, sumFloor=80, L=210 (some gutter). Desired = min(300,135)=135.
    // Surplus = 210-80=130. R-to-L: leaf gets min(135-40,130)=95 → 135.
    // Remaining 35 → ancestor gets min(135-40,35)=35 → 75.
    const r = allocateSpine(210, segs([300, 300]), ABS_MAX, 300, noState);
    expect(r.degenerate).toBe(false);
    // No segment may exceed the available L (no horizontal overflow).
    expect(r.width[0] + r.width[1]).toBeLessThanOrEqual(210);
    // Each segment stays within the proportional cap for a 300px panel.
    const cap = Math.max(40, 0.45 * 300);
    expect(r.width[0]).toBeLessThanOrEqual(cap + 1); // +1 for rounding
    expect(r.width[1]).toBeLessThanOrEqual(cap + 1);
  });

  it('MONOTONICITY preserved across narrow→wide range with proportional-only cap', () => {
    // Sweep panel from 200px to 1200px — offsets must never decrease.
    // Validates that removing the 240px absolute cap keeps monotonicity intact.
    const input = segs([180, 140, 200]);
    let prev: number[] | null = null;
    for (let panelW = 200; panelW <= 1200; panelW += 13) {
      const L = Math.max(0, panelW - 90); // simulate origin + gutter
      if (L <= 0) continue;
      const r = allocateSpine(L, input, ABS_MAX, panelW, noState);
      if (prev) {
        for (let d = 0; d < r.offset.length; d++) {
          expect(r.offset[d]).toBeGreaterThanOrEqual((prev[d] ?? 0) - 1); // <=1px snap
        }
      }
      prev = r.offset;
    }
  });
});

describe('allocateSpine — leaf sizes to content (trailing drag-region space)', () => {
  it('leaf does NOT absorb trailing surplus — it stops at its content width', () => {
    // panel 600 → cap 270. content [60,80,100] sums 240 << L 580. Each seg
    // reaches its content via the R-to-L loop; the 340 leftover surplus is NOT
    // folded into the leaf (which used to grow it to the 270 cap).
    const r = allocateSpine(580, segs([60, 80, 100]), ABS_MAX, 600, noState);
    expect(r.degenerate).toBe(false);
    expect(r.width[0]).toBe(60);
    expect(r.width[1]).toBe(80);
    expect(r.width[2]).toBe(100); // leaf at CONTENT, not the 270 cap
  });

  it('leaves trailing empty space (contentExtent < L) to grab as a drag region', () => {
    const r = allocateSpine(580, segs([60, 80, 100]), ABS_MAX, 600, noState);
    const contentExtent = r.offset[r.offset.length - 1];
    expect(contentExtent).toBe(240); // 60+80+100, not 410
    expect(contentExtent).toBeLessThan(580); // ~340px of trailing drag space
  });

  it('does NOT truncate a long-but-fitting filename: leaf gets full content up to cap', () => {
    // content 260 < cap 270 → the whole filename still shows; only the empty
    // padding BEYOND the filename is gone.
    const r = allocateSpine(580, segs([60, 80, 260]), ABS_MAX, 600, noState);
    expect(r.width[2]).toBe(260);
  });

  it('SPINE INVARIANT: ancestor offsets are unchanged as leaf content varies', () => {
    // The tree indents from ancestor offsets O[0..N-1]; only the leaf width
    // (O[N]) may move. Removing the fold must not shear the spine.
    const wide = allocateSpine(580, segs([60, 80, 100]), ABS_MAX, 600, noState);
    const short = allocateSpine(580, segs([60, 80, 40]), ABS_MAX, 600, noState);
    expect(short.offset.slice(0, -1)).toEqual(wide.offset.slice(0, -1));
    expect(short.offset[short.offset.length - 1]).not.toBe(
      wide.offset[wide.offset.length - 1],
    );
  });
});

describe('allocateSpine — ancestors starve to their icon before they stub out', () => {
  // The editor's answer to the site breadcrumb's "…" fold. It cannot fold: a
  // folded crumb has no column, and the tree indents rows off these columns. So
  // it keeps every column and drops labels from the left instead.
  const LEGIBLE = 72;
  const ICON = 30;
  const path = (n: number, content = 200) =>
    Array.from({ length: n }, () => ({ content, floor: LEGIBLE, icon: ICON }));

  it('leaves every crumb legible while they all still fit', () => {
    const r = allocateSpine(600, path(4), ABS_MAX, 700, noState);
    expect(r.state.iconified).toBe(0);
    for (const w of r.width) expect(w).toBeGreaterThanOrEqual(LEGIBLE);
  });

  it('demotes root-first, only as far as it has to', () => {
    // 4 crumbs need 288 legible. At L=250, dropping the root's label frees
    // 42px → 246, which fits, so exactly one crumb goes icon-only.
    const r = allocateSpine(250, path(4), ABS_MAX, 700, noState);
    expect(r.state.iconified).toBe(1);
    expect(r.width[0]).toBe(ICON);
    expect(r.width[1]).toBeGreaterThanOrEqual(LEGIBLE);
  });

  it('keeps the leaf legible while every ancestor is an icon', () => {
    const r = allocateSpine(170, path(4), ABS_MAX, 400, noState);
    expect(r.state.iconified).toBe(3);
    expect(r.width[3]).toBeGreaterThanOrEqual(LEGIBLE);
    expect(r.degenerate).toBe(false);
  });

  it('never demotes the leaf, even when nothing fits', () => {
    const r = allocateSpine(60, path(4), ABS_MAX, 200, noState);
    expect(r.state.iconified).toBeLessThanOrEqual(3);
    expect(r.degenerate).toBe(true);
    expect(r.offset[r.offset.length - 1]).toBeLessThanOrEqual(60);
  });

  it('holds a demoted crumb demoted until there is real room to spare', () => {
    // 250 is exactly the width at which one icon is enough. Coming back up from
    // a demoted state at that same width must NOT restore the label, or a panel
    // parked on the boundary flickers on every resize tick.
    const wide = allocateSpine(250, path(4), ABS_MAX, 700, noState);
    expect(wide.state.iconified).toBe(1);
    const held = allocateSpine(288, path(4), ABS_MAX, 700, { iconified: 1 });
    expect(held.state.iconified).toBe(1);
    const restored = allocateSpine(300, path(4), ABS_MAX, 700, { iconified: 1 });
    expect(restored.state.iconified).toBe(0);
  });

  it('a segment with no icon width behaves exactly as before', () => {
    // The `icon` field is optional; omitting it makes every rung of the ladder
    // the same width, so the count is 0 and nothing changes for old callers.
    const r = allocateSpine(130, segs([50, 50, 50, 50], 40), ABS_MAX, 200, noState);
    expect(r.state.iconified).toBe(0);
  });
});

describe('allocateSpine — degenerate cases', () => {
  it('N=1: leaf gets min(L, proportional-cap), no ancestors to collapse', () => {
    const r = allocateSpine(500, segs([1000]), ABS_MAX, 700, noState);
    // panel 700 → cap = max(40, min(0.45*700=315, Infinity)) = 315.
    // content 1000 clamps to 315. L=500 > 315, so leaf gets 315.
    expect(r.width[0]).toBe(315);
    expect(r.offset).toEqual([0, 315]);
    expect(r.degenerate).toBe(false);
  });

  it('N=1 with floor > L: leaf truncates to L (degenerate, no scroll)', () => {
    const r = allocateSpine(30, segs([100], 40), ABS_MAX, 60, noState);
    expect(r.degenerate).toBe(true);
    expect(r.width[0]).toBe(30); // truncated to available, never exceeds L
    expect(r.offset[r.offset.length - 1]).toBe(30);
  });

  it('L <= 0 (panel hidden/collapsed): returns the last-good offsets, no garbage', () => {
    const lastGood = { iconified: 0 } as const;
    const r = allocateSpine(0, segs([60, 80, 100]), ABS_MAX, 0, lastGood, [0, 60, 140, 240]);
    expect(r.offset).toEqual([0, 60, 140, 240]); // unchanged last-good vector
    expect(r.degenerate).toBe(true);
  });

  it('floors overflow (Σfloor > L), N>=2: protect leaf, floor survivors, truncate left', () => {
    // 4 segs, floor 40 each → Σfloor 160. L = 130 < 160.
    const r = allocateSpine(130, segs([50, 50, 50, 50], 40), ABS_MAX, 200, noState);
    expect(r.degenerate).toBe(true);
    // Leaf keeps a full floor; the deficit is taken from the LEFT.
    expect(r.width[r.width.length - 1]).toBeGreaterThanOrEqual(40);
    // Total never exceeds L (no horizontal scroll).
    expect(r.offset[r.offset.length - 1]).toBeLessThanOrEqual(130);
    // Offsets are monotonic non-decreasing (no negative widths).
    for (let i = 1; i < r.offset.length; i++) {
      expect(r.offset[i]).toBeGreaterThanOrEqual(r.offset[i - 1]);
    }
  });
  // ── Reservation invariance (the claim the --ft-pr reservation rests on) ────
  // Reserving the titlebar buttons' width out of L is only safe if it changes
  // NOTHING when the crumbs already fit. That holds because the normal path
  // grants each segment min(content, cap) and leaves the surplus as trailing
  // space, capEff is driven by panelW rather than L, and roundCumulative is
  // pinned to the content extent. It is NOT unconditional: iconifyCount applies
  // a 12px DEMOTE_HYSTERESIS band when coming back up from a demoted frame, so
  // the invariant needs Σlegible <= L_reserved - 12, or prev.iconified === 0.

  it('reserving L does not change the allocation when the crumbs still fit', () => {
    const input = segs([60, 80, 100], 40);      // Σdesired = 240, Σfloor = 120
    const panelW = 600;
    const unreserved = allocateSpine(400, input, ABS_MAX, panelW, noState);
    const reserved = allocateSpine(300, input, ABS_MAX, panelW, noState);
    // 240 <= 300 <= 400, and prev.iconified === 0, so both must agree exactly.
    expect(reserved.width).toEqual(unreserved.width);
    expect(reserved.offset).toEqual(unreserved.offset);
    expect(reserved.state.iconified).toBe(0);
    expect(reserved.degenerate).toBe(false);
  });

  it('a reservation that crosses the floors DOES demote — that is the fix, not a regression', () => {
    // 4 crumbs, legible floor 72 (--ft-seg-min) → Σfloor = 288.
    // Unreserved L=300 fits all four; reserved L=228 (390px panel, 76px traffic
    // lights, ~86px reservation) fits only three.
    const input = segs([200, 200, 200, 200], 72).map((s) => ({ ...s, icon: 34 }));
    const before = allocateSpine(300, input, ABS_MAX, 390, noState);
    const after = allocateSpine(228, input, ABS_MAX, 390, noState);
    expect(before.state.iconified).toBe(0);
    expect(after.state.iconified).toBeGreaterThan(0);
    // The leaf keeps a readable label either way.
    expect(after.width[after.width.length - 1]).toBeGreaterThanOrEqual(72);
  });

  it('inside the hysteresis band, a demoted previous frame stays demoted', () => {
    // Σlegible = 3*72 = 216. L = 220 clears the floors outright, but coming UP
    // from iconified=1 requires clearing L - 12 = 208 as well, which 216 does
    // not — so the label stays hidden until there is real room for it. This is
    // why the invariance test above pins prev.iconified === 0.
    const input = segs([200, 200, 200], 72).map((s) => ({ ...s, icon: 34 }));
    const fromClean = allocateSpine(220, input, ABS_MAX, 400, { iconified: 0 });
    const fromDemoted = allocateSpine(220, input, ABS_MAX, 400, { iconified: 1 });
    expect(fromClean.state.iconified).toBe(0);
    expect(fromDemoted.state.iconified).toBe(1);
  });
});

// ── The reservations the DOM layer feeds the allocator ────────────────────
//
// Pure arithmetic, so it belongs here rather than in a render gate: the gate
// rule is "only if it needs an engine". What needs an engine is the WIDTH of
// `.nav-right` (glyph metrics, per engine and per locale) — not what is done
// with that width once measured.

describe('navReserve', () => {
  it('reserves the buttons plus a gutter on each side of them', () => {
    expect(navReserve(120)).toBe(FT_PR_BASE + 120 + FT_NAV_GUTTER);
  });

  it('reserves only the bare gutter when the cluster cannot be measured', () => {
    // 0 means "not measurable" — jsdom, a hidden webview, a pre-layout frame —
    // and NOT "there are no buttons". Inventing space for buttons whose width is
    // unknown would push the crumbs around on the first paint and then again
    // once the real measurement arrived.
    expect(navReserve(0)).toBe(FT_PR_BASE);
  });

  it('grows with the cluster, which is the whole reason it is measured', () => {
    // The save badge mounts INSIDE `.nav-right` and widens it mid-session, so a
    // reservation that ignored the argument would be right until the user typed.
    expect(navReserve(200)).toBeGreaterThan(navReserve(120));
  });
});

describe('segIconFloor', () => {
  it('adds the chevron box to the live inset rather than to a baked-in one', () => {
    // Fullscreen raises `--ft-base` from 8 to 16; the floor has to follow, or a
    // demoted crumb clips its own icon in exactly that state.
    expect(segIconFloor(8)).toBe(8 + FT_ICON_BOX);
    expect(segIconFloor(16)).toBe(16 + FT_ICON_BOX);
  });
});
