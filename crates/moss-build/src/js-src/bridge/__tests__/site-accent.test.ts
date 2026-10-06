// @vitest-environment jsdom
/**
 * Site-accent reporting — the iframe half of the identity tint.
 *
 * jsdom resolves neither `var()` in `color` nor non-sRGB colour spaces, so
 * these tests inject the style reader and canvas (the chrome-ambient idiom)
 * and cover the mechanics: normalization, the sentinel → null mapping, the
 * post-on-start / re-post-on-theme-flip lifecycle, and detach. "The probe
 * resolves real CSS" belongs to the visual pass.
 */

import { describe, it, expect, vi, afterEach } from "vitest";
import {
  normalizeToRgb,
  readSiteAccent,
  startSiteAccent,
} from "../site-accent";

/**
 * Canvas whose fill pixel is looked up from what was assigned to fillStyle —
 * the shape the normalizer relies on: any parseable CSS colour in, sRGB out.
 */
function canvasFor(pixels: Record<string, [number, number, number, number]>) {
  return () => {
    let fill = "";
    return {
      width: 0,
      height: 0,
      getContext: () => ({
        set fillStyle(v: string) {
          fill = v;
        },
        get fillStyle() {
          return fill;
        },
        clearRect: () => {},
        fillRect: () => {},
        getImageData: () => ({ data: pixels[fill] ?? [0, 0, 0, 255] }),
      }),
    } as unknown as HTMLCanvasElement;
  };
}

describe("normalizeToRgb", () => {
  it("normalizes any colour space the computed style may serialize in", () => {
    // Modern WebKit serializes color-mix()/oklch()/display-p3 results in
    // their own spaces, not rgb() — the whole reason the canvas round-trip
    // exists (engineering review S1 of the design).
    const createCanvas = canvasFor({ "oklch(0.6 0.1 150)": [72, 154, 106, 255] });
    expect(normalizeToRgb("oklch(0.6 0.1 150)", createCanvas)).toBe("rgb(72, 154, 106)");
  });

  it("maps a fully transparent colour to null — the sentinel", () => {
    const createCanvas = canvasFor({ "rgba(0, 0, 0, 0)": [0, 0, 0, 0] });
    expect(normalizeToRgb("rgba(0, 0, 0, 0)", createCanvas)).toBeNull();
  });

  it("maps empty and missing input to null", () => {
    const createCanvas = canvasFor({});
    expect(normalizeToRgb("", createCanvas)).toBeNull();
    expect(normalizeToRgb(null, createCanvas)).toBeNull();
  });

  it("gives up quietly when no 2d context is available", () => {
    const createCanvas = () =>
      ({ getContext: () => null }) as unknown as HTMLCanvasElement;
    expect(normalizeToRgb("rgb(1, 2, 3)", createCanvas)).toBeNull();
  });
});

describe("readSiteAccent", () => {
  it("probes with the sentinel fallback and normalizes the computed colour", () => {
    // The fallback is load-bearing: without it, an undefined accent is IACVT
    // -> color inherits -> the probe reads the page's TEXT colour, and a raw
    // non-moss HTML page would poison the tint with near-black (review S2).
    let probed: HTMLElement | null = null;
    const win = {
      document,
      getComputedStyle: (el: Element) => {
        probed = el as HTMLElement;
        return { color: "rgb(45, 90, 45)" };
      },
    };
    const createCanvas = canvasFor({ "rgb(45, 90, 45)": [45, 90, 45, 255] });

    const out = readSiteAccent(
      win as unknown as Window & typeof globalThis,
      createCanvas,
    );

    expect(out).toBe("rgb(45, 90, 45)");
    expect(probed).not.toBeNull();
    expect(probed!.style.color).toBe("var(--moss-color-accent, transparent)");
    // The probe must not linger in the served document's DOM.
    expect(probed!.isConnected).toBe(false);
  });

  it("returns null when the sentinel fires (no accent on this page)", () => {
    const win = {
      document,
      getComputedStyle: () => ({ color: "rgba(0, 0, 0, 0)" }),
    };
    const createCanvas = canvasFor({ "rgba(0, 0, 0, 0)": [0, 0, 0, 0] });
    expect(
      readSiteAccent(win as unknown as Window & typeof globalThis, createCanvas),
    ).toBeNull();
  });
});

describe("startSiteAccent", () => {
  afterEach(() => {
    vi.useRealTimers();
    document.documentElement.removeAttribute("data-theme");
  });

  function harness(initial: string) {
    let color = initial;
    const posts: Array<string | null> = [];
    const win = {
      document,
      MutationObserver,
      getComputedStyle: () => ({ color }),
    };
    const createCanvas = canvasFor({
      "rgb(45, 90, 45)": [45, 90, 45, 255],
      "rgb(106, 154, 90)": [106, 154, 90, 255],
      "rgba(0, 0, 0, 0)": [0, 0, 0, 0],
    });
    return {
      posts,
      setColor: (c: string) => {
        color = c;
      },
      start: () =>
        startSiteAccent(
          win as unknown as Window & typeof globalThis,
          (c) => posts.push(c),
          createCanvas,
        ),
    };
  }

  it("posts the accent immediately on start", () => {
    const h = harness("rgb(45, 90, 45)");
    const accent = h.start();
    expect(h.posts).toEqual(["rgb(45, 90, 45)"]);
    accent.stop();
  });

  it("posts null on start when the page has no accent", () => {
    // An explicit null, not silence: the shell must clear a previous page's
    // sticky tint when a token-less page loads.
    const h = harness("rgba(0, 0, 0, 0)");
    const accent = h.start();
    expect(h.posts).toEqual([null]);
    accent.stop();
  });

  it("re-posts on a theme flip", () => {
    // The accent carries distinct light/dark values.
    vi.useFakeTimers();
    const h = harness("rgb(45, 90, 45)");
    const accent = h.start();
    h.posts.length = 0;

    h.setColor("rgb(106, 154, 90)");
    document.documentElement.setAttribute("data-theme", "dark");
    return Promise.resolve().then(() => {
      expect(h.posts).toEqual(["rgb(106, 154, 90)"]);
      accent.stop();
    });
  });
});
