/**
 * Tests for the scroll-position message emitted by iframe-bridge.ts.
 *
 * The bridge observes user scroll in the preview and posts
 * `{ type: 'moss-scroll-position', url, line }` to the parent window.
 * The `url` field is required so the parent can validate that the
 * reported line belongs to the currently-active page (and not a page
 * that was just superseded by a navigation).
 *
 * Since iframe-bridge.ts is an IIFE injected into the preview iframe,
 * we test the emitted payload shape by:
 *   (a) running a local harness that mirrors the bridge's
 *       reportScrollPosition() logic, and
 *   (b) asserting the shipped, minified bundle contains the URL field
 *       so the behavior can't regress silently.
 */
import { describe, it, test, expect, beforeEach, vi } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { interpolatedScrollTop, isAlreadyShowing } from "../scroll-interpolate";

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

/**
 * Mirrors iframe-bridge.ts reportScrollPosition(). Reports the annotated element
 * ON the focus line (FOCUS_FRACTION = 0.5 of the viewport) — the last element at
 * or above it — falling back to the topmost element when the focus line is above
 * them all. Then posts { type, url, line } to window.parent.
 */
const FOCUS_FRACTION = 0.5;
function reportScrollPosition(win: Window & typeof globalThis): void {
  const els = win.document.querySelectorAll(
    "[data-source-line], [data-source-range]"
  );
  const focusY = win.innerHeight * FOCUS_FRACTION;
  let focusEl: Element | null = null;
  let focusElTop = -Infinity;
  let topEl: Element | null = null;
  let topElTop = Infinity;
  let topLine = 0;

  for (const el of Array.from(els)) {
    const top = el.getBoundingClientRect().top;
    if (top <= focusY && top > focusElTop) { focusElTop = top; focusEl = el; }
    if (top >= -10 && top < topElTop) { topElTop = top; topEl = el; }
  }
  const chosen = focusEl ?? topEl;
  if (chosen) {
    const raw = chosen.getAttribute("data-source-line");
    topLine = raw ? parseInt(raw, 10) : 0;
  }

  if (topLine > 0) {
    win.parent.postMessage(
      { type: "moss-scroll-position", url: win.location.href, line: topLine },
      "*"
    );
  }
}

describe("iframe-bridge scroll position messages", () => {
  type PostedMsg = { type: string; url?: string; line?: number };
  let posted: PostedMsg[];
  let win: Window & typeof globalThis;

  beforeEach(() => {
    posted = [];
    // Pretend window.parent is a separate object with a postMessage spy.
    const parentStub = { postMessage: vi.fn((msg: PostedMsg) => posted.push(msg)) };

    // Build a minimal DOM with a source-line-tagged element positioned
    // in the upper 30% of the viewport.
    document.body.innerHTML =
      '<p data-source-line="42" id="target">hello</p>';
    const target = document.getElementById("target")!;
    vi.spyOn(target, "getBoundingClientRect").mockReturnValue({
      top: 10,
      left: 0,
      right: 100,
      bottom: 30,
      width: 100,
      height: 20,
      x: 0,
      y: 10,
      toJSON() {},
    } as DOMRect);

    // Override innerHeight so 10px top falls into the upper 30%.
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 800,
    });

    // jsdom doesn't allow reassigning window.location; use defineProperty.
    Object.defineProperty(window, "parent", {
      configurable: true,
      value: parentStub,
    });

    // Force a known href without navigating jsdom.
    history.replaceState(null, "", "/posts/foo/");

    win = window as unknown as Window & typeof globalThis;
  });

  it("moss-scroll-position includes url field matching location.href", () => {
    reportScrollPosition(win);

    const scrollMsg = posted.find((m) => m.type === "moss-scroll-position");
    expect(scrollMsg).toBeDefined();
    expect(scrollMsg!.url).toBe(window.location.href);
    expect(scrollMsg!.url).toContain("/posts/foo/");
    expect(typeof scrollMsg!.line).toBe("number");
    expect(scrollMsg!.line).toBe(42);
  });

  test.skipIf(!bundleExists)(
    "shipped iframe-bridge.js bundle emits url alongside moss-scroll-position",
    () => {
      const bundle = readFileSync(BUNDLE_PATH, "utf8");

      // Find the minified moss-scroll-position emit site and verify it
      // carries both url (from location.href) and line in the same object
      // literal. The bundle is minified, so we match loosely.
      const scrollEmitRegion = bundle.match(
        /"moss-scroll-position"[\s\S]{0,200}/
      );
      expect(scrollEmitRegion).not.toBeNull();
      const region = scrollEmitRegion![0];
      expect(region).toMatch(/url\s*:\s*[a-zA-Z_$][\w$.]*\.href|location\.href/);
      expect(region).toMatch(/line\s*:/);
    }
  );
});

// ── Task 1: preview reports line 1 at literal top ─────────────────────

const TOP_THRESHOLD = 4;

/** Post-fix harness mirroring reportScrollPosition. When scrollY < threshold
 * AND no annotated element was found in the upper 30%, send line: 1 so the
 * editor can mirror "at top". Without this, page chrome (site header, hero
 * block) that lacks data-source-line annotations swallows the sync. */
function reportScrollPositionV2(win: Window & typeof globalThis): void {
  const els = win.document.querySelectorAll(
    "[data-source-line], [data-source-range]"
  );
  let topEl: Element | null = null;
  let topLine = 0;

  for (const el of Array.from(els)) {
    const rect = el.getBoundingClientRect();
    if (rect.top >= -10 && rect.top < win.innerHeight * 0.3) {
      if (!topEl || rect.top < topEl.getBoundingClientRect().top) {
        topEl = el;
        const raw = el.getAttribute("data-source-line");
        topLine = raw ? parseInt(raw, 10) : 0;
      }
    }
  }

  if (topLine === 0 && win.scrollY < TOP_THRESHOLD) {
    topLine = 1;
  }

  if (topLine > 0) {
    win.parent.postMessage(
      { type: "moss-scroll-position", url: win.location.href, line: topLine },
      "*"
    );
  }
}

describe("iframe-bridge scroll position at literal top", () => {
  type PostedMsg = { type: string; url?: string; line?: number };
  let posted: PostedMsg[];
  let win: Window & typeof globalThis;

  beforeEach(() => {
    posted = [];
    const parentStub = { postMessage: vi.fn((msg: PostedMsg) => posted.push(msg)) };

    // Page header has NO data-source-line. The first annotated paragraph
    // sits below the upper-30% window (top: 600px in an 800px viewport).
    document.body.innerHTML =
      '<header id="hdr">Site</header>' +
      '<p data-source-line="3" id="below-fold">content</p>';
    const belowFold = document.getElementById("below-fold")!;
    vi.spyOn(belowFold, "getBoundingClientRect").mockReturnValue({
      top: 600, left: 0, right: 100, bottom: 620, width: 100, height: 20, x: 0, y: 600, toJSON() {},
    } as DOMRect);

    Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
    Object.defineProperty(window, "parent", { configurable: true, value: parentStub });
    Object.defineProperty(window, "scrollY", { configurable: true, value: 0 });

    history.replaceState(null, "", "/posts/foo/");
    win = window as unknown as Window & typeof globalThis;
  });

  it("emits line: 1 when scrollY is at the literal top and no element is in the upper 30%", () => {
    reportScrollPositionV2(win);
    const msg = posted.find((m) => m.type === "moss-scroll-position");
    expect(msg).toBeDefined();
    expect(msg!.line).toBe(1);
  });

  it("does NOT emit when scrollY is past the threshold and no element is in upper 30%", () => {
    Object.defineProperty(window, "scrollY", { configurable: true, value: 200 });
    reportScrollPositionV2(win);
    const msg = posted.find((m) => m.type === "moss-scroll-position");
    expect(msg).toBeUndefined();
  });

  test.skipIf(!bundleExists)(
    "shipped iframe-bridge.js bundle emits line:1 fallback when scrollY is at top",
    () => {
      const bundle = readFileSync(BUNDLE_PATH, "utf8");
      // The atTop refactor stores `scrollY < TOP_THRESHOLD` in a local variable,
      // then applies it in a `<var>&&<topLine>===0&&(<topLine>=1)` chain. The
      // minifier renames the identifier AND — now that atBottom is computed in
      // the same scope — folds the declaration into a multi-declarator
      // (`let c=…scrollY<l,u=…,h=…;c&&s===0&&(s=1)`), so match the atTop var
      // across the rest of the declaration and back-reference it at the use site.
      expect(bundle).toMatch(
        /(?:let|var|const)\s+([A-Za-z_$][\w$]*)\s*=\s*[^;]*scrollY\s*<[^;]*;[\s\S]{0,160}\1\s*&&[^;]{0,40}=\s*1\b/
      );
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

const TOP_THRESHOLD_V3 = 4;

/** Post-atTop reportScrollPosition. Always emits atTop boolean. */
function reportScrollPositionV3(win: Window & typeof globalThis): void {
  const els = win.document.querySelectorAll(
    "[data-source-line], [data-source-range]"
  );
  let topEl: Element | null = null;
  let topLine = 0;

  for (const el of Array.from(els)) {
    const rect = el.getBoundingClientRect();
    if (rect.top >= -10 && rect.top < win.innerHeight * 0.3) {
      if (!topEl || rect.top < topEl.getBoundingClientRect().top) {
        topEl = el;
        const raw = el.getAttribute("data-source-line");
        topLine = raw ? parseInt(raw, 10) : 0;
      }
    }
  }

  const atTop = win.scrollY < TOP_THRESHOLD_V3;

  // If at-top but no element matched, fall back to line: 1 so the receiver
  // has some line number to carry. atTop is the authoritative signal.
  if (atTop && topLine === 0) {
    topLine = 1;
  }

  if (topLine > 0) {
    win.parent.postMessage(
      { type: "moss-scroll-position", url: win.location.href, line: topLine, atTop },
      "*"
    );
  }
}

describe("iframe-bridge atTop wire field — emit", () => {
  type PostedMsg = { type: string; url?: string; line?: number; atTop?: boolean };
  let posted: PostedMsg[];

  beforeEach(() => {
    posted = [];
    const parentStub = { postMessage: vi.fn((msg: PostedMsg) => posted.push(msg)) };

    document.body.innerHTML = '<p data-source-line="5" id="p5">x</p>';
    const p5 = document.getElementById("p5")!;
    vi.spyOn(p5, "getBoundingClientRect").mockReturnValue({
      top: 50, left: 0, right: 100, bottom: 70, width: 100, height: 20, x: 0, y: 50, toJSON() {},
    } as DOMRect);
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
    Object.defineProperty(window, "parent", { configurable: true, value: parentStub });
    history.replaceState(null, "", "/posts/foo/");
  });

  it("emits atTop: true with the real line when scrollY=0", () => {
    Object.defineProperty(window, "scrollY", { configurable: true, value: 0 });
    reportScrollPositionV3(window as Window & typeof globalThis);
    const msg = posted.find((m) => m.type === "moss-scroll-position");
    expect(msg).toBeDefined();
    expect(msg!.atTop).toBe(true);
    expect(msg!.line).toBe(5);  // line stays accurate; atTop is the authoritative signal
  });

  it("emits atTop: false when scrollY=500", () => {
    Object.defineProperty(window, "scrollY", { configurable: true, value: 500 });
    reportScrollPositionV3(window as Window & typeof globalThis);
    const msg = posted.find((m) => m.type === "moss-scroll-position");
    expect(msg!.atTop).toBe(false);
  });

  it("at-top with no annotated element falls back to line: 1 and atTop: true", () => {
    document.body.innerHTML = '<header>Site</header>';
    Object.defineProperty(window, "scrollY", { configurable: true, value: 0 });
    reportScrollPositionV3(window as Window & typeof globalThis);
    const msg = posted.find((m) => m.type === "moss-scroll-position");
    expect(msg!.atTop).toBe(true);
    expect(msg!.line).toBe(1);
  });
});

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

// ── atBottom return leg (symmetric with atTop) ───────────────────────────────
//
// The preview reporter must post an atBottom signal so the editor's return
// handler can park at its own bottom instead of chasing a stale upper-30% line
// — the asymmetry that let editor→preview EOF sync bounce. Mirrors the bridge's
// reportScrollPosition atBottom predicate.
describe("reportScrollPosition atBottom edge signal", () => {
  const TOP = 4;
  const BOTTOM = 4;
  function edges(scrollY: number, scrollHeight: number, innerHeight: number) {
    const atTop = scrollY < TOP;
    const maxScroll = scrollHeight - innerHeight;
    const atBottom = scrollY >= maxScroll - BOTTOM;
    return { atTop, atBottom };
  }

  it("is true at the page bottom (scrollY at max)", () => {
    // 5000px page, 800px viewport → max scroll 4200.
    expect(edges(4200, 5000, 800).atBottom).toBe(true);
    expect(edges(4198, 5000, 800).atBottom).toBe(true); // within 4px slack
    expect(edges(4200, 5000, 800).atTop).toBe(false);
  });

  it("is false in the middle of a tall page", () => {
    expect(edges(1000, 5000, 800).atBottom).toBe(false);
    expect(edges(1000, 5000, 800).atTop).toBe(false);
  });

  it("a page too short to scroll resolves to top (both flags true, atTop wins)", () => {
    // maxScroll <= 0, so any scrollY >= maxScroll-4 → atBottom, and scrollY≈0 → atTop.
    const e = edges(0, 600, 800);
    expect(e.atTop).toBe(true);
    expect(e.atBottom).toBe(true); // receiver checks atTop first → parks at top
  });
});

describe("shipped bundle emits the atBottom return signal", () => {
  test.skipIf(!bundleExists)(
    "moss-scroll-position emit carries atBottom alongside line + atTop",
    () => {
      const bundle = readFileSync(BUNDLE_PATH, "utf8");
      const region = bundle.match(/"moss-scroll-position"[\s\S]{0,200}/);
      expect(region).not.toBeNull();
      expect(region![0]).toMatch(/atBottom\s*:/);
    }
  );
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
