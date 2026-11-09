/**
 * Ambient band — the shell half.
 *
 * The preview samples colours off its own top strip
 * (`crates/moss-build/src/js-src/bridge/chrome-ambient.ts`) and posts them up; this turns them
 * into the CSS gradient painted behind the titlebar by `.moss-ambient-band`.
 *
 * Why it exists: the shell insets the preview iframe below the chrome, so no
 * page pixels sit behind the pill and its `backdrop-filter` has only the
 * shell's flat container to blur. This puts the site's colour back into the
 * chrome without giving the served document any knowledge of the app's
 * geometry, so the shell remains the only writer of the clearance.
 *
 * Pure and DOM-free so the validation below is testable on its own. The
 * validation is the point: these strings arrive by postMessage from previewed
 * content, which is author-controlled, and land in a CSS custom property.
 */

import { deriveSiteTint } from "./site-tint";

/**
 * CSS colour syntaxes accepted from the preview.
 *
 * A whitelist, matching the one `moss-page-color` already uses. Anything
 * else — `url(...)`, `var(...)`, an unclosed function, a bare identifier — is
 * dropped rather than escaped, because there is no legitimate sampler output
 * that needs it and a permissive parser here writes attacker-influenced text
 * straight into a style property.
 */
const HEX = /^#(?:[\da-fA-F]{3,4}|[\da-fA-F]{6}|[\da-fA-F]{8})$/;
const NUM = String.raw`[+-]?(?:\d+\.?\d*|\.\d+)%?`;
const SEP = String.raw`(?:\s*,\s*|\s+)`;
const ALPHA = String.raw`(?:\s*[,/]\s*${NUM})?`;
const RGB = new RegExp(`^rgba?\\(\\s*${NUM}${SEP}${NUM}${SEP}${NUM}${ALPHA}\\s*\\)$`);
const HSL = new RegExp(
  `^hsla?\\(\\s*[+-]?(?:\\d+\\.?\\d*|\\.\\d+)(?:deg)?${SEP}${NUM}${SEP}${NUM}${ALPHA}\\s*\\)$`,
);

/**
 * Longest string the grammar will even look at. The RGB/HSL patterns
 * backtrack quadratically on a long digit run that never satisfies the
 * separator (`NUM` and `SEP` both match around a digit boundary), so a
 * hostile page posting a 200k-char "rgb(99…9)" freezes the SHELL's main
 * thread inside `.test()`. A real CSS colour is under 64 characters;
 * anything longer is hostile by construction and fails on length alone.
 */
const MAX_COLOR_LENGTH = 64;

/**
 * Bound on how many entries of a posted colour array we will process. The
 * sampler sends 7 columns; a million-entry hostile array must not buy
 * main-thread time. Kept in step with site-tint.ts's MAX_SAMPLES.
 */
const MAX_SAMPLES = 32;

/**
 * True if `value` is a colour we are willing to hand to CSS.
 *
 * The component grammar is checked, not just the character set. A looser
 * pattern is safe against injection — no `(`, `;`, `*` or letter can reach the
 * output either way — but it lets structurally invalid colours through
 * (`#ggg`, `rgb(,,)`), and one of those poisons the whole declaration:
 * `background: var(--moss-ambient-band, <fallback>)` becomes
 * invalid-at-computed-value-time, which resolves to `unset`, NOT to the
 * fallback. The band would paint transparent rather than degrade to the flat
 * page colour.
 */
export function isSafeColor(value: unknown): value is string {
  if (typeof value !== "string" || value.length > MAX_COLOR_LENGTH) return false;
  const v = value.trim();
  return HEX.test(v) || RGB.test(v) || HSL.test(v);
}

/**
 * Build the `linear-gradient(...)` for the band, or null to clear it.
 *
 * Returns null when nothing usable arrived, so the caller removes the custom
 * property and the band falls back to the flat page colour rather than
 * painting a default that flashes.
 *
 * Stops are spread evenly across the width; CSS interpolates between them.
 * That interpolation is the cheap stand-in for the blur we cannot run —
 * `backdrop-filter` is a compositor operation over a backdrop root and cannot
 * reach across the webview boundary into the iframe, at any strength.
 */
export function buildAmbientGradient(colors: unknown): string | null {
  if (!Array.isArray(colors) || colors.length === 0) return null;

  const safe = colors.slice(0, MAX_SAMPLES).filter(isSafeColor).map((c) => c.trim());
  if (safe.length === 0) return null;

  // A single sample has no direction to interpolate along; emit it as a flat
  // fill rather than a degenerate one-stop gradient.
  if (safe.length === 1) return safe[0];

  const stops = safe
    .map((c, i) => `${c} ${((i / (safe.length - 1)) * 100).toFixed(1)}%`)
    .join(", ");
  return `linear-gradient(90deg, ${stops})`;
}

/**
 * Write the band onto `root`, or clear it when nothing usable arrived.
 *
 * Clearing rather than leaving the last gradient in place matters on
 * navigation: the band falls back to `--moss-page-color` for the new document
 * instead of holding the previous page's colours through the load.
 */
export function applyAmbientBand(root: HTMLElement, colors: unknown): void {
  const gradient = buildAmbientGradient(colors);
  if (gradient) root.style.setProperty("--moss-ambient-band", gradient);
  else root.style.removeProperty("--moss-ambient-band");
}

/**
 * Write the site-identity veil onto `root`, or clear it.
 *
 * Same trust boundary as the band: `accent` arrives by postMessage from
 * previewed content. It is grammar-checked here, then parsed strictly by
 * `deriveSiteTint` (which only accepts the bridge's normalized `rgb(...)`
 * form, treats moss's own default accent as absence, and only ever emits a
 * string it built from parsed integers), so what lands in CSS is
 * shell-constructed. Anything else is treated as absence: the property is
 * removed and the chrome falls back to today's neutral rendering.
 *
 * Theme is a parameter because the derivation's lightness comes from the
 * APP theme's surface (transfer colour, never luminance — see
 * preview/site-tint.ts); the caller re-invokes on app theme flips.
 */
export function applySiteAccent(
  root: HTMLElement,
  accent: unknown,
  theme: "light" | "dark",
): void {
  const veil = isSafeColor(accent) ? deriveSiteTint(accent, theme) : null;
  if (veil) root.style.setProperty("--moss-site-veil", veil);
  else root.style.removeProperty("--moss-site-veil");
}
