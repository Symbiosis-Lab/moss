/**
 * Tests for the morphing font control.
 *
 * Design: A single glyph ("字" or "Aa") sits next to the date at the
 * currently-selected font size. Clicking it morphs it into a 4-button
 * pill (the glyph slides into position, then the pill fades in around
 * it). Selecting a size updates the glyph and the document scale.
 * Dismissing (click-outside or scroll) reverses the morph — the pill
 * fades out, the glyph reappears at the active button's position, and
 * slides back to its rest spot.
 *
 * Key invariant: the trigger and the pill are never both visible at the
 * same time. When the pill is visible, the trigger is hidden (and vice
 * versa). This creates the illusion of a single element morphing.
 *
 * DOM structure:
 *   .date-line
 *     .date
 *     .font-anchor          (relative wrapper for trigger + pill)
 *       .font-trigger       (position: absolute, overlays first pill button)
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
});
