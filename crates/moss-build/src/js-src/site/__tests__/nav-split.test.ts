/**
 * Tests for nav-split.ts — a plain-site-name masthead that wraps keeps its
 * toggles on row 1 (`data-nav-split`); breadcrumb pages and the hamburger
 * range are left to the shipped CSS.
 *
 * jsdom does no layout, so the width arithmetic is pinned through the pure
 * `oneRowFits` decision, and the DOM tests pin the *scoping*: which mastheads
 * the module refuses to manage at all. The real-engine half — where the
 * toggles actually land — is visible in any built site at a narrow window.
 */

import { describe, test, expect, beforeEach } from "vitest";

import { initNavSplit, oneRowFits } from "../nav/nav-split";

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

describe("initNavSplit — which mastheads it manages", () => {
  test("a breadcrumb masthead is never split — the trail owns row 1", () => {
    renderMasthead(
      `<a href="/" class="site-name">My Site</a>
       <a href="/a/" class="breadcrumb-segment"><span class="breadcrumb-label">A</span></a>`,
      `<a href="/blog/">Blog</a>`,
    );
    navContent().setAttribute("data-nav-split", ""); // e.g. left over from a morph
    initNavSplit();
    expect(navContent().hasAttribute("data-nav-split")).toBe(false);
  });

  test("a masthead with no links has nothing to move", () => {
    renderMasthead(`<a href="/" class="site-name">My Site</a>`, "");
    navContent().setAttribute("data-nav-split", "");
    initNavSplit();
    expect(navContent().hasAttribute("data-nav-split")).toBe(false);
  });

  test("in jsdom every width is 0, so a plain-name masthead fits and stays unsplit", () => {
    renderMasthead(`<a href="/" class="site-name">My Site</a>`, `<a href="/blog/">Blog</a>`);
    initNavSplit();
    expect(navContent().hasAttribute("data-nav-split")).toBe(false);
  });
});
