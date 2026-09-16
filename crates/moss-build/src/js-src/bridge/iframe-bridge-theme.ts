/**
 * The theme half of the bridge: shell↔site theme messages, and the page-colour
 * reporting the shell derives `data-chrome-theme` from.
 *
 * ## Page-colour reporting
 *
 * The previewed page's effective backdrop (`--moss-color-bg`) is what the
 * shell derives `data-chrome-theme` from, so the floating glass chrome stays
 * legible over whatever is behind it (see `frontend/app/utils/chrome-theme.ts`
 * and docs/archive/2026-06-23-shell-chrome-tracks-preview-backdrop-design.md).
 * This module reads that colour inside the page and posts it up.
 *
 * WHY IT LIVES IN THE BRIDGE. It used to live in `frontend/site/link-preview.ts`,
 * which ships as `preview.js` only when `[site].link_preview` is true. That made
 * an *app chrome* mechanism a hostage of a *content* preference: with the flag
 * off, the site's own moon toggle repainted the page and told the shell nothing,
 * so the shell kept the colour it had measured under the old theme. A stale
 * colour is not inert — `applyChromeTheme()` prefers any non-null colour over
 * the shell's intended theme — so the chrome wedged on the wrong pole until the
 * next rebuild. The bridge is injected into every previewed page unconditionally
 * (`src-tauri/src/preview/iframe_bridge.rs`), which is where a mechanism the
 * chrome depends on belongs.
 *
 * The `data-theme` observer and settle re-read are `onThemeSettled`
 * (theme-settled.ts), shared with chrome-ambient and site-accent.
 */

import { onThemeSettled, type ThemeSettled } from "./theme-settled";

/** Validate a postMessage payload and return a theme, or null. Pure — unit-testable. */
export function pickThemeFromMessage(data: unknown): "light" | "dark" | null {
  if (!data || typeof data !== "object") return null;
  const d = data as { type?: unknown; theme?: unknown };
  if (d.type !== "moss-set-theme") return null;
  return d.theme === "light" || d.theme === "dark" ? d.theme : null;
}

/**
 * Read the previewed page's effective backdrop colour, or null.
 *
 * `--moss-color-bg` first: it is the site's declared page colour and is what
 * every moss-built page defines. The body's computed background is the fallback
 * for a raw HTML file in the site folder that defines no moss tokens.
 *
 * A transparent body yields null rather than `rgba(0, 0, 0, 0)`: transparent is
 * not a measurement of anything, and the shell treats null as "clear the tint"
 * rather than painting chrome from a value that says nothing about the backdrop.
 */
export function readPageColor(
  win: Window & typeof globalThis,
  getStyle: (el: Element) => CSSStyleDeclaration = (el) => win.getComputedStyle(el),
): string | null {
  const root = win.document.documentElement;
  if (root) {
    const token = getStyle(root).getPropertyValue("--moss-color-bg").trim();
    if (token) return token;
  }
  const body = win.document.body;
  if (body) {
    const bodyBg = getStyle(body).backgroundColor;
    if (bodyBg && bodyBg !== "rgba(0, 0, 0, 0)" && bodyBg !== "transparent") {
      return bodyBg;
    }
  }
  return null;
}

/**
 * Start reporting the page's backdrop to the shell: once on start, and again
 * on every `data-theme` flip.
 *
 * No dedup, deliberately — posts are rare (start, theme flips, morphs), and the
 * shell's property write is idempotent. The ambient sampler dedups because
 * scroll fires per frame; nothing here does.
 */
export function startPageColor(
  win: Window & typeof globalThis,
  post: (color: string | null) => void,
  read: () => string | null = () => readPageColor(win),
): ThemeSettled {
  const settled = onThemeSettled(win, () => post(read()));

  // The body may not exist yet when the bridge runs in <head>, and the body
  // fallback needs it. Post what we can now (the token path does not need a
  // body) and confirm once the document is parsed.
  post(read());
  if (win.document.readyState === "loading") {
    win.document.addEventListener("DOMContentLoaded", () => post(read()), { once: true });
  }

  return settled;
}
