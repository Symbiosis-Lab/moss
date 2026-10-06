/**
 * Tests for the mobile-menu accessibility state theme.ts maintains:
 * aria-expanded on the hamburger, and `inert` on the closed `.nav-links` so
 * its invisible links leave the tab order (WCAG 4.1.2 / 2.4.7 / 2.4.3,
 * a hidden-menu keyboard-focus regression caught on the desktop app).
 *
 * jsdom does no layout, so "is the mobile-menu CSS actually hiding the
 * links" is simulated via a `matchMedia` mock rather than measured — the real
 * cascade is covered by a render gate, not here (see CLAUDE.md's render-gates
 * section: "put an assertion here only if it needs an engine").
 */

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

function mockMatchMedia(matches: boolean): void {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    configurable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  });
}

function renderNav(): { navLinks: HTMLElement; button: HTMLElement } {
  document.body.innerHTML = `
    <button class="mobile-menu-button" onclick="toggleMobileMenu()" aria-expanded="false" aria-controls="nav-links"></button>
    <div class="nav-links" id="nav-links"><a href="/about/">About</a></div>
  `;
  return {
    navLinks: document.querySelector<HTMLElement>(".nav-links")!,
    button: document.querySelector<HTMLElement>(".mobile-menu-button")!,
  };
}

async function loadTheme(): Promise<void> {
  vi.resetModules();
  await import("../theme");
}

beforeEach(() => {
  document.body.innerHTML = "";
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("mobile menu — under the mobile breakpoint", () => {
  beforeEach(() => {
    mockMatchMedia(true); // simulates matchMedia("(max-width: 20rem)").matches
  });

  test("closed on load is inert", async () => {
    const { navLinks } = renderNav();
    await loadTheme();
    expect(navLinks.inert).toBe(true);
  });

  test("toggling open clears inert and sets aria-expanded", async () => {
    const { navLinks, button } = renderNav();
    await loadTheme();

    window.toggleMobileMenu();

    expect(navLinks.classList.contains("mobile-open")).toBe(true);
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(navLinks.inert).toBe(false);
  });

  test("toggling closed again restores inert and aria-expanded", async () => {
    const { navLinks, button } = renderNav();
    await loadTheme();

    window.toggleMobileMenu();
    window.toggleMobileMenu();

    expect(navLinks.classList.contains("mobile-open")).toBe(false);
    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(navLinks.inert).toBe(true);
  });

  test("clicking outside closes the menu and resets aria-expanded/inert", async () => {
    const { navLinks, button } = renderNav();
    document.body.appendChild(document.createElement("main")); // an outside target
    await loadTheme();

    window.toggleMobileMenu(); // open it
    expect(navLinks.inert).toBe(false);

    document.querySelector("main")!.dispatchEvent(new MouseEvent("click", { bubbles: true }));

    expect(navLinks.classList.contains("mobile-open")).toBe(false);
    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(navLinks.inert).toBe(true);
  });
});

describe("mobile menu — above the mobile breakpoint", () => {
  beforeEach(() => {
    mockMatchMedia(false); // desktop: .nav-links is the plain, always-visible row
  });

  test("never inert, even while closed", async () => {
    const { navLinks } = renderNav();
    await loadTheme();
    expect(navLinks.inert).toBe(false);
  });

  test("toggling (e.g. a stray call) still never sets inert", async () => {
    const { navLinks } = renderNav();
    await loadTheme();

    window.toggleMobileMenu();
    expect(navLinks.inert).toBe(false);

    window.toggleMobileMenu();
    expect(navLinks.inert).toBe(false);
  });
});
