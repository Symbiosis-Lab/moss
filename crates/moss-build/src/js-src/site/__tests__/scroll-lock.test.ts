/**
 * The page scroll lock behind the search panel and the picture viewer: it
 * keeps a classic scrollbar's gutter while locked, so the page does not shift
 * sideways, and adds none to a page that had no scrollbar.
 */
import { afterEach, describe, expect, test, vi } from "vitest";
import { holdPageScroll } from "../viewport";

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
  delete (window as { __mossPageScroll?: unknown }).__mossPageScroll;
});

describe("holdPageScroll", () => {
  test("a page with a classic scrollbar keeps its gutter while locked", () => {
    engine(15, true);
    holdPageScroll("search", true);
    expect(root.style.overflow).toBe("hidden");
    expect(root.style.scrollbarGutter).toBe("stable");
    holdPageScroll("search", false);
    expect(root.style.overflow).toBe("");
    expect(root.style.scrollbarGutter).toBe("");
  });

  test("a page without a scrollbar gains no gutter", () => {
    engine(0, true);
    holdPageScroll("search", true);
    expect(root.style.overflow).toBe("hidden");
    expect(root.style.scrollbarGutter).toBe("");
    holdPageScroll("search", false);
  });

  test("a classic scrollbar's kept gutter adds no padding", () => {
    engine(15, true);
    holdPageScroll("search", true);
    expect(root.style.scrollbarGutter).toBe("stable");
    expect(root.style.paddingRight).toBe("");
    holdPageScroll("search", false);
  });

  test("a styled scrollbar the gutter does not keep is padded back, and unpadded on release", () => {
    // Where the platform overlays its scrollbars, the gutter it keeps is none.
    engine(15, false);
    holdPageScroll("search", true);
    expect(root.style.paddingRight).toBe("15px");
    holdPageScroll("search", false);
    expect(root.style.paddingRight).toBe("");
  });

  test("unlocking restores the inline styles the page already had", () => {
    engine(15, true);
    root.style.overflow = "clip";
    root.style.scrollbarGutter = "stable both-edges";
    holdPageScroll("search", true);
    holdPageScroll("search", false);
    expect(root.style.overflow).toBe("clip");
    expect(root.style.scrollbarGutter).toBe("stable both-edges");
  });

  test("owners nest, and only the last release unlocks", () => {
    engine(15, true);
    holdPageScroll("fullscreen", true);
    holdPageScroll("search", true);
    holdPageScroll("fullscreen", false);
    expect(root.style.overflow).toBe("hidden");
    holdPageScroll("fullscreen", false);
    expect(root.style.overflow, "releasing an owner that holds nothing changes nothing").toBe("hidden");
    holdPageScroll("search", true);
    holdPageScroll("search", false);
    expect(root.style.overflow).toBe("");
    expect(root.style.scrollbarGutter).toBe("");
  });

  test("bundles that each carry their own copy of the module share one lock", async () => {
    engine(15, true);
    vi.resetModules();
    const a = await import("../viewport");
    vi.resetModules();
    const b = await import("../viewport");
    expect(a.holdPageScroll).not.toBe(b.holdPageScroll);

    a.holdPageScroll("menu", true);
    b.holdPageScroll("search", true);
    a.holdPageScroll("menu", false);
    expect(root.style.overflow).toBe("hidden");
    b.holdPageScroll("search", false);
    expect(root.style.overflow).toBe("");
    expect(root.style.scrollbarGutter).toBe("");
  });
});
