/**
 * Tests for the chrome ambient sampler (`js-src/bridge/chrome-ambient.ts`).
 *
 * The behaviour that matters, and that a naive implementation gets wrong:
 *
 *  - an `<img>` must sample its PIXELS, not its CSS background. This is the
 *    whole requirement — "when an image is scrolled up it samples that
 *    colour" — and `getComputedStyle` reports transparent for every image.
 *  - a transparent element must fall through to whatever paints behind it,
 *    or a page with `body{background:#fff}` and transparent sections samples
 *    as nothing.
 *  - the sampler must be quiet when nothing changes, because scroll fires far
 *    more often than the picture changes.
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import {
  startChromeAmbient,
  parseRgb,
  toRgbString,
  averageRgb,
  colorsDiffer,
  imageAverageColor,
  resolveElementColor,
  sampleTopBand,
  DEFAULT_SAMPLE_OPTIONS,
} from "../chrome-ambient";

describe("parseRgb", () => {
  it("parses hsl() and hsla() — the ONLY form moss emits --moss-cover-color in", () => {
    // color_extract.rs::darkened_hsla formats `hsla({h}, {s}%, {l}%, 1)`, and
    // IFRAME_COVER_FALLBACK is "hsla(0, 0%, 18%, 1)". A parser without this
    // branch drops the cover colour on every page that has one.
    expect(parseRgb("hsl(0, 100%, 50%)")).toEqual([255, 0, 0]);
    expect(parseRgb("hsl(120, 100%, 50%)")).toEqual([0, 255, 0]);
    expect(parseRgb("hsla(0, 0%, 18%, 1)")).toEqual([45.9, 45.9, 45.9]);
    expect(parseRgb("hsl(240deg 100% 50%)")).toEqual([0, 0, 255]);
    expect(parseRgb("hsla(212, 14%, 29%, 0)")).toBeNull(); // alpha 0 → no colour
  });

  it("parses rgb(), rgba(), #rgb, #rrggbb", () => {
    expect(parseRgb("rgb(10, 20, 30)")).toEqual([10, 20, 30]);
    expect(parseRgb("rgba(10, 20, 30, 0.5)")).toEqual([10, 20, 30]);
    expect(parseRgb("#abc")).toEqual([170, 187, 204]);
    expect(parseRgb("#0a141e")).toEqual([10, 20, 30]);
  });

  it("treats fully transparent as no colour, so sampling falls through", () => {
    // The common case: a <section> with no background sitting on a white body.
    // Returning black here would paint a dark band over a light page.
    expect(parseRgb("rgba(0, 0, 0, 0)")).toBeNull();
    expect(parseRgb("transparent")).toBeNull();
    expect(parseRgb("#00000000")).toBeNull();
  });

  it("returns null for junk rather than throwing", () => {
    expect(parseRgb("")).toBeNull();
    expect(parseRgb(null)).toBeNull();
    expect(parseRgb("not-a-colour")).toBeNull();
    expect(parseRgb("rgb(10, 20)")).toBeNull();
  });

  it("keeps a partially transparent colour (only alpha 0 disqualifies)", () => {
    expect(parseRgb("rgba(255, 0, 0, 0.01)")).toEqual([255, 0, 0]);
  });
});

describe("averageRgb / toRgbString", () => {
  it("averages component-wise and rounds on format", () => {
    const avg = averageRgb([
      [0, 0, 0],
      [10, 20, 30],
    ]);
    expect(avg).toEqual([5, 10, 15]);
    expect(toRgbString(avg!)).toBe("rgb(5, 10, 15)");
  });

  it("clamps out-of-range components", () => {
    expect(toRgbString([-5, 300, 128])).toBe("rgb(0, 255, 128)");
  });

  it("returns null for an empty set", () => {
    expect(averageRgb([])).toBeNull();
  });
});

describe("colorsDiffer", () => {
  it("ignores sub-threshold drift so scrolling does not repaint every frame", () => {
    expect(colorsDiffer(["rgb(100, 100, 100)"], ["rgb(102, 100, 100)"])).toBe(false);
  });

  it("reports a real change", () => {
    expect(colorsDiffer(["rgb(100, 100, 100)"], ["rgb(140, 100, 100)"])).toBe(true);
  });

  it("reports a length change", () => {
    expect(colorsDiffer(["rgb(0,0,0)"], [])).toBe(true);
  });
});

/** Minimal stand-in for a canvas that reports one fixed pixel. */
function fakeCanvas(pixel: [number, number, number, number]) {
  return () =>
    ({
      width: 0,
      height: 0,
      getContext: () => ({
        drawImage: () => {},
        getImageData: () => ({ data: pixel }),
      }),
    }) as unknown as HTMLCanvasElement;
}

function fakeImg(src: string, complete = true, naturalWidth = 100) {
  const img = document.createElement("img");
  Object.defineProperty(img, "currentSrc", { value: src, configurable: true });
  Object.defineProperty(img, "complete", { value: complete, configurable: true });
  Object.defineProperty(img, "naturalWidth", { value: naturalWidth, configurable: true });
  return img;
}

describe("imageAverageColor", () => {
  it("downscales an image to one pixel and returns it", () => {
    const cache = new Map<string, string | null>();
    const color = imageAverageColor(
      fakeImg("/a.png"),
      cache,
      fakeCanvas([12, 34, 56, 255]),
    );
    expect(color).toBe("rgb(12, 34, 56)");
  });

  it("caches by resolved URL — the same cover scrolls through repeatedly", () => {
    const cache = new Map<string, string | null>();
    const create = vi.fn(fakeCanvas([1, 2, 3, 255]));
    const img = fakeImg("/a.png");
    imageAverageColor(img, cache, create);
    imageAverageColor(img, cache, create);
    imageAverageColor(img, cache, create);
    expect(create).toHaveBeenCalledTimes(1);
  });

  it("returns null for an undecoded image WITHOUT caching the miss", () => {
    // Caching here would pin a just-loading cover to null for the session.
    const cache = new Map<string, string | null>();
    expect(imageAverageColor(fakeImg("/a.png", false, 0), cache, fakeCanvas([1, 2, 3, 255])))
      .toBeNull();
    expect(cache.size).toBe(0);
  });

  it("returns null for a fully transparent pixel (padded logo)", () => {
    const cache = new Map<string, string | null>();
    expect(imageAverageColor(fakeImg("/a.png"), cache, fakeCanvas([9, 9, 9, 0]))).toBeNull();
  });

  it("gives up quietly on a tainted canvas instead of throwing each frame", () => {
    const cache = new Map<string, string | null>();
    const throwing = () =>
      ({
        getContext: () => ({
          drawImage: () => {},
          getImageData: () => {
            throw new Error("SecurityError: tainted");
          },
        }),
      }) as unknown as HTMLCanvasElement;
    expect(() =>
      imageAverageColor(fakeImg("/remote.png"), cache, throwing),
    ).not.toThrow();
    expect(imageAverageColor(fakeImg("/remote.png"), cache, throwing)).toBeNull();
  });
});

describe("resolveElementColor", () => {
  const win = (styles: Map<Element, string>) => ({
    getComputedStyle: (e: Element) => ({ backgroundColor: styles.get(e) ?? "rgba(0, 0, 0, 0)" }),
    document,
  });

  it("prefers an image's pixels over any background behind it", () => {
    // THE requirement. A naive getComputedStyle walk returns the wrapper's
    // background and the chrome never reacts to the picture.
    const wrapper = document.createElement("div");
    const img = fakeImg("/cover.png");
    wrapper.appendChild(img);
    const styles = new Map<Element, string>([[wrapper, "rgb(255, 255, 255)"]]);
    expect(
      resolveElementColor(img, win(styles), new Map(), fakeCanvas([200, 30, 40, 255])),
    ).toBe("rgb(200, 30, 40)");
  });

  it("prefers moss's build-time cover colour over canvas work", () => {
    // Already normalised for contrast by prepare_cover_color_muted; re-deriving
    // it from pixels would discard that and cost a decode.
    const section = document.createElement("section");
    section.style.setProperty("--moss-cover-color", "hsla(212, 14%, 29%, 1)");
    expect(
      resolveElementColor(section, win(new Map()), new Map(), fakeCanvas([1, 1, 1, 255])),
    ).toBe("rgb(64, 73, 84)");
  });

  it("falls through when the cover colour is unreadable, instead of eating the element", () => {
    // Returning a string this module's own parser cannot read would make the
    // caller drop the column AND skip the ancestor walk — one bad value
    // blanking the band rather than degrading to the background behind it.
    const body = document.createElement("div");
    const section = document.createElement("section");
    body.appendChild(section);
    section.style.setProperty("--moss-cover-color", "color(display-p3 1 0 0)");
    const styles = new Map<Element, string>([[body, "rgb(250, 248, 245)"]]);
    expect(resolveElementColor(section, win(styles), new Map(), fakeCanvas([1, 1, 1, 255])))
      .toBe("rgb(250, 248, 245)");
  });

  it("falls through a transparent element to the ancestor that paints", () => {
    const body = document.createElement("div");
    const section = document.createElement("section");
    body.appendChild(section);
    const styles = new Map<Element, string>([
      [section, "rgba(0, 0, 0, 0)"],
      [body, "rgb(250, 248, 245)"],
    ]);
    expect(resolveElementColor(section, win(styles), new Map(), fakeCanvas([0, 0, 0, 255])))
      .toBe("rgb(250, 248, 245)");
  });

  it("returns null when nothing in the chain paints", () => {
    const el = document.createElement("div");
    expect(resolveElementColor(el, win(new Map()), new Map(), fakeCanvas([0, 0, 0, 255])))
      .toBeNull();
  });
});

describe("sampleTopBand", () => {
  function windowWith(colorAt: (x: number, y: number) => string | null, width = 700) {
    const els = new Map<string, HTMLElement>();
    const styles = new Map<Element, string>();
    return {
      innerWidth: width,
      getComputedStyle: (e: Element) => ({
        backgroundColor: styles.get(e) ?? "rgba(0, 0, 0, 0)",
      }),
      document: {
        documentElement: document.documentElement,
        elementFromPoint: (x: number, y: number) => {
          const c = colorAt(x, y);
          if (!c) return null;
          const key = `${c}`;
          let el = els.get(key);
          if (!el) {
            el = document.createElement("div");
            els.set(key, el);
            styles.set(el, c);
          }
          return el;
        },
      } as unknown as Document,
    };
  }

  it("returns one colour per column, left to right", () => {
    const win = windowWith(() => "rgb(10, 20, 30)");
    const out = sampleTopBand(win, DEFAULT_SAMPLE_OPTIONS, new Map(), fakeCanvas([0, 0, 0, 255]));
    expect(out).toHaveLength(DEFAULT_SAMPLE_OPTIONS.columns);
    expect(new Set(out)).toEqual(new Set(["rgb(10, 20, 30)"]));
  });

  it("resolves a left/right split rather than averaging the whole band", () => {
    // A cover beside a sidebar must not collapse to one muddy colour, or the
    // gradient carries no information.
    const win = windowWith((x) => (x < 350 ? "rgb(200, 0, 0)" : "rgb(0, 0, 200)"));
    const out = sampleTopBand(win, DEFAULT_SAMPLE_OPTIONS, new Map(), fakeCanvas([0, 0, 0, 255]));
    expect(out[0]).toBe("rgb(200, 0, 0)");
    expect(out[out.length - 1]).toBe("rgb(0, 0, 200)");
  });

  it("samples inside the viewport, never on its edges", () => {
    // elementFromPoint returns null exactly at x=0 and x=innerWidth in some
    // engines; column CENTRES avoid depending on that.
    const seen: number[] = [];
    const win = windowWith((x) => {
      seen.push(x);
      return "rgb(1, 2, 3)";
    });
    sampleTopBand(win, DEFAULT_SAMPLE_OPTIONS, new Map(), fakeCanvas([0, 0, 0, 255]));
    expect(Math.min(...seen)).toBeGreaterThan(0);
    expect(Math.max(...seen)).toBeLessThan(win.innerWidth);
  });

  it("fills an unresolvable column from its neighbour, not with a hole", () => {
    const win = windowWith((x) => (x < 100 ? null : "rgb(5, 5, 5)"));
    const out = sampleTopBand(win, DEFAULT_SAMPLE_OPTIONS, new Map(), fakeCanvas([0, 0, 0, 255]));
    expect(out).toHaveLength(DEFAULT_SAMPLE_OPTIONS.columns);
    expect(out.every((c) => c === "rgb(5, 5, 5)")).toBe(true);
  });

  it("carries a real hsla cover colour all the way through to the output", () => {
    // End-to-end, in moss's actual emitted format. The per-function tests all
    // passed while this path was dead: resolveElementColor returned the raw
    // hsla string, parseRgb here could not read it, every column resolved to
    // null and sampleTopBand returned [] — on exactly the cover pages the
    // feature exists for. Assert through the caller, not the unit.
    const cover = document.createElement("div");
    cover.style.setProperty("--moss-cover-color", "hsla(212, 14%, 29%, 1)");
    const win = {
      innerWidth: 700,
      getComputedStyle: () => ({ backgroundColor: "rgba(0, 0, 0, 0)" }),
      document: {
        documentElement: document.documentElement,
        elementFromPoint: () => cover,
      } as unknown as Document,
    };
    const out = sampleTopBand(win, DEFAULT_SAMPLE_OPTIONS, new Map(), fakeCanvas([0, 0, 0, 255]));
    expect(out).toHaveLength(DEFAULT_SAMPLE_OPTIONS.columns);
    expect(out[0]).toBe("rgb(64, 73, 84)");
  });

  it("returns empty when nothing resolves, so the caller leaves chrome alone", () => {
    const win = windowWith(() => null);
    expect(sampleTopBand(win, DEFAULT_SAMPLE_OPTIONS, new Map(), fakeCanvas([0, 0, 0, 255])))
      .toEqual([]);
  });
});

describe("startChromeAmbient", () => {
  afterEach(() => vi.useRealTimers());

  /**
   * A document whose top strip is one flat colour we can change between
   * samples, wired to the real jsdom `document` so MutationObserver fires.
   */
  function harness() {
    const el = document.createElement("div");
    let color = "rgb(10, 10, 10)";
    const posts: string[][] = [];
    const frames: Array<() => void> = [];
    const win = {
      innerWidth: 700,
      MutationObserver,
      requestAnimationFrame: (cb: () => void) => {
        frames.push(cb);
        return frames.length;
      },
      addEventListener: () => {},
      removeEventListener: () => {},
      getComputedStyle: () => ({ backgroundColor: color }),
      document,
    };
    Object.defineProperty(document, "elementFromPoint", {
      value: () => el,
      configurable: true,
    });
    return {
      posts,
      setColor: (c: string) => { color = c; },
      /** Run every rAF callback queued so far. */
      flush: () => { frames.splice(0).forEach((cb) => cb()); },
      start: () =>
        startChromeAmbient(
          win as unknown as Window & typeof globalThis,
          (colors) => posts.push(colors),
        ),
    };
  }

  it("re-samples on a theme flip, which fires none of scroll/resize/load", () => {
    // The band would otherwise wear the old theme's colours until the user
    // happened to scroll — the same staleness moss-page-color already hit.
    vi.useFakeTimers();
    const h = harness();
    const ambient = h.start();
    h.flush();
    expect(h.posts).toHaveLength(1);

    h.setColor("rgb(240, 240, 240)");
    document.documentElement.setAttribute("data-theme", "dark");
    return Promise.resolve().then(() => {
      h.flush();
      expect(h.posts[h.posts.length - 1]).toContain("rgb(240, 240, 240)");
      ambient.stop();
      document.documentElement.removeAttribute("data-theme");
    });
  });

  it("re-posts after load even when the colours have not changed", () => {
    // The shell clears its ambient state on iframe `load`, but the bridge
    // samples before `load` fires (images delay `load`, not the first rAF).
    // If the load re-sample is swallowed by the dedup, the shell stays
    // cleared until the user scrolls — invisible for the band (cleared ≈
    // page colour at scroll 0) but fatal for anything that paints the
    // reported colours at full strength, e.g. the room glow.
    const el = document.createElement("div");
    const posts: string[][] = [];
    const frames: Array<() => void> = [];
    const listeners = new Map<string, () => void>();
    const win = {
      innerWidth: 700,
      MutationObserver,
      requestAnimationFrame: (cb: () => void) => {
        frames.push(cb);
        return frames.length;
      },
      addEventListener: (type: string, cb: () => void) => listeners.set(type, cb),
      removeEventListener: () => {},
      getComputedStyle: () => ({ backgroundColor: "rgb(10, 10, 10)" }),
      document,
    };
    Object.defineProperty(document, "elementFromPoint", {
      value: () => el,
      configurable: true,
    });
    const flush = () => { frames.splice(0).forEach((cb) => cb()); };

    const ambient = startChromeAmbient(
      win as unknown as Window & typeof globalThis,
      (colors) => posts.push(colors),
    );
    flush();
    expect(posts).toHaveLength(1);

    listeners.get("load")?.();
    flush();

    expect(posts).toHaveLength(2);
    expect(posts[1]).toEqual(posts[0]);
    ambient.stop();
  });
});
