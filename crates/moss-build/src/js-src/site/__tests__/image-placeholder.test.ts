/**
 * The inline low-quality placeholder on a content image is removed once the
 * image has loaded, and kept when the image fails.
 */
import { beforeAll, beforeEach, describe, expect, test } from "vitest";

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
  beforeAll(async () => {
    await import("../theme");
  });

  beforeEach(() => {
    document.body.innerHTML = "";
  });

  test("an image that fires load loses its placeholder", () => {
    const el = img(PLACEHOLDER);
    el.dispatchEvent(new Event("load"));
    expect(el.style.backgroundImage).toBe("");
    expect(el.style.backgroundSize).toBe("");
  });

  test("an already-complete image loses it when the page is scanned", () => {
    const el = img(PLACEHOLDER, { complete: true, naturalWidth: 800 });
    // Startup and preview morphs both run this scan.
    document.dispatchEvent(new Event("moss-morph-patched"));
    expect(el.style.backgroundImage).toBe("");
  });

  test("an image that fails to load keeps its placeholder", () => {
    const el = img(PLACEHOLDER);
    el.dispatchEvent(new Event("error"));
    expect(el.style.backgroundImage).not.toBe("");
  });

  test("a complete image with no natural size (a broken one) keeps it", () => {
    const el = img(PLACEHOLDER, { complete: true, naturalWidth: 0 });
    document.dispatchEvent(new Event("moss-morph-patched"));
    expect(el.style.backgroundImage).not.toBe("");
  });

  test("an image without a placeholder is left alone", () => {
    const el = img("width:10px", { complete: true, naturalWidth: 800 });
    el.dispatchEvent(new Event("load"));
    document.dispatchEvent(new Event("moss-morph-patched"));
    expect(el.getAttribute("style")).toBe("width:10px");
  });

  test("a non-image load event is ignored", () => {
    const div = document.createElement("div");
    div.setAttribute("style", PLACEHOLDER);
    document.body.appendChild(div);
    div.dispatchEvent(new Event("load"));
    expect(div.style.backgroundImage).not.toBe("");
  });
});
