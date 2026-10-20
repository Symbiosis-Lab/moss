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

function mockMatchMedia(initial: boolean): { set(matches: boolean): void; fire(): void } {
  let matches = initial;
  let onChange: (() => void) | undefined;
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    configurable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      get matches() {
        return matches;
      },
      media: query,
      addEventListener: (_: string, fn: () => void) => {
        onChange = fn;
      },
      removeEventListener: vi.fn(),
    })),
  });
  return {
    set: (next) => {
      matches = next;
    },
    fire: () => onChange?.(),
  };
}

function renderNav(): { navLinks: HTMLElement; button: HTMLElement } {
  document.body.innerHTML = `
    <header><button class="mobile-menu-button" onclick="toggleMobileMenu()" aria-label="Menu" aria-expanded="false" aria-controls="nav-links"></button></header>
    <div class="nav-links" id="nav-links"><a href="/about/">About</a><a href="/work/">Work</a></div>
  `;
  return {
    navLinks: document.querySelector<HTMLElement>(".nav-links")!,
    button: document.querySelector<HTMLElement>(".mobile-menu-button")!,
  };
}

// Each load registers document listeners that outlive the module; drop them so
// an earlier test's copy of theme.ts cannot act on this test's DOM.
type Registered = [EventTarget, string, EventListenerOrEventListenerObject, boolean | AddEventListenerOptions | undefined];
const registered: Registered[] = [];

async function loadTheme(): Promise<void> {
  vi.resetModules();
  for (const target of [document, window]) {
    const add = target.addEventListener.bind(target);
    vi.spyOn(target, "addEventListener").mockImplementation(
      (type: string, fn: EventListenerOrEventListenerObject, opts?: boolean | AddEventListenerOptions) => {
        registered.push([target, type, fn, opts]);
        add(type, fn, opts);
      },
    );
  }
  await import("../theme");
}

beforeEach(() => {
  document.body.innerHTML = "";
  document.documentElement.style.overflow = "";
  delete (window as { __mossPageScroll?: unknown }).__mossPageScroll;
});

afterEach(() => {
  vi.restoreAllMocks();
  for (const [target, type, fn, opts] of registered.splice(0)) target.removeEventListener(type, fn, opts);
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

describe("mobile menu — keyboard, focus and scroll lock", () => {
  beforeEach(() => {
    mockMatchMedia(true);
    document.documentElement.style.overflow = "";
  });

  test("opening focuses the first link, locks page scroll and keeps the button name", async () => {
    const { button } = renderNav();
    await loadTheme();

    window.toggleMobileMenu();

    expect(document.activeElement).toBe(document.querySelector(".nav-links a"));
    expect(document.documentElement.style.overflow).toBe("hidden");
    expect(button.getAttribute("aria-label")).toBe("Menu");
  });

  test("Escape closes the menu and puts focus back on the button", async () => {
    const { navLinks, button } = renderNav();
    await loadTheme();
    window.toggleMobileMenu();

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));

    expect(navLinks.classList.contains("mobile-open")).toBe(false);
    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(button.getAttribute("aria-label")).toBe("Menu");
    expect(document.activeElement).toBe(button);
    expect(document.documentElement.style.overflow).toBe("");
  });

  test("Escape with the menu closed leaves focus alone", async () => {
    const { button } = renderNav();
    await loadTheme();
    const link = document.querySelector<HTMLElement>(".nav-links a")!;
    link.focus();

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));

    expect(document.activeElement).toBe(link);
    expect(document.activeElement).not.toBe(button);
  });
});

describe("mobile menu — sharing the page scroll lock", () => {
  test("closing the menu leaves another holder's lock in place", async () => {
    mockMatchMedia(true);
    renderNav();
    await loadTheme();
    const { holdPageScroll } = await import("../viewport");
    holdPageScroll("search", true);

    window.toggleMobileMenu();
    window.toggleMobileMenu();
    expect(document.documentElement.style.overflow).toBe("hidden");

    holdPageScroll("search", false);
    expect(document.documentElement.style.overflow).toBe("");
  });

  test("a morph that drops the open class releases the menu's lock", async () => {
    mockMatchMedia(true);
    const { navLinks } = renderNav();
    await loadTheme();
    window.toggleMobileMenu();
    navLinks.classList.remove("mobile-open");

    document.dispatchEvent(new Event("moss-morph-patched"));

    expect(document.documentElement.style.overflow).toBe("");
  });
});

describe("mobile menu — crossing the breakpoint while open", () => {
  test("widening past the breakpoint closes the sheet and releases the scroll lock", async () => {
    const media = mockMatchMedia(true);
    const { navLinks, button } = renderNav();
    await loadTheme();
    window.toggleMobileMenu();
    expect(document.documentElement.style.overflow).toBe("hidden");

    media.set(false);
    media.fire();

    expect(navLinks.classList.contains("mobile-open")).toBe(false);
    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(document.documentElement.style.overflow).toBe("");
    expect(navLinks.inert).toBe(false);
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
