/**
 * Tests for the morphing font control.
 *
 * Design: A single glyph ("字" or "Aa") sits next to the date, at a fixed
 * slot the pill's own first button always shares — no size is ever
 * "active" in a way that moves either one. Clicking the glyph grows a
 * 4-button pill out of that slot. Selecting a size updates the glyph's
 * `size-*` co-class and the document scale; while the pill stays open, the
 * scale change reflows the page around the control (title, column width),
 * so theme.ts holds `.font-anchor` in place against that reflow until the
 * pill closes. jsdom has no layout, so the hold itself — and the geometry
 * invariants it exists for — are covered by the render-gate under
 * tests/render-gates/site/vertical-nav-chrome.spec.ts, not here. Dismissing
 * (click-outside, Escape, or scroll) closes the pill and releases the hold.
 *
 * Key invariant: the trigger and the pill are never both visible at the
 * same time. When the pill is visible, the trigger is hidden (and vice
 * versa). This creates the illusion of a single element morphing.
 *
 * DOM structure:
 *   .date-line
 *     .date
 *     .font-anchor          (relative wrapper for trigger + pill)
 *       .font-trigger       (position: absolute, fixed slot shared with the pill's first button)
 *       .font-pill#fontPill  (always in layout via visibility:hidden)
 *         button[data-scale] × 4
 *
 * Theme toggle tests are in theme-toggle.test.ts.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";

let initFontPanel: () => void;

function setupDOM(): void {
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.className = "";
  document.body.innerHTML = `
    <div class="date-line">
      <span class="date">2026 · 1 · 1</span>
      <div class="font-anchor">
        <button class="font-trigger size-std"></button>
        <div class="font-pill" id="fontPill">
          <button data-scale="small"></button>
          <button data-scale="" class="active"></button>
          <button data-scale="large"></button>
          <button data-scale="xlarge"></button>
        </div>
      </div>
    </div>
  `;
}

beforeEach(() => {
  localStorage.clear();
  setupDOM();
});

afterEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.className = "";
});

describe("Morphing font control", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const mod = await import("../theme");
    initFontPanel = mod.initFontPanel;
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  /** Flush all pending animation timeouts (slide + fade) */
  function flushAnimations(): void {
    vi.advanceTimersByTime(500);
  }

  test("clicking trigger opens pill and hides trigger", () => {
    initFontPanel();

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    const pill = document.querySelector(".font-pill") as HTMLElement;

    // Initially: trigger visible, pill hidden
    expect(trigger.classList.contains("hidden")).toBe(false);
    expect(pill.classList.contains("visible")).toBe(false);

    // Click to open, flush animation timeouts
    trigger.click();
    flushAnimations();

    // After open: trigger hidden, pill visible
    expect(trigger.classList.contains("hidden")).toBe(true);
    expect(pill.classList.contains("visible")).toBe(true);
  });

  test("clicking trigger again closes pill and shows trigger", () => {
    initFontPanel();

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    const pill = document.querySelector(".font-pill") as HTMLElement;

    // Open
    trigger.click();
    flushAnimations();
    expect(pill.classList.contains("visible")).toBe(true);

    // Close
    trigger.click();
    flushAnimations();
    expect(trigger.classList.contains("hidden")).toBe(false);
    expect(pill.classList.contains("visible")).toBe(false);
  });

  test("click outside dismisses pill and shows trigger", () => {
    initFontPanel();

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    const pill = document.querySelector(".font-pill") as HTMLElement;

    trigger.click();
    flushAnimations();
    expect(pill.classList.contains("visible")).toBe(true);

    // Click outside
    document.body.click();
    flushAnimations();
    expect(trigger.classList.contains("hidden")).toBe(false);
    expect(pill.classList.contains("visible")).toBe(false);
  });

  test("scroll dismisses pill and shows trigger", () => {
    initFontPanel();

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    const pill = document.querySelector(".font-pill") as HTMLElement;

    trigger.click();
    flushAnimations();
    expect(pill.classList.contains("visible")).toBe(true);

    window.dispatchEvent(new Event("scroll"));
    flushAnimations();
    expect(trigger.classList.contains("hidden")).toBe(false);
    expect(pill.classList.contains("visible")).toBe(false);
  });

  test("font scale — selecting small updates document class and localStorage", () => {
    initFontPanel();

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    trigger.click();
    flushAnimations();

    const btn = document.querySelector('[data-scale="small"]') as HTMLElement;
    btn.click();

    expect(document.documentElement.classList.contains("scale-small")).toBe(true);
    expect(localStorage.getItem("moss-font-scale")).toBe("small");
    expect(btn.classList.contains("active")).toBe(true);
  });

  test("font scale — selecting standard removes scale class", () => {
    initFontPanel();

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    trigger.click();
    flushAnimations();

    // First set to small
    const smallBtn = document.querySelector('[data-scale="small"]') as HTMLElement;
    smallBtn.click();

    // Then back to standard
    const stdBtn = document.querySelector('[data-scale=""]') as HTMLElement;
    stdBtn.click();

    expect(document.documentElement.classList.contains("scale-small")).toBe(false);
    expect(localStorage.getItem("moss-font-scale")).toBe("");
    expect(stdBtn.classList.contains("active")).toBe(true);
  });

  test("font scale — selecting large", () => {
    initFontPanel();

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    trigger.click();
    flushAnimations();

    const btn = document.querySelector('[data-scale="large"]') as HTMLElement;
    btn.click();

    expect(document.documentElement.classList.contains("scale-large")).toBe(true);
    expect(localStorage.getItem("moss-font-scale")).toBe("large");
  });

  test("trigger size class updates when scale is selected", () => {
    initFontPanel();

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    trigger.click();
    flushAnimations();

    const btn = document.querySelector('[data-scale="large"]') as HTMLElement;
    btn.click();

    expect(trigger.classList.contains("size-large")).toBe(true);
    expect(trigger.classList.contains("size-std")).toBe(false);
  });

  test("restore font scale on init", () => {
    localStorage.setItem("moss-font-scale", "large");
    setupDOM();
    initFontPanel();

    expect(document.documentElement.classList.contains("scale-large")).toBe(true);
    const btn = document.querySelector('[data-scale="large"]') as HTMLElement;
    expect(btn.classList.contains("active")).toBe(true);
    const stdBtn = document.querySelector('[data-scale=""]') as HTMLElement;
    expect(stdBtn.classList.contains("active")).toBe(false);

    // Trigger should have the restored size class
    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    expect(trigger.classList.contains("size-large")).toBe(true);
  });

  test("does not run when .font-trigger is absent", () => {
    document.body.innerHTML = "<p>No trigger here</p>";
    // Should not throw
    initFontPanel();
  });

  test("font scale is restored on pages without font trigger (site-wide)", async () => {
    // Simulate a non-article page: no .font-trigger in DOM
    localStorage.setItem("moss-font-scale", "large");
    document.body.innerHTML = "<p>Homepage content</p>";

    // Re-import the module — the top-level IIFE runs on import
    vi.resetModules();
    await import("../theme");

    // The IIFE should have applied the scale class to <html>
    expect(document.documentElement.classList.contains("scale-large")).toBe(true);
  });

  // jsdom lays nothing out — every rect here is a scripted stand-in for
  // "the page reflowed", not a real measurement — so these test the hold's
  // arithmetic and state machine (accumulate, reset on open/close, skip
  // when closed), not real geometry. Real geometry is the render-gate's job
  // (tests/render-gates/site/vertical-nav-chrome.spec.ts).
  describe("hold-while-open (anchor translate)", () => {
    let rectSpy: ReturnType<typeof vi.spyOn>;

    const rect = (left: number, top: number) =>
      ({ left, top, right: left, bottom: top, width: 0, height: 0, x: left, y: top, toJSON() {} }) as DOMRect;

    beforeEach(() => {
      rectSpy = vi.spyOn(Element.prototype, "getBoundingClientRect");
    });

    afterEach(() => {
      rectSpy.mockRestore();
    });

    test("picking a size while open translates the anchor by the pill's before/after delta, instantly", () => {
      initFontPanel();
      const trigger = document.querySelector(".font-trigger") as HTMLElement;
      const anchor = document.querySelector(".font-anchor") as HTMLElement;

      trigger.click();
      flushAnimations();

      // Pill measured at (100, 50) before the scale-class swap reflows the
      // page, (130, 60) after — the hold must cancel exactly that 30/10 shift.
      rectSpy.mockReturnValueOnce(rect(100, 50)).mockReturnValueOnce(rect(130, 60));
      (document.querySelector('[data-scale="large"]') as HTMLElement).click();

      expect(anchor.style.translate).toBe("-30px -10px");
      // "Instantly" is `transition: none` written in the same step as the
      // translate — site.css's own `.font-anchor` transition only animates
      // a change when this inline override isn't present (see close() below).
      expect(anchor.style.transition).toBe("none");
    });

    test("a second pick while still open accumulates onto the running hold, each one instant", () => {
      initFontPanel();
      const trigger = document.querySelector(".font-trigger") as HTMLElement;
      const anchor = document.querySelector(".font-anchor") as HTMLElement;

      trigger.click();
      flushAnimations();

      rectSpy.mockReturnValueOnce(rect(0, 0)).mockReturnValueOnce(rect(20, 0));
      (document.querySelector('[data-scale="large"]') as HTMLElement).click();
      expect(anchor.style.translate).toBe("-20px 0px");
      expect(anchor.style.transition).toBe("none");

      rectSpy.mockReturnValueOnce(rect(0, 0)).mockReturnValueOnce(rect(-5, 0));
      (document.querySelector('[data-scale="xlarge"]') as HTMLElement).click();
      expect(anchor.style.translate).toBe("-15px 0px");
      expect(anchor.style.transition).toBe("none");
    });

    test("restoring a saved scale on load writes no translate and no transition override — the pill is closed", () => {
      localStorage.setItem("moss-font-scale", "large");
      // A huge, obviously-wrong delta: if restore ever read it, the
      // assertion below would fail loudly instead of passing by accident.
      rectSpy.mockReturnValue(rect(999, 999));
      setupDOM();
      initFontPanel();

      const anchor = document.querySelector(".font-anchor") as HTMLElement;
      expect(anchor.style.translate).toBe("");
      expect(anchor.style.transition).toBe("");
    });

    test("the next open starts from a zero hold, not the previous session's, and close released the transition override too", () => {
      initFontPanel();
      const trigger = document.querySelector(".font-trigger") as HTMLElement;
      const anchor = document.querySelector(".font-anchor") as HTMLElement;

      trigger.click();
      flushAnimations();
      rectSpy.mockReturnValueOnce(rect(0, 0)).mockReturnValueOnce(rect(40, 0));
      (document.querySelector('[data-scale="large"]') as HTMLElement).click();
      expect(anchor.style.transition).toBe("none");

      trigger.click(); // close, releases the hold
      flushAnimations();
      expect(anchor.style.translate).toBe("");
      // The release clears the inline override together with the translate,
      // so site.css's own transition is what's left to animate it — a bare
      // "none" left behind here would make the release jump instead.
      expect(anchor.style.transition).toBe("");

      trigger.click(); // reopen
      flushAnimations();
      expect(anchor.style.translate).toBe("");

      // Picking the size already active — before/after both report the
      // same rect (no reflow) — must yield exactly a zero-delta hold, not
      // whatever total the previous open session left off at.
      rectSpy.mockReturnValueOnce(rect(0, 0)).mockReturnValueOnce(rect(0, 0));
      (document.querySelector('[data-scale="large"]') as HTMLElement).click();
      expect(anchor.style.translate).toBe("0px 0px");
      expect(anchor.style.transition).toBe("none");
    });
  });
});
