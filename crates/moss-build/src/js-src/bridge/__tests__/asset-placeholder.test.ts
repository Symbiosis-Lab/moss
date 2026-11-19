/**
 * Tests for asset-placeholder.ts — the blueprint placeholder for any <img> that
 * fails to load, and the restore that swaps it back when the bytes land.
 *
 * These replace thumb-swap.test.ts (that module was absorbed; a missing
 * `.thumb.jpg` is just one image) and two Rust tests in the app's own
 * template tests that asserted on tokens in the
 * MINIFIED script — `swaps_src_in_place_not_replace_element` and
 * `strips_picture_source_children`. Both intents are covered here against a real
 * DOM instead of a grep: element identity survives, and source-set selection is
 * genuinely neutralized.
 *
 * The invariant every test below exists to protect: PLACE IS REVERSIBLE. The
 * previous version dropped `srcset` and deleted `<picture>`'s `<source>`
 * children outright, which left the preview's URL-keyed asset swap with nothing
 * to match — a pending image stayed a blueprint grid until a manual reload.
 */

import { describe, test, expect, beforeEach, vi } from "vitest";

// Side-effect import: the module installs its capture-phase error listener on
// `document` at load, exactly as the inlined <head> script does.
import "../asset-placeholder";
import { install, place, restore } from "../asset-placeholder";

const BLUEPRINT = "data:image/svg+xml,";

/** Build `<picture><source srcset=…><img src=…></picture>` and return the <img>. */
function picture(srcset: string, src: string): HTMLImageElement {
  const pic = document.createElement("picture");
  const source = document.createElement("source");
  source.setAttribute("srcset", srcset);
  source.setAttribute("type", "image/webp");
  const img = document.createElement("img");
  img.setAttribute("src", src);
  pic.append(source, img);
  document.body.appendChild(pic);
  return img;
}

function bare(src: string): HTMLImageElement {
  const img = document.createElement("img");
  img.setAttribute("src", src);
  document.body.appendChild(img);
  return img;
}

function video(src: string, poster?: string): HTMLVideoElement {
  const el = document.createElement("video");
  el.setAttribute("src", src);
  if (poster !== undefined) el.setAttribute("poster", poster);
  // jsdom has no media stack, so load() is not implemented. restore() calls it
  // for real engines; stub it so the assertions can see that it was called.
  (el as unknown as { load: () => void }).load = () => { loaded.push(el); };
  document.body.appendChild(el);
  return el;
}

/** Elements whose load() restore() called, in order. */
let loaded: HTMLVideoElement[] = [];

/** `error` does not bubble; the module listens in the capture phase. */
function fail(el: HTMLImageElement | HTMLVideoElement): void {
  el.dispatchEvent(new Event("error", { bubbles: false }));
}

beforeEach(() => {
  document.body.innerHTML = "";
  document.documentElement.removeAttribute("data-theme");
  loaded = [];
});

test("a picture still downloading (503) rests three ticks, then restores when its original arrives", async () => {
  const doc = document.implementation.createHTMLDocument("preview");
  const img = doc.createElement("img");
  img.src = "/assets/card.webp";
  img.getBoundingClientRect = () => ({ left: 0, top: 0, right: 100, bottom: 100 } as DOMRect);
  doc.body.appendChild(img);
  let tick: (() => void) | undefined;
  const fetch = vi.fn()
    .mockResolvedValueOnce({ ok: false, status: 503 })
    .mockResolvedValueOnce({ ok: true, status: 200 });
  const fakeWindow = {
    fetch,
    innerWidth: 1000,
    innerHeight: 1000,
    setInterval: (fn: () => void) => { tick = fn; return 1; },
    clearInterval: () => {},
    setTimeout: () => 2,
    clearTimeout: () => {},
  } as unknown as Window;
  const teardown = install(doc, fakeWindow);
  img.dispatchEvent(new Event("error"));
  expect(img.hasAttribute("data-moss-ph")).toBe(true);

  tick?.();
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(img.hasAttribute("data-moss-ph")).toBe(true);
  for (let i = 0; i < 3; i++) { tick?.(); await new Promise((resolve) => setTimeout(resolve, 0)); }
  expect(fetch).toHaveBeenCalledTimes(1);
  tick?.();
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(img.hasAttribute("data-moss-ph")).toBe(false);
  expect(img.getAttribute("src")).toContain("/assets/card.webp?_t=");
  teardown();
});

test("the retry probe skips offscreen images", async () => {
  const doc = document.implementation.createHTMLDocument("preview");
  const img = doc.createElement("img");
  img.src = "/assets/offscreen.webp";
  img.getBoundingClientRect = () => ({ left: 1200, top: 0, right: 1300, bottom: 100 } as DOMRect);
  doc.body.appendChild(img);
  let tick: (() => void) | undefined;
  const fetch = vi.fn().mockResolvedValue({ ok: false, status: 503 });
  const win = {
    fetch, innerWidth: 1000, innerHeight: 1000,
    setInterval: (fn: () => void) => { tick = fn; return 1; },
    clearInterval: () => {}, setTimeout: () => 2, clearTimeout: () => {},
  } as unknown as Window;
  const teardown = install(doc, win);
  img.dispatchEvent(new Event("error"));
  tick?.();
  expect(fetch).not.toHaveBeenCalled();
  img.getBoundingClientRect = () => ({ left: 0, top: 0, right: 100, bottom: 100 } as DOMRect);
  tick?.();
  expect(fetch).toHaveBeenCalledTimes(1);
  teardown();
});

test("teardown aborts an in-flight probe and cannot restore its image", async () => {
  const doc = document.implementation.createHTMLDocument("preview");
  const img = doc.createElement("img");
  img.src = "/assets/card.webp";
  img.getBoundingClientRect = () => ({ left: 0, top: 0, right: 100, bottom: 100 } as DOMRect);
  doc.body.appendChild(img);
  let tick: (() => void) | undefined;
  let reply: ((value: { ok: boolean; status: number }) => void) | undefined;
  let signal: AbortSignal | undefined;
  const fetch = vi.fn((_url: string, init: RequestInit) => {
    signal = init.signal as AbortSignal;
    return new Promise<{ ok: boolean; status: number }>((resolve) => { reply = resolve; });
  });
  const win = {
    fetch, innerWidth: 1000, innerHeight: 1000,
    setInterval: (fn: () => void) => { tick = fn; return 1; },
    clearInterval: () => {}, setTimeout: () => 2, clearTimeout: () => {},
  } as unknown as Window;
  const teardown = install(doc, win);
  img.dispatchEvent(new Event("error"));
  tick?.();
  expect(fetch).toHaveBeenCalledTimes(1);
  teardown();
  expect(signal?.aborted).toBe(true);
  reply?.({ ok: true, status: 200 });
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(img.hasAttribute("data-moss-ph")).toBe(true);
});

function retryHarness(srcs: string[], answer: (url: string) => Promise<{ ok: boolean; status: number }>, url = "http://preview.test/page/") {
  const doc = document.implementation.createHTMLDocument("preview");
  Object.defineProperty(doc, "URL", { value: url });
  const base = doc.createElement("base");
  base.href = url;
  doc.head.appendChild(base);
  const images = srcs.map((src) => {
    const img = doc.createElement("img");
    img.setAttribute("src", src);
    img.getBoundingClientRect = () => ({ left: 0, top: 0, right: 100, bottom: 100 } as DOMRect);
    doc.body.appendChild(img);
    return img;
  });
  const fetch = vi.fn(answer);
  let tick: (() => void) | undefined;
  let live = 0;
  const win = {
    fetch, innerWidth: 1000, innerHeight: 1000,
    setInterval: (fn: () => void) => { tick = fn; live++; return 1; },
    clearInterval: () => { live--; }, setTimeout: () => 2, clearTimeout: () => {},
  } as unknown as Window;
  const teardown = install(doc, win);
  for (const img of images) img.dispatchEvent(new Event("error"));
  const ticks = async (n: number) => { for (let i = 0; i < n; i++) { tick?.(); await new Promise((r) => setTimeout(r, 0)); } };
  return { images, fetch, ticks, teardown, live: () => live };
}

test("the retry clock runs only while a picture waits, and not while the page is hidden", async () => {
  const doc = document.implementation.createHTMLDocument("preview");
  const img = doc.createElement("img");
  img.src = "/assets/card.webp";
  img.getBoundingClientRect = () => ({ left: 0, top: 0, right: 100, bottom: 100 } as DOMRect);
  doc.body.appendChild(img);
  let tick: (() => void) | undefined;
  let live = 0;
  const win = {
    fetch: vi.fn(async () => ({ ok: true, status: 200 })), innerWidth: 1000, innerHeight: 1000,
    setInterval: (fn: () => void) => { tick = fn; live++; return 1; },
    clearInterval: () => { live--; }, setTimeout: () => 2, clearTimeout: () => {},
  } as unknown as Window;
  const teardown = install(doc, win);
  expect(live).toBe(0);
  img.dispatchEvent(new Event("error"));
  expect(live).toBe(1);
  tick?.();
  await new Promise((r) => setTimeout(r, 0));
  expect(img.hasAttribute("data-moss-ph")).toBe(false);
  tick?.();
  expect(live).toBe(0);
  img.src = "/assets/next.webp";
  img.dispatchEvent(new Event("error"));
  expect(live).toBe(1);
  Object.defineProperty(doc, "visibilityState", { value: "hidden", configurable: true });
  doc.dispatchEvent(new Event("visibilitychange"));
  expect(live).toBe(0);
  Object.defineProperty(doc, "visibilityState", { value: "visible", configurable: true });
  doc.dispatchEvent(new Event("visibilitychange"));
  expect(live).toBe(1);
  teardown();
  expect(live).toBe(0);
});

test("an empty src, which resolves to the page itself, is never probed and keeps no clock", async () => {
  const { fetch, ticks, teardown, live } = retryHarness([""], async () => ({ ok: true, status: 200 }));
  expect(live()).toBe(0);
  await ticks(3);
  expect(fetch).not.toHaveBeenCalled();
  teardown();
});

test("a picture that fails again on the same address after a restore is not asked again", async () => {
  const { images, fetch, ticks, teardown } = retryHarness(["/assets/card.webp"], async () => ({ ok: true, status: 200 }));
  await ticks(1);
  expect(images[0].hasAttribute("data-moss-ph")).toBe(false);
  images[0].dispatchEvent(new Event("error"));
  expect(images[0].hasAttribute("data-moss-ph")).toBe(true);
  fetch.mockClear();
  await ticks(3);
  expect(fetch).not.toHaveBeenCalled();
  expect(restore("/assets/card.webp", images[0].ownerDocument)).toBe(1);
  teardown();
});

test("every waiting picture on screen shows within a few ticks of the server having it, a 404 included", async () => {
  const doc = document.implementation.createHTMLDocument("preview");
  const images = Array.from({ length: 6 }, (_, i) => {
    const img = doc.createElement("img");
    img.src = `/assets/card-${i}.w800.webp`;
    img.getBoundingClientRect = () => ({ left: 0, top: 0, right: 100, bottom: 100 } as DOMRect);
    doc.body.appendChild(img);
    return img;
  });
  // The build has not promised these pictures yet: the first answer is a 404.
  let promised = false;
  const fetch = vi.fn(async () => (promised ? { ok: true, status: 200 } : { ok: false, status: 404 }));
  let tick: (() => void) | undefined;
  const win = {
    fetch, innerWidth: 1000, innerHeight: 1000,
    setInterval: (fn: () => void) => { tick = fn; return 1; },
    clearInterval: () => {}, setTimeout: () => 2, clearTimeout: () => {},
  } as unknown as Window;
  const teardown = install(doc, win);
  for (const img of images) img.dispatchEvent(new Event("error"));
  const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
  for (let i = 0; i < 3; i++) { tick?.(); await settle(); }
  expect(images.every((img) => img.hasAttribute("data-moss-ph"))).toBe(true);

  promised = true;
  for (let i = 0; i < 6; i++) { tick?.(); await settle(); }
  expect(images.filter((img) => img.hasAttribute("data-moss-ph"))).toHaveLength(0);
  teardown();
});

describe("place — paints the blueprint without destroying identity", () => {
  test("swaps the <img>'s own src and adds the marker class", () => {
    const img = bare("/assets/photo.webp");
    fail(img);

    expect(img.getAttribute("src")).toContain(BLUEPRINT);
    expect(img.classList.contains("moss-img-fallback")).toBe(true);
  });

  test("keeps the SAME element, with its classes and attributes intact", () => {
    // Regression guard carried over from Rust: an early version replaced the
    // <img> with a fabricated <span>, silently dropping every context-specific
    // CSS rule keyed on the element (.site-logo, .moss-card-cover > img, …).
    const img = bare("/assets/photo.webp");
    img.className = "site-logo";
    img.id = "logo";
    img.setAttribute("width", "120");
    const parent = img.parentElement;

    fail(img);

    expect(img.parentElement).toBe(parent);
    expect(img.tagName).toBe("IMG");
    expect(img.id).toBe("logo");
    expect(img.classList.contains("site-logo")).toBe(true);
    expect(img.getAttribute("width")).toBe("120");
  });

  test("neutralizes <source> for selection but leaves the elements in the DOM", () => {
    // A <picture>'s <source> outranks the <img>'s own src in source-set
    // selection, so the blueprint would never be chosen while a matching
    // <source srcset> is live. Removing `srcset` neutralizes it; DELETING the
    // element (what this used to do) is what made the swap unrecoverable.
    const img = picture("/assets/photo.w800.webp 800w", "/assets/photo.png");
    fail(img);

    const sources = img.parentElement!.querySelectorAll("source");
    expect(sources).toHaveLength(1);
    expect(sources[0]!.hasAttribute("srcset")).toBe(false);
    expect(sources[0]!.getAttribute("data-moss-ph-srcset")).toBe(
      "/assets/photo.w800.webp 800w",
    );
  });

  test("stashes a bare <img srcset> ladder too", () => {
    const img = bare("/assets/photo.png");
    img.setAttribute("srcset", "/assets/photo.w800.webp 800w");
    fail(img);

    expect(img.hasAttribute("srcset")).toBe(false);
    expect(img.getAttribute("data-moss-ph-srcset")).toBe(
      "/assets/photo.w800.webp 800w",
    );
  });

  test("paints no grid of its own: an empty stand-in in the picture's own shape, in either theme", () => {
    for (const theme of ["light", "dark"]) {
      document.documentElement.setAttribute("data-theme", theme);
      const img = bare("/assets/photo.webp");
      img.setAttribute("width", "1600");
      img.setAttribute("height", "900");
      fail(img);
      const svg = decodeURIComponent(img.getAttribute("src")!.slice(BLUEPRINT.length));
      expect(svg).not.toMatch(/pattern|path|rect|fill|stroke/);
      expect(svg).toContain("viewBox='0 0 1600 900'");
    }
  });

  test("is idempotent — a second error does not overwrite the stash", () => {
    const img = bare("/assets/photo.webp");
    fail(img);
    fail(img);
    expect(img.getAttribute("data-moss-ph-src")).toBe("/assets/photo.webp");
  });

  test("ignores an error from a non-image target", () => {
    const script = document.createElement("script");
    document.body.appendChild(script);
    script.dispatchEvent(new Event("error", { bubbles: false }));
    expect(script.hasAttribute("data-moss-ph")).toBe(false);
  });
});

describe("restore — swaps the real asset back in", () => {
  test("a picture whose address has a query of its own keeps it, and is restored again by the same path", () => {
    const img = bare("/assets/photo.webp?v=2");
    fail(img);

    expect(restore("/assets/photo.webp")).toBe(1);
    expect(img.getAttribute("src")).toMatch(/^\/assets\/photo\.webp\?v=2&_t=\d+$/);
    fail(img);
    expect(restore("/assets/photo.webp")).toBe(1);
    expect(img.getAttribute("src")).toMatch(/^\/assets\/photo\.webp\?v=2&_t=\d+$/);
  });

  test("restores src, drops the marker, and cache-busts the re-fetch", () => {
    const img = bare("/assets/photo.webp");
    fail(img);

    expect(restore("/assets/photo.webp")).toBe(1);
    expect(img.getAttribute("src")).toMatch(/^\/assets\/photo\.webp\?_t=\d+$/);
    expect(img.classList.contains("moss-img-fallback")).toBe(false);
    expect(img.hasAttribute("data-moss-ph")).toBe(false);
    expect(img.hasAttribute("data-moss-ph-src")).toBe(false);
  });

  test("restores a <picture> ladder so source-set selection runs again", () => {
    const img = picture("/assets/photo.w800.webp 800w", "/assets/photo.png");
    fail(img);

    // The event names the VARIANT that finished encoding, not the <img> src.
    expect(restore("/assets/photo.w800.webp")).toBe(1);

    const source = img.parentElement!.querySelector("source")!;
    expect(source.getAttribute("srcset")).toMatch(
      /^\/assets\/photo\.w800\.webp\?_t=\d+ 800w$/,
    );
    expect(source.hasAttribute("data-moss-ph-srcset")).toBe(false);
  });

  test("matches ONE candidate of a multi-candidate ladder", () => {
    // The whole point of per-candidate matching: an AssetsSettled path names a
    // single rung, and comparing it against the whole srcset attribute (the old
    // behaviour in iframe-bridge) matched nothing.
    const img = picture(
      "/assets/photo.w800.webp 800w, /assets/photo.w1600.webp 1600w",
      "/assets/photo.png",
    );
    fail(img);

    expect(restore("/assets/photo.w1600.webp")).toBe(1);

    const srcset = img.parentElement!.querySelector("source")!.getAttribute("srcset")!;
    // Both candidates come back, descriptors preserved, both busted.
    expect(srcset).toMatch(/photo\.w800\.webp\?_t=\d+ 800w/);
    expect(srcset).toMatch(/photo\.w1600\.webp\?_t=\d+ 1600w/);
  });

  test("decodes percent-encoded non-ASCII URLs before comparing", () => {
    // moss percent-encodes emitted URLs per segment; event paths arrive decoded.
    // Without decoding, no image on a CJK-named site ever matches.
    const img = bare("/%E9%97%9C%E6%96%BC/assets/%E9%A0%AD%E5%83%8F.webp");
    fail(img);
    expect(restore("/關於/assets/頭像.webp")).toBe(1);
  });

  test("leaves placeholders for other assets alone", () => {
    const a = bare("/assets/a.webp");
    const b = bare("/assets/b.webp");
    fail(a);
    fail(b);

    expect(restore("/assets/a.webp")).toBe(1);
    expect(a.hasAttribute("data-moss-ph")).toBe(false);
    expect(b.hasAttribute("data-moss-ph")).toBe(true);
    expect(b.getAttribute("src")).toContain(BLUEPRINT);
  });

  test("returns 0 when nothing is placeheld — the caller reads this as no-match", () => {
    // iframe-bridge adds this count to its own matches; a non-zero count on a
    // page with no placeholder would stop the swap buffer from retrying.
    bare("/assets/photo.webp");
    expect(restore("/assets/photo.webp")).toBe(0);
  });

  test("a restored image that fails AGAIN gets placeheld again", () => {
    // The encode landed, the swap fired, the fetch still failed. place() must
    // not be permanently disarmed by its own earlier run.
    const img = bare("/assets/photo.webp");
    fail(img);
    restore("/assets/photo.webp");
    fail(img);

    expect(img.getAttribute("src")).toContain(BLUEPRINT);
    // Stash holds the busted URL; strip it and the identity still resolves.
    expect(restore("/assets/photo.webp")).toBe(1);
  });
});

describe("window handle — how iframe-bridge reaches restore", () => {
  test("install publishes restore on window.__mossAssetPlaceholder", () => {
    const handle = (window as unknown as {
      __mossAssetPlaceholder?: { restore(p: string): number };
    }).__mossAssetPlaceholder;

    expect(typeof handle?.restore).toBe("function");

    const img = bare("/assets/photo.webp");
    place(img);
    expect(handle!.restore("/assets/photo.webp")).toBe(1);
  });
});

describe("video — the same placeholder through a different attribute", () => {
  test("a failed <video> shows the grid as its poster and drops the dead src", () => {
    // A video cannot display an SVG data-URI as media, but it does display its
    // poster for as long as no frame has decoded — which for a source that never
    // arrives is forever. Leaving `src` on would have the engine re-request a URL
    // it already failed.
    const el = video("/videos/clip.mp4");
    fail(el);

    expect(el.getAttribute("poster")).toContain(BLUEPRINT);
    expect(el.hasAttribute("src")).toBe(false);
    expect(el.classList.contains("moss-img-fallback")).toBe(true);
  });

  test("the real poster is stashed, not lost — a converting video keeps its frame", () => {
    // moss generates a thumbnail poster for videos. It can be a perfectly good
    // frame of a video whose .mp4 is still converting, so the blueprint borrows
    // the slot rather than taking it.
    const el = video("/videos/clip.mp4", "/videos/clip.thumb.jpg");
    fail(el);
    expect(el.getAttribute("poster")).toContain(BLUEPRINT);

    expect(restore("/videos/clip.mp4")).toBe(1);
    expect(el.getAttribute("poster")).toBe("/videos/clip.thumb.jpg");
  });

  test("restore puts the src back with a bust and calls load()", () => {
    // The first fetch 404'd, so the URL is negatively cached; and a <video>
    // does not start a fetch on a new `src` alone.
    const el = video("/videos/clip.mp4");
    fail(el);

    expect(restore("/videos/clip.mp4")).toBe(1);
    expect(el.getAttribute("src")).toMatch(/^\/videos\/clip\.mp4\?_t=\d+$/);
    expect(loaded).toEqual([el]);
    expect(el.hasAttribute("data-moss-ph")).toBe(false);
    expect(el.hasAttribute("data-moss-ph-poster")).toBe(false);
  });

  test("a video with no poster of its own ends with no poster attribute", () => {
    // Restoring `poster=""` would make the engine fetch the page URL as an image.
    const el = video("/videos/clip.mp4");
    fail(el);
    restore("/videos/clip.mp4");

    expect(el.hasAttribute("poster")).toBe(false);
  });

  test("another video's arrival leaves this one on its placeholder", () => {
    const a = video("/videos/a.mp4");
    const b = video("/videos/b.mp4");
    fail(a);
    fail(b);

    expect(restore("/videos/a.mp4")).toBe(1);
    expect(b.getAttribute("poster")).toContain(BLUEPRINT);
    expect(loaded).toEqual([a]);
  });

  test("an <audio> failure is left alone — there is nothing to placehold", () => {
    // The publish gate still refuses over it; a placeholder would be theatre.
    const el = document.createElement("audio");
    el.setAttribute("src", "/audio/song.m4a");
    document.body.appendChild(el);
    fail(el as unknown as HTMLVideoElement);

    expect(el.hasAttribute("data-moss-ph")).toBe(false);
    expect(el.getAttribute("src")).toBe("/audio/song.m4a");
  });
});
