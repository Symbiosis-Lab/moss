/**
 * Regression test for the "Aa font panel is silently dead after a same-URL
 * morph introduces its markup for the first time" bug.
 *
 * Background: `.font-anchor`/`.font-trigger`/`#fontPill` are only emitted
 * when the page has `date:` frontmatter (src-tauri/src/build/page/html.rs).
 * `create_files` writes brand-new files completely empty, so the FIRST load
 * of a just-created page has no font-panel markup at all — `initFontPanel()`
 * (which runs once at module-init / DOMContentLoaded) legitimately no-ops.
 *
 * When the user later adds `date:` while the page stays open at the same
 * URL, the rebuild goes through the in-place idiomorph morph (not a reload).
 * Since the font-panel subtree has no prior counterpart, idiomorph CREATES
 * it as brand-new DOM nodes — nodes `initFontPanel()` has never seen, so
 * they carry zero click listeners. Nothing re-runs site-script init after a
 * morph, so the Aa button is a permanent, silent no-op.
 *
 * The fix: the bridge dispatches a `moss-morph-patched` CustomEvent on
 * `document` right after a morph is applied; theme.ts listens for it and
 * re-invokes `initFontPanel()` (idempotent per-element via
 * `pill.dataset.initialized`, so calling it again only binds whatever
 * wasn't already bound).
 */

import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";

/** DOM state mirroring a brand-new page's first load: no `date:`
 *  frontmatter yet, so html.rs never emitted the font-panel subtree. */
function setupPageWithoutFontPanel(): void {
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.className = "";
  document.body.innerHTML = `<article><h1>Auguries of Innocence</h1></article>`;
}

/**
 * Mirrors what idiomorph's `createNode` does when a same-URL morph
 * introduces an id-bearing subtree that has no old counterpart: the
 * elements are constructed fresh and inserted, never touched by any
 * script. Using `insertAdjacentHTML` produces exactly that — brand-new
 * nodes with zero listeners bound, regardless of what ran earlier.
 */
function morphInFontPanelMarkup(): void {
  document.querySelector("article")!.insertAdjacentHTML(
    "beforeend",
    `<div class="date-line">
      <span class="date">2026 · 7 · 5</span>
      <div class="font-anchor">
        <button class="font-trigger size-std"></button>
        <div class="font-pill" id="fontPill">
          <button data-scale="small"></button>
          <button data-scale="" class="active"></button>
          <button data-scale="large"></button>
          <button data-scale="xlarge"></button>
        </div>
      </div>
    </div>`,
  );
}

function flushAnimations(): void {
  vi.advanceTimersByTime(500);
}

beforeEach(() => {
  localStorage.clear();
  vi.useFakeTimers();
  setupPageWithoutFontPanel();
});

afterEach(() => {
  localStorage.clear();
  vi.useRealTimers();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.className = "";
});

describe("font panel re-init after a same-URL morph introduces its markup", () => {
  test("clicking the Aa trigger opens the pill once the bridge announces the morph, even though the markup didn't exist at script-init time", async () => {
    // 1. First load: no `date:` yet, so theme.ts's module-init call to
    //    initFontPanel() finds nothing and no-ops (matches the real bug's
    //    "brand-new empty file" first render).
    vi.resetModules();
    await import("../theme");

    // 2. The user adds `date:` frontmatter; the same-URL rebuild morphs the
    //    font-panel subtree into existence as fresh, listener-less nodes.
    morphInFontPanelMarkup();

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    const pill = document.querySelector(".font-pill") as HTMLElement;

    // Sanity check: freshly-morphed-in nodes start unbound (the silent-no-op
    // bug, if nothing re-runs init).
    trigger.click();
    flushAnimations();
    expect(pill.classList.contains("visible")).toBe(false);

    // 3. The bridge finishes applying the morph and announces it.
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));

    // 4. The trigger must now actually work — this is the assertion that
    //    fails without the post-morph re-init hook.
    trigger.click();
    flushAnimations();
    expect(pill.classList.contains("visible")).toBe(true);
  });

  test("a second moss-morph-patched event (e.g. a later, content-only rebuild) does not double-bind the already-initialized panel", async () => {
    // Bind once, exactly as the previous test does.
    vi.resetModules();
    await import("../theme");
    morphInFontPanelMarkup();
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));

    const trigger = document.querySelector(".font-trigger") as HTMLElement;
    const pill = document.querySelector(".font-pill") as HTMLElement;
    expect((pill as HTMLElement).dataset.initialized).toBe("1");

    // A later morph that leaves the SAME panel nodes in place (e.g. a
    // content-only rebuild elsewhere on the page) fires the event again.
    // `initFontPanel`'s `pill.dataset.initialized` guard must make this a
    // complete no-op — no second set of listeners.
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));
    expect((pill as HTMLElement).dataset.initialized).toBe("1");

    // If a second listener set had been registered, a single click would be
    // handled twice (open() then immediately close(), or vice versa) and
    // the pill would NOT end up open after one click. A clean single-click
    // open proves exactly one listener set is bound.
    trigger.click();
    flushAnimations();
    expect(pill.classList.contains("visible")).toBe(true);
  });
});
