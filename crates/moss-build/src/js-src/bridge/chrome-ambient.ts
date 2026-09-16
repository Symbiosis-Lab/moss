/**
 * Chrome ambient sampling — the iframe half.
 *
 * ADR-039 inset the preview iframe below the floating titlebar, which fixed a
 * nine-time-recurring geometry bug but cost the glass its content: the pill's
 * `backdrop-filter` now blurs the shell's flat container background, because
 * there are no page pixels above the iframe's top edge any more.
 *
 * This puts the site's colour back into the chrome without putting the app's
 * geometry back into the site. We sample a horizontal strip of the page near
 * the top of the viewport and post the colours up to the shell, which paints
 * them behind the titlebar. Nothing here changes layout, and nothing the
 * server emits differs between a preview and a real web visit — this runs
 * only in the shell-mounted bridge.
 *
 * Why the DOM and not the raster: `backdrop-filter` is a compositor operation
 * over a backdrop root and cannot cross a webview boundary, so the shell
 * cannot blur iframe pixels no matter how it is styled. Reading colours out
 * and repainting them is the only direction that works without rendering the
 * page twice.
 *
 * Why not `getComputedStyle` alone: it reports an `<img>`'s CSS background,
 * never its content, so a page whose top is a photo would sample as
 * transparent. Images are resolved by a build-time dominant colour when moss
 * computed one (`--moss-cover-color`, see build/components/color_extract.rs),
 * and by a 1×1 canvas downscale otherwise. Text is deliberately not sampled:
 * WebKit's own `predominantColor()` excludes it, and a strip of body copy
 * averages to a muddy grey that tracks nothing.
 */

import { onThemeSettled, type ThemeSettled } from "./theme-settled";

/** One sampled column, left to right across the band. */
export type AmbientColors = string[];

export interface SampleOptions {
  /** Number of columns sampled across the width. */
  columns: number;
  /** Height of the strip sampled, in CSS px from the viewport top. */
  bandHeight: number;
  /** Vertical probes per column, spread through the band. */
  rows: number;
}

export const DEFAULT_SAMPLE_OPTIONS: SampleOptions = {
  // 7 columns is enough to carry a left/right split (a cover image beside a
  // sidebar) without the gradient reading as banded. Cost is ~columns×rows
  // hit-tests per frame; measured at 128 hit-tests ≈ 1ms, so 21 is free.
  columns: 7,
  bandHeight: 48,
  rows: 3,
};

/** hue in degrees, saturation and lightness in 0..1 → RGB 0..255. */
function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const hue = (((h % 360) + 360) % 360) / 360;
  const sat = Math.max(0, Math.min(1, s));
  const lig = Math.max(0, Math.min(1, l));
  if (sat === 0) return [lig * 255, lig * 255, lig * 255];
  const q = lig < 0.5 ? lig * (1 + sat) : lig + sat - lig * sat;
  const p = 2 * lig - q;
  const channel = (t: number) => {
    let x = t;
    if (x < 0) x += 1;
    if (x > 1) x -= 1;
    if (x < 1 / 6) return p + (q - p) * 6 * x;
    if (x < 1 / 2) return q;
    if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
    return p;
  };
  return [
    channel(hue + 1 / 3) * 255,
    channel(hue) * 255,
    channel(hue - 1 / 3) * 255,
  ];
}

/**
 * Parse a CSS colour string into RGB, or null when it carries no colour.
 *
 * Returns null for `transparent` and any `rgba(…, 0)` so a transparent
 * element falls through to its ancestor rather than painting a hole.
 */
export function parseRgb(css: string | null | undefined): [number, number, number] | null {
  if (!css) return null;
  const s = css.trim().toLowerCase();
  if (s === "transparent" || s === "none") return null;

  const fn = s.match(/^rgba?\(([^)]+)\)$/);
  if (fn) {
    const parts = fn[1].split(/[\s,/]+/).filter(Boolean);
    if (parts.length < 3) return null;
    const [r, g, b] = parts.slice(0, 3).map((p) =>
      p.endsWith("%") ? (parseFloat(p) * 255) / 100 : parseFloat(p),
    );
    if ([r, g, b].some((n) => !Number.isFinite(n))) return null;
    if (parts.length >= 4) {
      const a = parts[3].endsWith("%") ? parseFloat(parts[3]) / 100 : parseFloat(parts[3]);
      if (Number.isFinite(a) && a === 0) return null;
    }
    return [r, g, b];
  }

  // hsl()/hsla() is not optional: moss emits --moss-cover-color ONLY in this
  // form (color_extract.rs::darkened_hsla), so a parser without this branch
  // drops the build-time cover colour on every page that has one.
  const hsl = s.match(/^hsla?\(([^)]+)\)$/);
  if (hsl) {
    const parts = hsl[1].split(/[\s,/]+/).filter(Boolean);
    if (parts.length < 3) return null;
    const h = parseFloat(parts[0].replace(/deg$/, ""));
    const sat = parseFloat(parts[1]) / 100;
    const l = parseFloat(parts[2]) / 100;
    if (![h, sat, l].every(Number.isFinite)) return null;
    if (parts.length >= 4) {
      const a = parts[3].endsWith("%") ? parseFloat(parts[3]) / 100 : parseFloat(parts[3]);
      if (Number.isFinite(a) && a === 0) return null;
    }
    return hslToRgb(h, sat, l);
  }

  const hex = s.match(/^#([\da-f]{3,8})$/);
  if (hex) {
    let h = hex[1];
    if (h.length === 3 || h.length === 4) h = h.split("").map((c) => c + c).join("");
    if (h.length !== 6 && h.length !== 8) return null;
    if (h.length === 8 && parseInt(h.slice(6, 8), 16) === 0) return null;
    return [
      parseInt(h.slice(0, 2), 16),
      parseInt(h.slice(2, 4), 16),
      parseInt(h.slice(4, 6), 16),
    ];
  }

  return null;
}

/** Format an RGB triple back into a CSS `rgb()` string. */
export function toRgbString([r, g, b]: [number, number, number]): string {
  const c = (n: number) => Math.max(0, Math.min(255, Math.round(n)));
  return `rgb(${c(r)}, ${c(g)}, ${c(b)})`;
}

/** Component-wise mean of the given colours; null when there are none. */
export function averageRgb(
  colors: Array<[number, number, number]>,
): [number, number, number] | null {
  if (colors.length === 0) return null;
  const sum = colors.reduce<[number, number, number]>(
    (acc, c) => [acc[0] + c[0], acc[1] + c[1], acc[2] + c[2]],
    [0, 0, 0],
  );
  return [sum[0] / colors.length, sum[1] / colors.length, sum[2] / colors.length];
}

/**
 * True when two colour lists differ enough to be worth repainting.
 *
 * Scroll fires far more often than the picture changes; without this the
 * shell would take a postMessage and a style write on every frame of every
 * scroll. Threshold is per-channel, in 0-255 units.
 */
export function colorsDiffer(a: AmbientColors, b: AmbientColors, threshold = 4): boolean {
  if (a.length !== b.length) return true;
  for (let i = 0; i < a.length; i++) {
    const pa = parseRgb(a[i]);
    const pb = parseRgb(b[i]);
    if (!pa || !pb) {
      if (a[i] !== b[i]) return true;
      continue;
    }
    for (let c = 0; c < 3; c++) {
      if (Math.abs(pa[c] - pb[c]) > threshold) return true;
    }
  }
  return false;
}

/**
 * Average colour of an image, via a 1×1 downscale.
 *
 * The browser's own box filter does the averaging, so this is one drawImage
 * regardless of source size. Results are cached by resolved URL because the
 * same cover scrolls through the band many times.
 *
 * Returns null if the image has not decoded yet, or if the canvas is tainted
 * — moss serves previewed assets from its own origin, so tainting means an
 * author hotlinked a remote image, which is exactly when we want to give up
 * quietly rather than throw on every scroll frame.
 */
export function imageAverageColor(
  img: HTMLImageElement,
  cache: Map<string, string | null>,
  createCanvas: () => HTMLCanvasElement,
): string | null {
  const key = img.currentSrc || img.src;
  if (!key) return null;
  if (cache.has(key)) return cache.get(key) ?? null;
  if (!img.complete || img.naturalWidth === 0) return null;

  let result: string | null = null;
  try {
    const canvas = createCanvas();
    canvas.width = 1;
    canvas.height = 1;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (ctx) {
      ctx.drawImage(img, 0, 0, 1, 1);
      const d = ctx.getImageData(0, 0, 1, 1).data;
      // Fully transparent pixels carry no colour (e.g. a padded PNG logo).
      if (d[3] !== 0) result = toRgbString([d[0], d[1], d[2]]);
    }
  } catch {
    result = null; // tainted canvas — remote image
  }
  cache.set(key, result);
  return result;
}

/**
 * Resolve a colour for one element, walking ancestors until something paints.
 *
 * Order matters: an image's own pixels beat any background painted behind it,
 * and a build-time cover colour beats canvas work because moss already
 * normalised it for contrast (`prepare_cover_color_muted`).
 */
export function resolveElementColor(
  el: Element | null,
  win: {
    getComputedStyle: (e: Element) => { backgroundColor: string };
    document: Document;
  },
  cache: Map<string, string | null>,
  createCanvas: () => HTMLCanvasElement,
): string | null {
  let node: Element | null = el;
  while (node && node !== win.document.documentElement) {
    if (node instanceof HTMLImageElement || node.tagName === "IMG") {
      const fromImage = imageAverageColor(
        node as HTMLImageElement,
        cache,
        createCanvas,
      );
      if (fromImage) return fromImage;
    }

    // Build-time dominant colour, already WCAG-normalised by moss. Normalised
    // through parseRgb rather than returned raw: returning a string this
    // module's own parser cannot read makes the caller drop the column AND
    // skips the ancestor walk below, so one unreadable value blanks the band
    // instead of degrading to the background behind it.
    const cover = (node as HTMLElement).style?.getPropertyValue?.("--moss-cover-color");
    const coverRgb = parseRgb(cover);
    if (coverRgb) return toRgbString(coverRgb);

    const bg = parseRgb(win.getComputedStyle(node).backgroundColor);
    if (bg) return toRgbString(bg);

    node = node.parentElement;
  }
  return null;
}

/**
 * Sample the top band of the viewport, returning one colour per column.
 *
 * Columns with nothing resolvable inherit the nearest resolved neighbour, so
 * a gap never paints as a hard transparent notch. If nothing resolves at all
 * the result is empty and the caller should leave the chrome as it was.
 */
export function sampleTopBand(
  win: {
    innerWidth: number;
    getComputedStyle: (e: Element) => { backgroundColor: string };
    document: Document;
  },
  opts: SampleOptions,
  cache: Map<string, string | null>,
  createCanvas: () => HTMLCanvasElement,
): AmbientColors {
  const { columns, rows, bandHeight } = opts;
  const out: Array<string | null> = [];

  for (let c = 0; c < columns; c++) {
    // Sample at column centres, so the first and last land inside the
    // viewport rather than exactly on its edges (elementFromPoint returns
    // null outside).
    const x = ((c + 0.5) / columns) * win.innerWidth;
    const found: Array<[number, number, number]> = [];
    for (let r = 0; r < rows; r++) {
      const y = ((r + 0.5) / rows) * bandHeight;
      const el = win.document.elementFromPoint(x, y);
      const color = resolveElementColor(el, win, cache, createCanvas);
      const parsed = parseRgb(color);
      if (parsed) found.push(parsed);
    }
    const avg = averageRgb(found);
    out.push(avg ? toRgbString(avg) : null);
  }

  if (out.every((c) => c === null)) return [];

  // Fill gaps from the nearest resolved neighbour, left then right.
  for (let i = 0; i < out.length; i++) {
    if (out[i]) continue;
    let j = i - 1;
    while (j >= 0 && !out[j]) j--;
    if (j >= 0) {
      out[i] = out[j];
      continue;
    }
    let k = i + 1;
    while (k < out.length && !out[k]) k++;
    if (k < out.length) out[i] = out[k];
  }

  return out.filter((c): c is string => c !== null);
}

/**
 * Start sampling and reporting the ambient band.
 *
 * Runs on scroll and on resize, coalesced to one sample per animation frame,
 * and posts only when the picture actually changed. A scroll-linked effect
 * crossing a process boundary has a floor of 1–2 frames of lag before the IPC
 * hop (Chromium's *Anatomy of Jank*; WebKit bug 206228), so the shell eases
 * toward the reported colours rather than snapping — see the CSS transition on
 * `.moss-ambient-band`. WebKit's own sampler sidesteps this by freezing on
 * scroll; we don't, because reacting to a cover scrolling past is the point.
 */
export function startChromeAmbient(
  win: Window & typeof globalThis,
  post: (colors: AmbientColors) => void,
  opts: SampleOptions = DEFAULT_SAMPLE_OPTIONS,
): ThemeSettled {
  const cache = new Map<string, string | null>();
  const createCanvas = () => win.document.createElement("canvas");
  let last: AmbientColors = [];
  let queued = false;

  const sampleNow = () => {
    queued = false;
    const colors = sampleTopBand(
      win as unknown as Parameters<typeof sampleTopBand>[0],
      opts,
      cache,
      createCanvas,
    );
    // An empty sample means "nothing resolved" — keep whatever the chrome has
    // rather than flashing it to a default.
    if (colors.length === 0) return;
    if (!colorsDiffer(last, colors)) return;
    last = colors;
    post(colors);
  };

  const schedule = () => {
    if (queued) return;
    queued = true;
    win.requestAnimationFrame(sampleNow);
  };

  // Theme flips change every colour on the page and fire none of scroll,
  // resize or load, so the band would hold the old theme's colours until the
  // user happened to scroll. onThemeSettled watches `data-theme` for that.
  const settled = onThemeSettled(win, schedule);

  // Late-decoding covers change the answer after first paint — and the shell
  // clears its ambient state on iframe `load`, while our first samples run
  // before `load` (images delay `load`, not the first rAF). Re-arm the dedup
  // so the post-load sample re-posts even when the picture is unchanged;
  // without this the cleared shell never hears from a page that was already
  // settled before `load`, and anything painting the reported colours at full
  // strength (the room glow) stays dark until the user scrolls.
  const onLoad = () => {
    last = [];
    schedule();
  };
  win.addEventListener("scroll", schedule, { passive: true });
  win.addEventListener("resize", schedule, { passive: true });
  win.addEventListener("load", onLoad);

  schedule();

  return {
    fire: settled.fire,
    stop: () => {
      win.removeEventListener("scroll", schedule);
      win.removeEventListener("resize", schedule);
      win.removeEventListener("load", onLoad);
      settled.stop();
    },
  };
}
