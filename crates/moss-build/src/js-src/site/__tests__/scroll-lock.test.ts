/**
 * The page scroll lock behind the search panel and the picture viewer: it
 * keeps a classic scrollbar's gutter while locked, so the page does not shift
 * sideways, and adds none to a page that had no scrollbar.
 */
import { afterEach, describe, expect, test } from "vitest";
import { lockPageScroll } from "../viewport";

const root = document.documentElement;

/** A page whose scrollbar is `bar` px wide, measured as Chromium reports it:
 * once overflow is hidden `clientWidth` is the full width, gutter or not, and
 * only the root's own width shows whether the gutter was kept. */
function engine(bar: number, gutterKept: boolean): void {
  const locked = () => root.style.overflow === "hidden";
  Object.defineProperty(root, "clientWidth", {
    configurable: true,
    get: () => window.innerWidth - (locked() ? 0 : bar),
  });
  root.getBoundingClientRect = () => {
    const kept = gutterKept && root.style.scrollbarGutter === "stable";
    return { width: window.innerWidth - (locked() && !kept ? 0 : bar) } as DOMRect;
  };
}

afterEach(() => {
  root.removeAttribute("style");
  delete (root as { clientWidth?: number }).clientWidth;
  delete (root as { getBoundingClientRect?: unknown }).getBoundingClientRect;
});

describe("lockPageScroll", () => {
  test("a page with a classic scrollbar keeps its gutter while locked", () => {
    engine(15, true);
    const unlock = lockPageScroll();
    expect(root.style.overflow).toBe("hidden");
    expect(root.style.scrollbarGutter).toBe("stable");
    unlock();
    expect(root.style.overflow).toBe("");
    expect(root.style.scrollbarGutter).toBe("");
  });

  test("a page without a scrollbar gains no gutter", () => {
    engine(0, true);
    const release = lockPageScroll();
    expect(root.style.overflow).toBe("hidden");
    expect(root.style.scrollbarGutter).toBe("");
    release();
  });

  test("a classic scrollbar's kept gutter adds no padding", () => {
    engine(15, true);
    const release = lockPageScroll();
    expect(root.style.scrollbarGutter).toBe("stable");
    expect(root.style.paddingRight).toBe("");
    release();
  });

  test("a styled scrollbar the gutter does not keep is padded back, and unpadded on release", () => {
    // Where the platform overlays its scrollbars, the gutter it keeps is none.
    engine(15, false);
    const release = lockPageScroll();
    expect(root.style.paddingRight).toBe("15px");
    release();
    expect(root.style.paddingRight).toBe("");
  });

  test("unlocking restores the inline styles the page already had", () => {
    engine(15, true);
    root.style.overflow = "clip";
    root.style.scrollbarGutter = "stable both-edges";
    lockPageScroll()();
    expect(root.style.overflow).toBe("clip");
    expect(root.style.scrollbarGutter).toBe("stable both-edges");
  });

  test("nested locks release in any order, and only the last one unlocks", () => {
    engine(15, true);
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
