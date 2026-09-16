/**
 * Site-accent reporting — the iframe half of the identity tint.
 *
 * The site's accent (`--moss-color-accent`) is its declared identity colour:
 * author-chosen, WCAG-normalised per theme. The shell tints its chrome and
 * the mobile room with a colour *derived* from it (hue and capped chroma from
 * the accent, lightness from the app surface — see
 * docs/archive/2026-08-03-site-identity-tint.md). This module reads the
 * accent inside the previewed page and posts it up.
 *
 * Shell-mounted bridge only, like chrome-ambient: the probe never ships in
 * the served bytes, so preview bytes stay identical to web bytes (ADR-039's
 * invariant — this adds a reader, never a writer).
 *
 * Why a probe element and not `getPropertyValue("--moss-color-accent")`: an
 * unregistered custom property returns its raw declared text. An author who
 * overrides the accent with `color-mix(...)` or `var(...)` would hand the
 * shell a string its whitelist rightly rejects. `color: var(...)` on a probe
 * computes to a concrete colour.
 *
 * Why the canvas round-trip: computed style is NOT "always rgb()" — modern
 * WebKit serializes `oklch()`, `color(display-p3 …)` and `color-mix` results
 * in their own colour spaces, which the shell's grammar whitelist rejects. A
 * 1×1 canvas `fillStyle` + `getImageData` forces sRGB and yields plain
 * channels for any colour the engine can parse.
 *
 * Why the `transparent` sentinel in the probe's fallback: without it an
 * undefined accent makes the declaration invalid-at-computed-value-time,
 * `color` falls back to *inherit*, and the probe reads the page's text
 * colour — a raw non-moss HTML file in the site folder would poison the
 * shell's sticky tint with near-black. Author accents are WCAG-normalised
 * and never transparent, so alpha 0 unambiguously means "no accent here",
 * which the sender reports as an explicit null so the shell clears.
 */

import { onThemeSettled, type ThemeSettled } from "./theme-settled";

export const SITE_ACCENT_PROBE = "var(--moss-color-accent, transparent)";

/**
 * Normalize any CSS colour the engine can parse into `rgb(r, g, b)`, or null
 * for no colour (empty input, alpha 0, no canvas context).
 *
 * Inputs come from `getComputedStyle`, which only emits valid colours — an
 * arbitrary invalid string would silently leave the canvas's default black
 * fillStyle in place, but that path is unreachable from the probe.
 */
export function normalizeToRgb(
  css: string | null | undefined,
  createCanvas: () => HTMLCanvasElement,
): string | null {
  if (!css || !css.trim()) return null;
  const canvas = createCanvas();
  canvas.width = 1;
  canvas.height = 1;
  const ctx = canvas.getContext("2d", { willReadFrequently: true }) as
    | CanvasRenderingContext2D
    | null;
  if (!ctx) return null;
  ctx.clearRect(0, 0, 1, 1);
  ctx.fillStyle = css;
  ctx.fillRect(0, 0, 1, 1);
  const d = ctx.getImageData(0, 0, 1, 1).data;
  if (d[3] === 0) return null;
  return `rgb(${d[0]}, ${d[1]}, ${d[2]})`;
}

/**
 * Read the page's accent via a probe element. Returns `rgb(...)` or null
 * when the page defines no accent (the sentinel fires).
 *
 * The probe is attached for the single `getComputedStyle` call — computed
 * style does not resolve `var()` on detached elements — and removed before
 * returning, so it never appears in the page the author is looking at.
 */
export function readSiteAccent(
  win: Window & typeof globalThis,
  createCanvas: () => HTMLCanvasElement,
): string | null {
  const doc = win.document;
  const probe = doc.createElement("span");
  probe.style.display = "none";
  probe.style.color = SITE_ACCENT_PROBE;
  doc.documentElement.appendChild(probe);
  let computed: string;
  try {
    computed = win.getComputedStyle(probe).color;
  } finally {
    probe.remove();
  }
  return normalizeToRgb(computed, createCanvas);
}

/**
 * Post the site's accent to the shell: once on start, and again on every
 * `data-theme` flip (the accent carries distinct light/dark values). Nulls
 * are posted too — they are how a token-less page clears a previous page's
 * sticky tint.
 *
 * No dedup, deliberately: posts are rare (start + theme flips), the shell's
 * property write is idempotent, and a same-value write does not restart CSS
 * transitions. The ambient sampler's dedup exists because scroll fires per
 * frame; nothing here does.
 */
export function startSiteAccent(
  win: Window & typeof globalThis,
  post: (color: string | null) => void,
  createCanvas: () => HTMLCanvasElement = () =>
    win.document.createElement("canvas"),
): ThemeSettled {
  const send = () => post(readSiteAccent(win, createCanvas));

  // `color` is not in site.css's background transition, but the accent
  // carries distinct light/dark values, so it re-reads on every flip too.
  const settled = onThemeSettled(win, send);
  send();
  return settled;
}
