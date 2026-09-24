/**
 * Tests for the scroll-position message emitted by iframe-bridge.ts.
 *
 * The bridge observes user scroll in the preview and posts
 * `{ type: 'moss-scroll-position', url, line }` to the parent window.
 * The `url` field is required so the parent can validate that the
 * reported line belongs to the currently-active page (and not a page
 * that was just superseded by a navigation).
 *
 * iframe-bridge.ts is an IIFE injected into the preview iframe and cannot be
 * imported, so what it computes lives in scroll-interpolate.ts and is tested
 * here directly; what only the bridge does (the message literal, the RPC case)
 * is asserted against the shipped bundle.
 */
import { describe, it, test, expect, beforeEach, vi } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { getSourceLine, interpolatedScrollTop, isAlreadyShowing, readScrollPosition } from "../scroll-interpolate";

const BUNDLE_PATH = resolve(
  __dirname,
  "../../../ops/serve/js/iframe-bridge.js"
);
const bundleExists = existsSync(BUNDLE_PATH);
if (!bundleExists) {
  // eslint-disable-next-line no-console
  console.warn(
    `[iframe-bridge-scroll-sync] Skipping bundle regression test: ${BUNDLE_PATH} not found. Run \`pnpm run frontend:build\` to generate it.`
  );
}

describe("readScrollPosition (what a scroll reports, and what the editor gets when it asks)", () => {
  /** Lay out annotated paragraphs at the given viewport tops. */
  function page(tops: Record<string, number>, scrollY: number, scrollHeight = 5000): Window {
    document.body.innerHTML = Object.keys(tops)
      .map((line) => `<p data-source-line="${line}" id="l${line}">x</p>`)
      .join("");
    for (const [line, top] of Object.entries(tops)) {
      vi.spyOn(document.getElementById(`l${line}`)!, "getBoundingClientRect")
        .mockReturnValue({ top } as DOMRect);
    }
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
    Object.defineProperty(window, "scrollY", { configurable: true, value: scrollY });
    Object.defineProperty(document.documentElement, "scrollHeight", { configurable: true, value: scrollHeight });
    return window;
  }

  it("reports the line on the focus line, not the first one visible", () => {
    // Focus line is 400px down an 800px viewport: 12 sits on it, 9 above.
    expect(readScrollPosition(page({ 9: 50, 12: 380, 20: 700 }, 1500)))
      .toEqual({ line: 12, atTop: false, atBottom: false });
  });

  it("near the page top, the topmost visible line stands in for the focus line", () => {
    expect(readScrollPosition(page({ 3: 600 }, 200)).line).toBe(3);
  });

  it("at the very top with nothing annotated in view, reports line 1 and atTop", () => {
    expect(readScrollPosition(page({}, 0))).toEqual({ line: 1, atTop: true, atBottom: false });
  });

  it("past the top with nothing annotated in view, reports no line", () => {
    expect(readScrollPosition(page({}, 200)).line).toBe(0);
  });

  it("at the page bottom, reports atBottom within the 4px slack", () => {
    // 5000px page, 800px viewport: the bottom is 4200.
    expect(readScrollPosition(page({ 90: 300 }, 4198)).atBottom).toBe(true);
    expect(readScrollPosition(page({ 90: 300 }, 1000)).atBottom).toBe(false);
  });

  it("a page too short to scroll is at both edges; the receiver resolves atTop first", () => {
    expect(readScrollPosition(page({ 1: 20 }, 0, 600))).toMatchObject({ atTop: true, atBottom: true });
  });

  it("a range annotation reports its first line", () => {
    document.body.innerHTML = '<div data-source-range="18-42" id="g">grid</div>';
    expect(getSourceLine(document.getElementById("g")!)).toBe(18);
  });
});

describe("shipped bundle", () => {
  test.skipIf(!bundleExists)(
    "moss-scroll-position carries url alongside the line",
    () => {
      const region = readFileSync(BUNDLE_PATH, "utf8").match(/"moss-scroll-position"[\s\S]{0,200}/);
      expect(region).not.toBeNull();
      expect(region![0]).toMatch(/url\s*:\s*[a-zA-Z_$][\w$.]*\.href|location\.href/);
      expect(region![0]).toMatch(/line\s*:/);
      expect(region![0]).toMatch(/atBottom\s*:/);
    }
  );

  test.skipIf(!bundleExists)(
    "answers the scrollPosition RPC",
    () => {
      expect(readFileSync(BUNDLE_PATH, "utf8")).toMatch(/case\s*"scrollPosition"/);
    }
  );
});

// ── Task 2: scrollToSourceLine(1) goes to scrollTop=0 ─────────────────

describe("scrollToSourceLine at line 1", () => {
  let scrollToCalls: Array<{ top: number; behavior: string }>;
  let intoViewCalls: number;

  beforeEach(() => {
    scrollToCalls = [];
    intoViewCalls = 0;
    document.body.innerHTML =
      '<header>Site</header>' +
      '<h1 data-source-line="1" id="h1">Title</h1>' +
      '<p data-source-line="3" id="p3">body</p>';

    // jsdom doesn't implement window.scrollTo with an options object; install a spy.
    (window as unknown as { scrollTo: (opts: { top: number; behavior: string }) => void }).scrollTo =
      vi.fn((opts: { top: number; behavior: string }) => {
        scrollToCalls.push(opts);
      });
    document.getElementById("h1")!.scrollIntoView = vi.fn(() => { intoViewCalls++; });
    document.getElementById("p3")!.scrollIntoView = vi.fn(() => { intoViewCalls++; });
  });

  /** Post-fix harness: targetLine <= 1 short-circuits to scrollTop=0 regardless
   * of whether a data-source-line="1" element exists. */
  function scrollToSourceLineV2(targetLine: number): number {
    if (targetLine <= 1) {
      window.scrollTo({ top: 0, behavior: 'smooth' });
      return 0;
    }
    const els = document.querySelectorAll('[data-source-line], [data-source-range]');
    let best: Element | null = null;
    let bestLine = 0;
    for (const el of Array.from(els)) {
      const raw = el.getAttribute("data-source-line");
      const l = raw ? parseInt(raw, 10) : 0;
      if (l <= targetLine && l > bestLine) {
        bestLine = l;
        best = el;
      }
    }
    if (best) {
      (best as HTMLElement).scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
    return bestLine;
  }

  it("targetLine=1 scrolls window to top, never calls scrollIntoView on H1", () => {
    const result = scrollToSourceLineV2(1);
    expect(scrollToCalls).toEqual([{ top: 0, behavior: 'smooth' }]);
    expect(intoViewCalls).toBe(0);
    expect(result).toBe(0);
  });

  it("targetLine=0 (defensive) also scrolls to top", () => {
    const result = scrollToSourceLineV2(0);
    expect(scrollToCalls).toEqual([{ top: 0, behavior: 'smooth' }]);
    expect(intoViewCalls).toBe(0);
    expect(result).toBe(0);
  });

  it("targetLine=3 still uses element scrollIntoView (regression guard)", () => {
    const result = scrollToSourceLineV2(3);
    expect(scrollToCalls).toHaveLength(0);
    expect(intoViewCalls).toBe(1);
    expect(result).toBe(3);
  });

  test.skipIf(!bundleExists)(
    "shipped iframe-bridge.js bundle short-circuits scrollToSourceLine(line<=1) to top",
    () => {
      const bundle = readFileSync(BUNDLE_PATH, "utf8");
      // Locate the scrollToSourceLine case body and look for the new top-of-doc
      // scroll. The handler is identified by the case label "scrollToSourceLine";
      // the new branch fires window.scrollTo({top:0,...}) before the search loop.
      const region = bundle.match(/"scrollToSourceLine"[\s\S]{0,400}/);
      expect(region).not.toBeNull();
      expect(region![0]).toMatch(/scrollTo\s*\(\s*\{\s*top\s*:\s*0/);
    }
  );
});

// ── atTop wire field: emit + receive + bundle regression ─────────────────
//
// This PR adds an explicit `atTop: boolean` to moss-scroll-position and to
// the scrollToSourceLine RPC, AND removes the legacy `line <= 1` backstops
// from both call sites. See docs/archive/2026-05-28-scroll-sync-atTop-wire-field.md.

describe("scrollToSourceLine RPC — atTop arg", () => {
  let scrollToCalls: Array<{ top: number; behavior: string }>;
  let intoViewCalls: number;

  beforeEach(() => {
    scrollToCalls = [];
    intoViewCalls = 0;
    document.body.innerHTML =
      '<h1 data-source-line="3" id="h3">Title</h1>' +
      '<p data-source-line="7" id="p7">body</p>';

    (window as unknown as { scrollTo: (opts: { top: number; behavior: string }) => void }).scrollTo =
      vi.fn((opts: { top: number; behavior: string }) => { scrollToCalls.push(opts); });
    document.getElementById("h3")!.scrollIntoView = vi.fn(() => { intoViewCalls++; });
    document.getElementById("p7")!.scrollIntoView = vi.fn(() => { intoViewCalls++; });
  });

  /** Post-atTop scrollToSourceLine harness: NO line<=1 backstop.
   * Only atTop=true scrolls to top; line is purely informational. */
  function scrollToSourceLineV3(targetLine: number, atTop: boolean): number {
    if (atTop) {
      window.scrollTo({ top: 0, behavior: 'smooth' });
      return 0;
    }
    const els = document.querySelectorAll('[data-source-line]');
    let best: Element | null = null;
    let bestLine = 0;
    for (const el of Array.from(els)) {
      const raw = el.getAttribute("data-source-line");
      const l = raw ? parseInt(raw, 10) : 0;
      if (l <= targetLine && l > bestLine) {
        bestLine = l;
        best = el;
      }
    }
    if (best) (best as HTMLElement).scrollIntoView({ behavior: 'smooth', block: 'start' });
    return bestLine;
  }

  it("atTop=true scrolls to scrollTop=0 regardless of line", () => {
    const result = scrollToSourceLineV3(5, true);
    expect(scrollToCalls).toEqual([{ top: 0, behavior: 'smooth' }]);
    expect(intoViewCalls).toBe(0);
    expect(result).toBe(0);
  });

  it("atTop=false with line<=1 NO LONGER short-circuits — uses search loop (CRITICAL CHANGE)", () => {
    // The previous PR's `targetLine <= 1` backstop is REMOVED. line:1 with
    // atTop:false means "the line-1 element" (if any) and falls through to
    // the loop. If no element matches, scrollY stays at 0 (no scrollTo).
    const result = scrollToSourceLineV3(1, false);
    expect(scrollToCalls).toHaveLength(0);  // no scrollTo({top:0})
    // No element has data-source-line=1 (h3 has line=3, p7 has line=7), so
    // best is null and no scrollIntoView either.
    expect(intoViewCalls).toBe(0);
    expect(result).toBe(0);
  });

  it("atTop=false with line=7 finds the matching element (regression guard)", () => {
    const result = scrollToSourceLineV3(7, false);
    expect(scrollToCalls).toHaveLength(0);
    expect(intoViewCalls).toBe(1);
    expect(result).toBe(7);
  });

  it("atTop=false with line=5 picks the highest l <= 5 (data-source-line=3)", () => {
    const result = scrollToSourceLineV3(5, false);
    expect(intoViewCalls).toBe(1);
    expect(result).toBe(3);
  });
});

describe("iframe-bridge atTop bundle regression", () => {
  test.skipIf(!bundleExists)(
    "bundle emits atTop in moss-scroll-position payload",
    () => {
      const bundle = readFileSync(BUNDLE_PATH, "utf8");
      // postMessage object literal keys survive minification (verified
      // against existing `url:`/`line:` patterns at line 134). atTop should
      // sit alongside them.
      const region = bundle.match(/"moss-scroll-position"[\s\S]{0,200}/);
      expect(region).not.toBeNull();
      expect(region![0]).toMatch(/atTop\s*:/);
    }
  );

  test.skipIf(!bundleExists)(
    "bundle scrollToSourceLine handler reads args[1] for atTop",
    () => {
      const bundle = readFileSync(BUNDLE_PATH, "utf8");
      const region = bundle.match(/"scrollToSourceLine"[\s\S]{0,400}/);
      expect(region).not.toBeNull();
      // Minifier may inline `=== true` as `===!0`; cover both.
      expect(region![0]).toMatch(/\[1\]\s*===?\s*(?:!0|true)/);
    }
  );

  test.skipIf(!bundleExists)(
    "bundle no longer contains the targetLine <= 1 backstop in scrollToSourceLine",
    () => {
      const bundle = readFileSync(BUNDLE_PATH, "utf8");
      const region = bundle.match(/"scrollToSourceLine"[\s\S]{0,800}/);
      expect(region).not.toBeNull();
      // Pre-PR shape: `else if(targetLine<=1)` followed by `scrollTo({top:0,...})`.
      // Now, scrollTo({top:0}) is reached ONLY via the atTop=true branch.
      // The pre-PR shape had a literal `<=1` comparison; assert that no such
      // comparison guards a scrollTo call in this case body.
      expect(region![0]).not.toMatch(/<=\s*1\s*[\)\s]+[^;]*scrollTo/);
    }
  );

  test.skipIf(!bundleExists)(
    "shipped bundle interpolates across the next anchor, not best.offsetHeight",
    () => {
      const bundle = readFileSync(BUNDLE_PATH, "utf8");
      // The scrollToSourceLine case body, up to the `find` case that follows.
      const region = bundle.match(/"scrollToSourceLine"[\s\S]{0,1200}/);
      expect(region).not.toBeNull();
      // Regression guard: the interpolation must derive its span from the two
      // anchors' rects (best + next), never from a single element's own height.
      // `offsetHeight` in this branch means the tall-embed undershoot bug is back.
      expect(region![0]).not.toMatch(/offsetHeight/);
      // Two getBoundingClientRect() calls remain (best AND next).
      const rectCalls = region![0].match(/getBoundingClientRect/g) ?? [];
      expect(rectCalls.length).toBeGreaterThanOrEqual(2);
    }
  );
});

// ── interpolatedScrollTop: the extracted, shipped interpolation geometry ──────
//
// This is the exact function the bridge calls (frontend/bridge/iframe-bridge.ts
// scrollToSourceLine). Testing it directly closes the gap that let the
// best.offsetHeight undershoot bug ship untested — the older harness above
// reimplements a simplified path and never exercised the interpolation branch.
describe("interpolatedScrollTop (real bridge geometry)", () => {
  it("spans the FULL distance to the next anchor across a tall unannotated gap", () => {
    // Repro of zh-hans/文档/文档.md: p@line9 (top 100, ~40px tall) then a ~450px
    // folder-to-site.html iframe with NO data-source-line, then p@line13 (top
    // 590). Target line 11 is the midpoint of lines 9→13.
    const fraction = (11 - 9) / (13 - 9); // 0.5
    const y = interpolatedScrollTop(/*scrollY*/ 0, /*bestTop*/ 100, /*nextTop*/ 590, fraction);
    // Correct: 0 + 100 + (590-100)*0.5 = 345 — i.e. INSIDE the iframe gap.
    expect(y).toBe(345);
    // The old best.offsetHeight math would have been 0 + 100 + 40*0.5 = 120,
    // undershooting to just below p@9 and never crossing the iframe.
    expect(y).toBeGreaterThan(120);
  });

  it("adds the current scrollY offset (viewport-relative rects → absolute top)", () => {
    const y = interpolatedScrollTop(1000, 50, 250, 0.5);
    expect(y).toBe(1000 + 50 + (250 - 50) * 0.5); // 1150
  });

  it("lands at best (fraction 0) and at next (fraction 1)", () => {
    expect(interpolatedScrollTop(0, 100, 590, 0)).toBe(100);
    expect(interpolatedScrollTop(0, 100, 590, 1)).toBe(590);
  });

  it("clamps a degenerate/inverted anchor pair to best (no upward scroll)", () => {
    // next visually at or above best → span floored to 0, land at best.
    expect(interpolatedScrollTop(0, 300, 200, 0.5)).toBe(300);
  });
});

// ── The caret half of the sync (2026-08-21) ────────────────────────────────
//
// Before it, the preview followed the editor only when the SCROLLBAR moved:
// clicking, jumping with a keystroke, or typing on a line already on screen
// moved nothing, so the preview could sit on a different part of the page than
// the one being edited until a scroll shook it loose. The caret trigger fixes
// that, and this predicate is what keeps it from being unbearable — it fires on
// every keystroke, so a sync that always centred would twitch the page
// continuously while the author typed.

describe('isAlreadyShowing (the caret sync\'s quiet band)', () => {
  const H = 1000;

  test('the middle of the viewport is already showing — do not move', () => {
    expect(isAlreadyShowing(500, H)).toBe(true);
    expect(isAlreadyShowing(150, H)).toBe(true);   // exactly the top edge of the band
    expect(isAlreadyShowing(850, H)).toBe(true);   // exactly the bottom edge
  });

  test('the extreme edges are not — a target there is about to leave', () => {
    expect(isAlreadyShowing(149, H)).toBe(false);
    expect(isAlreadyShowing(851, H)).toBe(false);
  });

  test('off screen either way is not', () => {
    expect(isAlreadyShowing(-200, H)).toBe(false);  // scrolled past above
    expect(isAlreadyShowing(1400, H)).toBe(false);  // below the fold
  });

  test('the band scales with the viewport, not a fixed pixel count', () => {
    // A short preview pane keeps the same proportion; a fixed margin would
    // leave a tall pane twitchy and a short one permanently quiet.
    expect(isAlreadyShowing(40, 200)).toBe(true);
    expect(isAlreadyShowing(40, 1000)).toBe(false);
  });
});
