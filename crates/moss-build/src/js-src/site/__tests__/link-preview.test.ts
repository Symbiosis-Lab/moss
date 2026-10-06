/**
 * Tests for link-preview.ts — hover preview cards for internal wikilinks.
 *
 * Regression guard for the CJK-path bug: `previews.json` keys are raw UTF-8
 * paths (`/zh-hans/文档/from-matters/`), but `URL.pathname` percent-encodes
 * non-ASCII segments (文档 → %E6%96%87%E6%A1%A3). The lookup must decode the
 * pathname or the popup silently never shows for any Chinese/Unicode path.
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";

// link-preview.ts is an IIFE side-effect script — importing it registers the
// document-level mouseover/focus listeners against jsdom's document.
import "../link-preview";

const PREVIEWS: Record<string, { title: string; description?: string }> = {
  // Keys are DECODED (raw UTF-8), exactly as moss emits them from `url_path`.
  "/zh-hans/文档/from-matters/": { title: "从 Matters 导入", description: "从 Matters.town 导入文章" },
  "/docs/from-matters/": { title: "Import from Matters" },
};

/** Run the 200ms hover-show delay forward without spending it. */
const wait = async (ms: number): Promise<void> => { await vi.advanceTimersByTimeAsync(ms); };

describe("link-preview — hover popup", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    // Clear the LINKS, not the whole body. The module appends its popup once
    // and keeps a singleton reference to it, so wiping `body.innerHTML` orphans
    // that node for good: every later test then hovers a link, `createPopup`
    // returns early because the reference is still live, and nothing appears in
    // the DOM to assert on. Hiding the popup is the reset it actually needs.
    document.body.querySelectorAll("a").forEach((a) => a.remove());
    document.querySelector(".moss-preview-popup")?.classList.remove("visible");
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve({ json: () => Promise.resolve(PREVIEWS) } as Response)),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.querySelectorAll("a").forEach((a) => a.remove());
    vi.unstubAllGlobals();
    for (const prop of ["clientWidth", "clientHeight"]) {
      Object.defineProperty(document.documentElement, prop, { configurable: true, value: 0 });
    }
  });

  test("shows a preview card for a CJK-path wikilink (decodes pathname to match previews.json)", async () => {
    const a = document.createElement("a");
    a.className = "wikilink";
    // Same-origin href with a percent-encoded CJK segment (文档), as the browser
    // produces it. Use location.origin so it isn't treated as an external link.
    a.href = location.origin + "/zh-hans/%E6%96%87%E6%A1%A3/from-matters/";
    a.textContent = "从 Matters 导入";
    document.body.appendChild(a);

    a.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await wait(300); // 200ms show delay + async previews.json fetch

    const popup = document.querySelector(".moss-preview-popup");
    expect(popup, "preview popup should be created for a CJK-path wikilink").not.toBeNull();
    expect(popup?.classList.contains("visible")).toBe(true);
    expect(popup?.textContent).toContain("从 Matters 导入");
    expect(popup?.textContent).toContain("从 Matters.town 导入文章");
  });

  test("clamps a right-edge card to the visible band, not to the window", async () => {
    // The distinction the whole clamp turns on: a 1024px window that reserves a
    // 24px scrollbar shows 1000px. Clamping on `window.innerWidth` puts the
    // card's right edge at 1016 — under the scrollbar, cropped — where
    // `documentElement.clientWidth` puts it at 992.
    Object.defineProperty(document.documentElement, "clientWidth", {
      configurable: true,
      value: 1000,
    });
    Object.defineProperty(document.documentElement, "clientHeight", {
      configurable: true,
      value: 768,
    });

    const a = document.createElement("a");
    a.className = "wikilink";
    a.href = location.origin + "/docs/from-matters/";
    a.textContent = "Import from Matters";
    a.getBoundingClientRect = vi.fn(
      () => ({ top: 300, bottom: 320, left: 900, right: 980, width: 80, height: 20 }) as DOMRect,
    );
    document.body.appendChild(a);

    // First hover creates the singleton popup; only then can its own rect be
    // stubbed, because the module never exposes the node.
    a.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await wait(300);
    const popup = document.querySelector(".moss-preview-popup") as HTMLElement;
    popup.getBoundingClientRect = vi.fn(
      () => ({ top: 0, bottom: 120, left: 0, right: 360, width: 360, height: 120 }) as DOMRect,
    );

    a.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    a.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await wait(300);

    expect(parseFloat(popup.style.left)).toBe(1000 - 8 - 360);
    // Below the link, which has room — the vertical slot only flips near the
    // bottom of the window.
    expect(parseFloat(popup.style.top)).toBe(328);
  });

  test("does not show a card for a non-wikilink internal link", async () => {
    const a = document.createElement("a");
    a.href = location.origin + "/docs/from-matters/"; // no .wikilink class
    a.textContent = "plain link";
    document.body.appendChild(a);

    a.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await wait(300);

    const popup = document.querySelector(".moss-preview-popup.visible");
    expect(popup).toBeNull();
  });
});
