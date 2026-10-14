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

/** A same-origin wikilink to a page that has a preview entry. */
const link = (text = "Import from Matters"): string =>
  `<a class="wikilink" href="${location.origin}/docs/from-matters/">${text}</a>`;

/**
 * Put `html` inside a `<main>` (the article element the shell renders) and
 * return its first anchor. Links outside `main` never preview, so every test
 * that expects a card has to mount its anchor here.
 */
function mount(html: string): HTMLAnchorElement {
  const main = document.createElement("main");
  main.innerHTML = html;
  document.body.appendChild(main);
  return main.querySelector("a") as HTMLAnchorElement;
}

/** Hover `a`, run the show delay, and report whether a card opened. */
async function hovers(a: HTMLAnchorElement): Promise<boolean> {
  a.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  await wait(300);
  return document.querySelector(".moss-preview-popup.visible") !== null;
}

describe("link-preview — hover popup", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    // Clear the mounted content, not the whole body. The module appends its popup once
    // and keeps a singleton reference to it, so wiping `body.innerHTML` orphans
    // that node for good: every later test then hovers a link, `createPopup`
    // returns early because the reference is still live, and nothing appears in
    // the DOM to assert on. Hiding the popup is the reset it actually needs.
    document.body.querySelectorAll("main").forEach((m) => m.remove());
    document.querySelector(".moss-preview-popup")?.classList.remove("visible");
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve({ json: () => Promise.resolve(PREVIEWS) } as Response)),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.querySelectorAll("main").forEach((m) => m.remove());
    document.body.querySelectorAll(":scope > p, :scope > a").forEach((n) => n.remove());
    vi.unstubAllGlobals();
    for (const prop of ["clientWidth", "clientHeight"]) {
      Object.defineProperty(document.documentElement, prop, { configurable: true, value: 0 });
    }
  });

  test("shows a preview card for a CJK-path wikilink (decodes pathname to match previews.json)", async () => {
    // Same-origin href with a percent-encoded CJK segment (文档), as the browser
    // produces it. Use location.origin so it isn't treated as an external link.
    const a = mount(
      `<p><a class="wikilink" href="${location.origin}/zh-hans/%E6%96%87%E6%A1%A3/from-matters/">从 Matters 导入</a></p>`,
    );

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

    const a = mount(`<p>${link()}</p>`);
    a.getBoundingClientRect = vi.fn(
      () => ({ top: 300, bottom: 320, left: 900, right: 980, width: 80, height: 20 }) as DOMRect,
    );

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
    const a = mount(`<p><a href="${location.origin}/docs/from-matters/">plain link</a></p>`); // no .wikilink class

    a.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await wait(300);

    const popup = document.querySelector(".moss-preview-popup.visible");
    expect(popup).toBeNull();
  });

  test("a link in a body paragraph previews", async () => {
    expect(await hovers(mount(`<p>${link()}</p>`))).toBe(true);
  });

  test("a paragraph inside a blockquote still previews", async () => {
    expect(await hovers(mount(`<blockquote><p>${link()}</p></blockquote>`))).toBe(true);
  });

  test("a link in a hero caption does not preview", async () => {
    expect(
      await hovers(mount(`<div class="moss-hero"><p class="moss-hero-caption">${link()}</p></div>`)),
    ).toBe(false);
  });

  test("a link in a figcaption does not preview", async () => {
    expect(await hovers(mount(`<figure><img alt=""><figcaption>${link()}</figcaption></figure>`))).toBe(false);
  });

  test("a link in a list item does not preview", async () => {
    expect(await hovers(mount(`<ul><li>${link()}</li></ul>`))).toBe(false);
  });

  test("a paragraph inside a list item does not preview", async () => {
    expect(await hovers(mount(`<ul><li><p>${link()}</p></li></ul>`))).toBe(false);
  });

  test.each([
    ["nav", "nav"],
    ["header", "header"],
    ["footer", "footer"],
  ])("a paragraph inside %s, when inside main, does not preview", async (_name, tag) => {
    expect(await hovers(mount(`<${tag}><p>${link()}</p></${tag}>`))).toBe(false);
  });

  test("a link in a card does not preview", async () => {
    expect(await hovers(mount(`<div class="moss-card"><p>${link()}</p></div>`))).toBe(false);
  });

  test("a paragraph outside any main element does not preview", async () => {
    const p = document.createElement("p");
    p.innerHTML = link();
    document.body.appendChild(p);
    expect(await hovers(p.querySelector("a") as HTMLAnchorElement)).toBe(false);
  });

  test("a previewed prose link is described by its card", async () => {
    const a = mount(`<p>${link()}</p>`);
    await hovers(a);
    expect(a.getAttribute("aria-describedby")).toMatch(/^moss-preview-\d+$/);
  });
});
