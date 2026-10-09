/**
 * ArrowRight on a series page moves to the next page. An open image viewer
 * owns the arrow keys, and a key another handler has already claimed is not
 * the series handler's to act on, so neither case may navigate away.
 */

import { describe, test, expect, beforeAll, afterEach, vi } from "vitest";

function arrowRight(target: Element): boolean {
  return target.dispatchEvent(
    new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true })
  );
}

// jsdom reports a page navigation it does not implement as a console error;
// counting those is how the test sees that the series handler tried to leave.
function navigationAttempts(spy: ReturnType<typeof vi.spyOn>): number {
  return spy.mock.calls.filter((args) => String(args[0]).includes("navigation")).length;
}

describe("series arrow keys vs. the open viewer", () => {
  let lightbox: HTMLElement;
  let closeButton: HTMLElement;
  let errors: ReturnType<typeof vi.spyOn>;

  beforeAll(async () => {
    document.body.innerHTML = `
      <nav class="moss-series-nav">
        <a class="moss-series-nav-next" href="/next/">Next</a>
      </nav>
      <div id="lightbox" class="lightbox" hidden tabindex="-1">
        <button class="lightbox-close">Close</button>
      </div>`;
    lightbox = document.getElementById("lightbox")!;
    closeButton = lightbox.querySelector(".lightbox-close") as HTMLElement;
    await import("../theme");
  });

  afterEach(() => {
    errors?.mockRestore();
    lightbox.hidden = true;
  });

  test("ArrowRight navigates to the next page when nothing else claims it", () => {
    errors = vi.spyOn(console, "error").mockImplementation(() => {});
    arrowRight(document.body);
    expect(navigationAttempts(errors)).toBe(1);
  });

  test("ArrowRight is left to the open viewer and does not navigate", () => {
    errors = vi.spyOn(console, "error").mockImplementation(() => {});
    lightbox.hidden = false;
    arrowRight(closeButton);
    expect(navigationAttempts(errors)).toBe(0);
  });

  test("ArrowRight that an earlier listener already prevented does not navigate", () => {
    errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const claim = (e: Event) => e.preventDefault();
    window.addEventListener("keydown", claim, { capture: true });
    try {
      arrowRight(document.body);
    } finally {
      window.removeEventListener("keydown", claim, { capture: true });
    }
    expect(navigationAttempts(errors)).toBe(0);
  });
});
