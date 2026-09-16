/**
 * Tests for breadcrumb-hint.ts — the hover hint appears only when the
 * breadcrumb label is actually cut off.
 *
 * The bug this guards: nav.rs used to emit `data-tooltip` directly, and
 * site.css renders that on hover unconditionally, so a fully readable
 * breadcrumb still popped a hint repeating itself. Now nav.rs emits the inert
 * `data-hint-label` and this module promotes it only while the label
 * overflows.
 *
 * jsdom does no layout, so `scrollWidth`/`clientWidth` are both 0 for every
 * element. Each test therefore states the widths it wants directly — which is
 * the right level here: the *measurement* is the browser's job, the
 * *decision* made from it is this module's, and that decision is what these
 * tests pin. The real-engine half lives in
 * tests/render-gates/site/nav-breadcrumb-truncate.spec.js.
 */

import { describe, test, expect, beforeEach, vi } from "vitest";

import { initBreadcrumbHints } from "../nav/breadcrumb-hint";

/** Pin an element's rendered vs content width, the way a browser would. */
function setWidths(el: HTMLElement, scrollWidth: number, clientWidth: number): void {
  Object.defineProperty(el, "scrollWidth", { value: scrollWidth, configurable: true });
  Object.defineProperty(el, "clientWidth", { value: clientWidth, configurable: true });
}

function renderTrail(labels: string[]): HTMLElement[] {
  document.body.innerHTML = `
    <nav><div class="nav-left">
      <a href="/" class="site-name">My Site</a>
      ${labels
        .map(
          (title, i) =>
            `<a href="/s${i}/" class="breadcrumb-segment" data-hint-label="${title}"><span class="breadcrumb-label">${title}</span></a>`,
        )
        .join('<span class="breadcrumb-separator">/</span>')}
    </div></nav>`;
  return Array.from(document.querySelectorAll<HTMLElement>("a.breadcrumb-segment"));
}

beforeEach(() => {
  document.body.innerHTML = "";
  // jsdom has no ResizeObserver; the module's own guard would skip observing,
  // but a no-op stub keeps that path exercised too.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
});

describe("breadcrumb-hint", () => {
  test("a truncated label gets its full title as data-tooltip", () => {
    const [segment] = renderTrail(["A Very Long Section Name"]);
    setWidths(segment.querySelector<HTMLElement>(".breadcrumb-label")!, 300, 80);

    initBreadcrumbHints();

    expect(segment.getAttribute("data-tooltip")).toBe("A Very Long Section Name");
  });

  test("a fully visible label gets no data-tooltip at all", () => {
    // This is the reported bug: the hint used to show here too.
    const [segment] = renderTrail(["Notes"]);
    setWidths(segment.querySelector<HTMLElement>(".breadcrumb-label")!, 60, 60);

    initBreadcrumbHints();

    expect(segment.hasAttribute("data-tooltip")).toBe(false);
  });

  test("a sub-pixel overflow does not count as truncation", () => {
    // Fractional flex widths make scrollWidth exceed clientWidth by up to a
    // pixel on labels that are not clipped. Without slack, every segment
    // would get a hint again on some layouts.
    const [segment] = renderTrail(["Notes"]);
    setWidths(segment.querySelector<HTMLElement>(".breadcrumb-label")!, 61, 60);

    initBreadcrumbHints();

    expect(segment.hasAttribute("data-tooltip")).toBe(false);
  });

  test("widening the window drops a hint that is no longer needed", () => {
    const [segment] = renderTrail(["A Very Long Section Name"]);
    const label = segment.querySelector<HTMLElement>(".breadcrumb-label")!;
    setWidths(label, 300, 80);
    initBreadcrumbHints();
    expect(segment.hasAttribute("data-tooltip")).toBe(true);

    setWidths(label, 300, 300);
    window.dispatchEvent(new Event("resize"));

    expect(segment.hasAttribute("data-tooltip")).toBe(false);
  });

  test("each segment is judged on its own width", () => {
    const [short, long] = renderTrail(["Notes", "A Very Long Section Name"]);
    setWidths(short.querySelector<HTMLElement>(".breadcrumb-label")!, 60, 60);
    setWidths(long.querySelector<HTMLElement>(".breadcrumb-label")!, 300, 80);

    initBreadcrumbHints();

    expect(short.hasAttribute("data-tooltip")).toBe(false);
    expect(long.getAttribute("data-tooltip")).toBe("A Very Long Section Name");
  });

  test("a page with no breadcrumbs is left alone", () => {
    document.body.innerHTML = `<nav><div class="nav-left"><a href="/" class="site-name">My Site</a></div></nav>`;

    expect(() => initBreadcrumbHints()).not.toThrow();
    expect(document.querySelector("[data-tooltip]")).toBeNull();
  });

  test("morphing to a page without breadcrumbs releases the old labels", () => {
    // The observer holds the label nodes. If a morph lands on a page with no
    // trail and init returns before disconnecting, those detached nodes stay
    // reachable for the life of the tab.
    const disconnect = vi.fn();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe(): void {}
        disconnect(): void {
          disconnect();
        }
      },
    );
    renderTrail(["Notes"]);
    initBreadcrumbHints();

    document.body.innerHTML = `<nav><div class="nav-left"><a href="/" class="site-name">My Site</a></div></nav>`;
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));

    expect(disconnect).toHaveBeenCalled();
  });

  test("a nav replaced by an in-place morph is re-measured", () => {
    renderTrail(["Notes"]);
    initBreadcrumbHints();

    // Idiomorph builds brand-new nodes the first observer never saw.
    const [fresh] = renderTrail(["A Very Long Section Name"]);
    setWidths(fresh.querySelector<HTMLElement>(".breadcrumb-label")!, 300, 80);
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));

    expect(fresh.getAttribute("data-tooltip")).toBe("A Very Long Section Name");
  });
});
