/**
 * The inline low-quality placeholder on a content image is removed once the
 * image has loaded, and kept when the image fails.
 */
import { beforeAll, beforeEach, describe, expect, test } from "vitest";
import { clearLoadedPlaceholders, initImagePlaceholders } from "../image-placeholder";

const PLACEHOLDER = "background-image:url(data:image/jpeg;base64,AAAA);background-size:cover";

function img(style: string | null, opts: { complete?: boolean; naturalWidth?: number } = {}): HTMLImageElement {
  const el = document.createElement("img");
  if (style !== null) el.setAttribute("style", style);
  if (opts.complete !== undefined) {
    Object.defineProperty(el, "complete", { value: opts.complete });
  }
  if (opts.naturalWidth !== undefined) {
    Object.defineProperty(el, "naturalWidth", { value: opts.naturalWidth });
  }
  document.body.appendChild(el);
  return el;
}

describe("image placeholder", () => {
  beforeAll(() => {
    initImagePlaceholders();
  });

  beforeEach(() => {
    document.body.innerHTML = "";
  });

  describe("load path", () => {
    test("an image that fires load loses its placeholder", () => {
      const el = img(PLACEHOLDER, { complete: true, naturalWidth: 800 });
      el.dispatchEvent(new Event("load"));
      expect(el.style.backgroundImage).toBe("");
      expect(el.style.backgroundSize).toBe("");
    });

    test("an SVG with no intrinsic size loses it on load despite naturalWidth 0", () => {
      const el = img(PLACEHOLDER, { complete: true, naturalWidth: 0 });
      el.dispatchEvent(new Event("load"));
      expect(el.style.backgroundImage).toBe("");
    });

    test("an image that fires error keeps its placeholder", () => {
      const el = img(PLACEHOLDER);
      el.dispatchEvent(new Event("error"));
      expect(el.style.backgroundImage).not.toBe("");
    });

    test("a non-image load event is ignored", () => {
      const div = document.createElement("div");
      div.setAttribute("style", PLACEHOLDER);
      document.body.appendChild(div);
      div.dispatchEvent(new Event("load"));
      expect(div.style.backgroundImage).not.toBe("");
    });
  });

  describe("scan path", () => {
    test("the scan clears an already-complete image", () => {
      const el = img(PLACEHOLDER, { complete: true, naturalWidth: 800 });
      clearLoadedPlaceholders();
      expect(el.style.backgroundImage).toBe("");
      expect(el.style.backgroundSize).toBe("");
    });

    test("DOMContentLoaded runs the scan", () => {
      const el = img(PLACEHOLDER, { complete: true, naturalWidth: 800 });
      document.dispatchEvent(new Event("DOMContentLoaded"));
      expect(el.style.backgroundImage).toBe("");
    });

    test("a preview morph runs the scan", () => {
      const el = img(PLACEHOLDER, { complete: true, naturalWidth: 800 });
      document.dispatchEvent(new Event("moss-morph-patched"));
      expect(el.style.backgroundImage).toBe("");
    });

    test("an incomplete image is left for its load event", () => {
      const el = img(PLACEHOLDER, { complete: false, naturalWidth: 0 });
      clearLoadedPlaceholders();
      expect(el.style.backgroundImage).not.toBe("");
    });

    test("a complete image with naturalWidth 0 keeps it (broken and SVG look alike here)", () => {
      const el = img(PLACEHOLDER, { complete: true, naturalWidth: 0 });
      clearLoadedPlaceholders();
      expect(el.style.backgroundImage).not.toBe("");
    });
  });

  test("an image without a placeholder is left alone", () => {
    const el = img("width:10px", { complete: true, naturalWidth: 800 });
    el.dispatchEvent(new Event("load"));
    clearLoadedPlaceholders();
    expect(el.getAttribute("style")).toBe("width:10px");
  });
});
