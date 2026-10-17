/**
 * The page scroll lock behind the search panel and the picture viewer: it
 * keeps a classic scrollbar's gutter while locked, so the page does not shift
 * sideways, and adds none to a page that had no scrollbar.
 */
import { afterEach, describe, expect, test } from "vitest";
import { lockPageScroll } from "../viewport";

const root = document.documentElement;

function pageWith(scrollbarWidth: number): void {
  Object.defineProperty(root, "clientWidth", { configurable: true, value: window.innerWidth - scrollbarWidth });
}

afterEach(() => {
  root.removeAttribute("style");
  delete (root as { clientWidth?: number }).clientWidth;
});

describe("lockPageScroll", () => {
  test("a page with a classic scrollbar keeps its gutter while locked", () => {
    pageWith(15);
    const unlock = lockPageScroll();
    expect(root.style.overflow).toBe("hidden");
    expect(root.style.scrollbarGutter).toBe("stable");
    unlock();
    expect(root.style.overflow).toBe("");
    expect(root.style.scrollbarGutter).toBe("");
  });

  test("a page without a scrollbar gains no gutter", () => {
    pageWith(0);
    const release = lockPageScroll();
    expect(root.style.overflow).toBe("hidden");
    expect(root.style.scrollbarGutter).toBe("");
    release();
  });

  test("a styled scrollbar the gutter does not keep is padded back, and unpadded on release", () => {
    // Where the platform overlays its scrollbars, a page's own 15px scrollbar
    // vanishes when the root stops scrolling, gutter or not.
    Object.defineProperty(root, "clientWidth", {
      configurable: true,
      get: () => window.innerWidth - (root.style.overflow === "hidden" ? 0 : 15),
    });
    const release = lockPageScroll();
    expect(root.style.paddingRight).toBe("15px");
    release();
    expect(root.style.paddingRight).toBe("");
  });

  test("a gutter that keeps the scrollbar's width adds no padding", () => {
    pageWith(15);
    const release = lockPageScroll();
    expect(root.style.paddingRight).toBe("");
    release();
  });

  test("unlocking restores the inline styles the page already had", () => {
    pageWith(15);
    root.style.overflow = "clip";
    root.style.scrollbarGutter = "stable both-edges";
    lockPageScroll()();
    expect(root.style.overflow).toBe("clip");
    expect(root.style.scrollbarGutter).toBe("stable both-edges");
  });

  test("nested locks release in any order, and only the last one unlocks", () => {
    pageWith(15);
    const releaseViewer = lockPageScroll();
    const releaseSearch = lockPageScroll();
    releaseViewer();
    expect(root.style.overflow).toBe("hidden");
    releaseViewer();
    expect(root.style.overflow, "a second release of the same lock changes nothing").toBe("hidden");
    releaseSearch();
    expect(root.style.overflow).toBe("");
    expect(root.style.scrollbarGutter).toBe("");
  });
});
