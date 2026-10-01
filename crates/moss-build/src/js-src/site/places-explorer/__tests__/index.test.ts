/**
 * Stub test for the places-explorer bundle: the only behaviour this landing
 * ships is finding the handshake element and marking it pending.
 */
import { afterEach, describe, expect, test } from "vitest";
import { initPlacesExplorer } from "../index";

afterEach(() => {
  document.body.innerHTML = "";
});

describe("initPlacesExplorer", () => {
  test("marks the handshake element pending", () => {
    document.body.innerHTML =
      '<figure class="moss-place-map" data-moss-places-explorer data-world="/w.svg" data-tiles="/t.json" data-places="/p.json" data-scope="places"></figure>';

    initPlacesExplorer();

    const el = document.querySelector("[data-moss-places-explorer]");
    expect(el?.getAttribute("data-moss-places-explorer-ready")).toBe("pending");
  });

  test("does nothing when no handshake element is on the page", () => {
    document.body.innerHTML = "<p>no map here</p>";

    expect(() => initPlacesExplorer()).not.toThrow();
    expect(document.querySelector("[data-moss-places-explorer-ready]")).toBeNull();
  });
});
