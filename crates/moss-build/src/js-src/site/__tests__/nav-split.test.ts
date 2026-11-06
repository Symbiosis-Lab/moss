/**
 * Tests for nav-split.ts — a masthead that wraps puts the name or trail and the
 * toggles on row 1 and the links on row 2 (`data-nav-split`).
 *
 * jsdom does no layout, so the width arithmetic is pinned through the pure
 * `oneRowFits` decision, and the DOM tests pin the *scoping*: which mastheads
 * the module refuses to manage at all. The real-engine half — where the
 * toggles actually land — is visible in any built site at a narrow window.
 */

import { describe, test, expect, beforeEach } from "vitest";

import { initNavSplit, lineCount, oneRowFits, trailSize } from "../nav/nav-split";

function renderMasthead(left: string, links: string): void {
  document.body.innerHTML = `
    <nav><div class="nav-content">
      <div class="nav-left">${left}</div>
      <div class="nav-right">
        <button class="mobile-menu-button" style="display: none"></button>
        <div class="nav-links">${links}</div>
        <div class="nav-icons"><button class="nav-theme-btn"></button></div>
      </div>
    </div></nav>`;
}

const navContent = () => document.querySelector<HTMLElement>(".nav-content")!;

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("oneRowFits — the split decision", () => {
  const row = { name: 180, links: 300, icons: 108, outerGap: 24, innerGap: 32 };

  test("splits exactly when name + gaps + links + icons exceed the row", () => {
    // 180+24+300+32+108 = 644.
    expect(oneRowFits({ ...row, container: 644 })).toBe(true);
    expect(oneRowFits({ ...row, container: 643 })).toBe(true); // 1px subpixel slack
    expect(oneRowFits({ ...row, container: 642 })).toBe(false);
  });

  test("a wider toggle cluster tips the same row over", () => {
    expect(oneRowFits({ ...row, container: 644, icons: 150 })).toBe(false);
    expect(oneRowFits({ ...row, container: 644, icons: 0 })).toBe(true);
  });
});

describe("trailSize — a trail measured on a copy", () => {
  function renderTrail(): HTMLElement {
    renderMasthead(
      `<a href="/" class="site-name" data-trail-crumb>My Site</a>
       <span class="breadcrumb-separator">/</span>
       <button class="moss-breadcrumb-more" hidden>…</button>
       <span class="breadcrumb-separator" data-trail-more-separator hidden>/</span>
       <a href="/a/" class="breadcrumb-segment" data-trail-crumb hidden>A</a>
       <span class="breadcrumb-separator" hidden>/</span>
       <a href="/a/b/" class="breadcrumb-segment" data-trail-crumb>B</a>`,
      `<a href="/blog/">Blog</a>`,
    );
    return document.querySelector<HTMLElement>(".nav-left")!;
  }
  const census = (el: HTMLElement) =>
    `${[...el.querySelectorAll("[data-trail-crumb]:not([hidden])")].map((c) => c.textContent).join(",")}; ` +
    `${el.querySelectorAll(".moss-breadcrumb-more:not([hidden])").length} more`;

  test("natural: every crumb, folded ones included, without the …", () => {
    const navLeft = renderTrail();
    const before = navLeft.outerHTML;
    let seen = "";
    trailSize(navLeft, (el) => ((seen = census(el)), 0), "natural");
    expect(seen).toBe("My Site,A,B; 0 more");
    expect(navLeft.outerHTML).toBe(before);
    expect(document.querySelectorAll(".nav-left")).toHaveLength(1);
  });

  test("folded: the site name, the … and the last crumb only", () => {
    const navLeft = renderTrail();
    let seen = "";
    trailSize(navLeft, (el) => ((seen = census(el)), 0), "folded");
    expect(seen).toBe("My Site,B; 1 more");
    expect(document.querySelectorAll(".nav-left")).toHaveLength(1);
  });
});

describe("lineCount — the name's lines beside the controls", () => {
  test("rects whose block extents overlap are one line", () => {
    // A logo taller than its text, on the same line.
    expect(lineCount([[2, 22], [0, 24]])).toBe(1);
  });

  test("stacked rects are separate lines", () => {
    expect(lineCount([[0, 20], [20, 40]])).toBe(2);
    expect(lineCount([[40, 60], [0, 20], [20, 40]])).toBe(3);
  });

  test("no text at all counts as no lines", () => {
    expect(lineCount([])).toBe(0);
  });
});

describe("initNavSplit — which mastheads it manages", () => {
  test("a masthead with no links has nothing to move", () => {
    renderMasthead(`<a href="/" class="site-name">My Site</a>`, "");
    navContent().setAttribute("data-nav-split", "");
    initNavSplit();
    expect(navContent().hasAttribute("data-nav-split")).toBe(false);
  });

  test("a vertical masthead is measured down its column, not across it", () => {
    renderMasthead(`<a href="/" class="site-name">My Site</a>`, `<a href="/blog/">Blog</a>`);
    navContent().style.writingMode = "vertical-rl";
    // Room for everything down the column, none across it.
    const sizes = (el: Element, width: number, height: number) => {
      Object.defineProperty(el, "offsetWidth", { value: width });
      Object.defineProperty(el, "offsetHeight", { value: height });
    };
    Object.defineProperty(navContent(), "clientWidth", { value: 40 });
    Object.defineProperty(navContent(), "clientHeight", { value: 600 });
    sizes(document.querySelector(".site-name")!, 30, 150);
    sizes(document.querySelector(".nav-links a")!, 30, 80);
    sizes(document.querySelector(".nav-icons")!, 36, 36);
    initNavSplit();
    expect(navContent().hasAttribute("data-nav-split")).toBe(false);
  });

  test("in jsdom every width is 0, so a plain-name masthead fits and stays unsplit", () => {
    renderMasthead(`<a href="/" class="site-name">My Site</a>`, `<a href="/blog/">Blog</a>`);
    initNavSplit();
    expect(navContent().hasAttribute("data-nav-split")).toBe(false);
  });
});
