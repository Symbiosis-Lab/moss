/**
 * theme.ts - Theme toggle, font preferences, and mobile menu functionality
 *
 * This script handles:
 * 1. Nav theme toggle (light ↔ dark, remembered in localStorage["moss-theme"])
 * 2. Aa font panel (font scale on article pages)
 * 3. Mobile menu toggle
 */

// Make this a module to allow declare global
export {};

// Expose toggle functions globally for onclick handlers
declare global {
  interface Window {
    toggleMobileMenu: () => void;
    toggleTheme: () => void;
  }
}

/**
 * The media query site.css's mobile-menu block (`@media (max-width: 20rem)`)
 * uses to collapse `.nav-links` to `max-height: 0; opacity: 0`. Below this
 * width a closed `.nav-links` is present but invisible; above it, `.nav-links`
 * is the plain always-on flex row and was never hidden by the toggle at all.
 * Both `syncMobileMenuInert` and the initial-load IIFE below key off the same
 * query so `inert` is only ever applied where the CSS is actually hiding
 * something — applying it above the breakpoint would make the always-visible
 * links unfocusable and unreadable by assistive tech even though they are
 * on screen.
 */
const MOBILE_MENU_QUERY = "(max-width: 20rem)";

/**
 * Keep `.nav-links`' `inert` in sync with whether it is actually hidden.
 *
 * `inert` (not just `aria-hidden`) removes the closed menu's links from the
 * tab order and from assistive-tech navigation — otherwise a keyboard user
 * could Tab into a link with `opacity: 0`, landing on nothing visible (WCAG
 * 2.4.7/2.4.3). Desktop nav must never be inert: `.nav-links` there is never
 * hidden, so this only sets it when the mobile-menu media query is the one
 * actually in effect.
 */
function syncMobileMenuInert(navLinks: HTMLElement, open: boolean): void {
  navLinks.inert = isMobileMenuActive() && !open;
}

/** True when `.nav-links`' visibility is governed by the mobile-menu CSS. */
function isMobileMenuActive(): boolean {
  return window.matchMedia?.(MOBILE_MENU_QUERY).matches ?? false;
}

/**
 * Toggle mobile menu visibility.
 *
 * Keeps the hamburger's `aria-expanded` and the menu's `inert` state
 * faithful to `.mobile-open`, so a screen reader knows whether the menu it
 * controls is disclosed (WCAG 4.1.2) and a keyboard user can't tab into a
 * closed, invisible menu (WCAG 2.4.7/2.4.3). See `MOBILE_MENU_QUERY` above.
 */
function toggleMobileMenu(): void {
  const navLinks = document.querySelector<HTMLElement>(".nav-links");
  const button = document.querySelector(".mobile-menu-button");
  if (navLinks) {
    const open = navLinks.classList.toggle("mobile-open");
    button?.setAttribute("aria-expanded", String(open));
    syncMobileMenuInert(navLinks, open);
  }
}

// Expose functions globally
window.toggleMobileMenu = toggleMobileMenu;

// Keep `.nav-links`' `inert` state correct as the viewport crosses
// MOBILE_MENU_QUERY — e.g. rotating a phone past 20rem, or resizing a
// desktop window down past it. Applied once on load (the menu starts closed
// under the mobile query, open/normal above it — never `mobile-open` on
// first paint) and again on every query match/unmatch after that.
function applyMobileMenuInert(): void {
  // Re-query rather than closing over the node: a preview morph can replace
  // `.nav-links` outright, and a stale reference would keep setting `inert` on
  // a detached element while the live menu stayed tabbable.
  const navLinks = document.querySelector<HTMLElement>(".nav-links");
  if (navLinks) syncMobileMenuInert(navLinks, navLinks.classList.contains("mobile-open"));
}

applyMobileMenuInert();
window.matchMedia?.(MOBILE_MENU_QUERY).addEventListener?.("change", applyMobileMenuInert);

// `inert` is a reflected IDL property, and idiomorph reconciles ATTRIBUTES —
// so a same-URL rebuild whose incoming markup has no `inert` attribute clears
// it from the live menu, silently restoring the tab stops this exists to
// remove. Nothing about the menu's open state changed, so nothing else would
// notice. Re-assert after every patch — a menu silently regaining tab stops
// it should not have is a WCAG 2.4.3/2.4.7 violation.
document.addEventListener("moss-morph-patched", applyMobileMenuInert);

/**
 * Toggle theme: light ↔ dark.
 * `data-theme` is always set — shell.html's pre-paint script resolves it from
 * localStorage["moss-theme"] or the OS before this file runs — so the toggle
 * is a flip of what is showing. The choice is stored under the same key, so
 * it persists across sessions and the next page pre-paints it. Unlike the
 * moss app's shell, a site does not clear the choice on an OS change: a
 * reader's pick sticks until they toggle again.
 *
 * Dispatches a `moss-theme-change` CustomEvent on <html> with detail:
 *   { theme: string, previous: string | null }
 * so site-level scripts can react to theme changes (e.g., exit custom modes).
 */
function toggleTheme(): void {
  const previous = document.documentElement.getAttribute("data-theme");
  const next = previous === "dark" ? "light" : "dark";

  document.documentElement.setAttribute("data-theme", next);
  localStorage.setItem("moss-theme", next);

  // Sync <meta name="theme-color"> to the new computed --moss-color-bg so
  // browser chrome (Android status bar, iOS Safari, PWA title bar) reflects
  // an author bg override set in .moss/theme/style.css after a toggle.
  // Two metas are emitted (one per prefers-color-scheme direction); after an
  // explicit toggle, override whichever media direction the OS is currently
  // reporting so the browser picks up the author value immediately.
  const bgColor = getComputedStyle(document.documentElement)
    .getPropertyValue("--moss-color-bg")
    .trim();
  if (bgColor) {
    const osDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches;
    // Target the meta whose media query matches the current OS preference —
    // that's the one the browser is actively reading.
    const targetMedia = osDark
      ? "(prefers-color-scheme: dark)"
      : "(prefers-color-scheme: light)";
    const meta = document.querySelector<HTMLMetaElement>(
      `meta[name="theme-color"][media="${targetMedia}"]`
    );
    if (meta) {
      meta.content = bgColor;
    }
  }

  // Notify site-level scripts of the theme change
  document.documentElement.dispatchEvent(
    new CustomEvent("moss-theme-change", {
      detail: { theme: next, previous },
      bubbles: true,
    })
  );
}
window.toggleTheme = toggleTheme;

// Close mobile menu when clicking outside
document.addEventListener("click", function (event: MouseEvent) {
  const navLinks = document.querySelector<HTMLElement>(".nav-links");
  const mobileButton = document.querySelector(".mobile-menu-button");

  if (
    navLinks &&
    mobileButton &&
    navLinks.classList.contains("mobile-open")
  ) {
    const target = event.target as Element;
    // Check if click is outside both the menu and the button
    if (!navLinks.contains(target) && !mobileButton.contains(target)) {
      navLinks.classList.remove("mobile-open");
      // Same faithfulness toggleMobileMenu's close path keeps — this is the
      // other way the menu closes, and both must leave aria-expanded/inert
      // correct or assistive tech is told the menu is open when it is not.
      mobileButton.setAttribute("aria-expanded", "false");
      syncMobileMenuInert(navLinks, false);
    }
  }
});

// Restore saved font scale on every page — runs before DOMContentLoaded,
// independent of whether the font control trigger exists. This ensures
// homepage, folder pages, and about pages all respect the user's choice.
(function () {
  const saved = localStorage.getItem("moss-font-scale");
  if (saved) {
    document.documentElement.classList.add(`scale-${saved}`);
  }
})();

/**
 * Morphing font control (article pages only).
 *
 * A single glyph ("字" or "Aa" depending on lang) sits next to the date,
 * rendered at the currently-selected font size. Clicking it opens a size
 * picker that grows out of that glyph.
 *
 * The trigger is the fixed anchor — it never moves and is never given a
 * runtime position. `.font-pill` is the second, differently-sized object
 * (ADR-049) that grows from the anchor's point: it carries a JS-computed
 * `--pill-offset` custom property so its *active* button always lines up
 * exactly under the trigger's fixed slot, then animates in with a
 * `transform-origin: inline-start` scale grow. The trigger and the pill
 * are never both visible at the same time — trigger hides, pill shows —
 * but because the pill's active button coincides with the trigger's slot,
 * this reads as one glyph unfolding rather than two nodes swapping.
 *
 * DOM contract (generated by render.rs):
 *
 *   .font-anchor              relative wrapper
 *     .font-trigger            absolutely positioned, static inline-start
 *     .font-pill#fontPill      always in layout (visibility:hidden when closed)
 *       button[data-scale] × 4
 *
 * `--pill-offset` is along the INLINE axis — under `writing-mode:
 * vertical-rl` (the `body[data-typesetting="vertical"]` sites), that axis
 * runs top-to-bottom instead of left-to-right, and `inset-inline-start`/
 * `transform-origin: inline-start` resolve to the same physical property
 * CSS would need branching to express. The cross axis is never positioned
 * here at all: `.font-anchor`'s `align-items: center` centers both children
 * on it, physical or not.
 *
 * Timing constants:
 *   SLIDE_MS  = 280ms  pill grow animation (CSS transition on `transform`)
 *   FADE_MS   = 150ms  pill opacity transition (CSS)
 *   PILL_PAD  = 3px    pill's CSS padding (first button offset)
 */

import { isVertical } from "./writing-mode";

const SIZE_CLASSES: Record<string, string> = {
  small: "size-small",
  "": "size-std",
  large: "size-large",
  xlarge: "size-xlarge",
};
const PILL_PAD = 3;

export function initFontPanel(): void {
  const trigger = document.querySelector(".font-trigger") as HTMLElement | null;
  const pill = document.getElementById("fontPill");
  const anchor = document.querySelector(".font-anchor");
  if (!trigger || !pill || !anchor) return;

  // Guard against double-init
  if (pill.dataset.initialized) return;
  pill.dataset.initialized = "1";

  let currentScale = "";
  let isOpen = false;

  /**
   * `el`'s inline-start edge relative to `anchor`'s, the logical equivalent
   * of `el.offsetLeft` (with `anchor` as offsetParent) that also holds under
   * vertical writing modes, where `offsetLeft` reads the wrong axis entirely.
   * `anchor`'s inline axis is `left` under horizontal-tb, `top` under
   * `vertical-rl`/`vertical-lr` — both are text-flow-relative "the axis the
   * pill's buttons are laid out along", never the anchor's physical box.
   */
  function inlineOffsetFromAnchor(el: HTMLElement): number {
    const elBox = el.getBoundingClientRect();
    const anchorBox = anchor!.getBoundingClientRect();
    return isVertical(anchor!) ? elBox.top - anchorBox.top : elBox.left - anchorBox.left;
  }

  /**
   * Recompute where the *pill* sits so its active button lands exactly
   * under the trigger's fixed slot. Runs once per selectScale() — never
   * per open/close — and writes only onto the pill; the trigger's own
   * `inset-inline-start` is a plain CSS constant that this never touches.
   */
  function updatePillOffset(): void {
    const btn = pill!.querySelector(`[data-scale="${currentScale}"]`) as HTMLElement;
    const dx = inlineOffsetFromAnchor(btn) - PILL_PAD;
    pill!.style.setProperty("--pill-offset", dx + "px");
  }

  function open(): void {
    if (isOpen) return;
    isOpen = true;
    trigger.setAttribute("aria-expanded", "true");
    trigger.classList.add("hidden");
    pill.classList.add("visible");
    (pill.querySelector(`[data-scale="${currentScale}"]`) as HTMLElement | null)?.focus();
  }

  function close(): void {
    if (!isOpen) return;
    isOpen = false;
    trigger.setAttribute("aria-expanded", "false");
    pill.classList.remove("visible");
    trigger.classList.remove("hidden");
  }

  function selectScale(scale: string): void {
    pill.querySelectorAll("button").forEach((b) => b.classList.remove("active"));
    pill.querySelector(`[data-scale="${scale}"]`)?.classList.add("active");

    Object.values(SIZE_CLASSES).forEach((c) => trigger.classList.remove(c));
    trigger.classList.add(SIZE_CLASSES[scale]);
    currentScale = scale;
    updatePillOffset();

    document.documentElement.className =
      document.documentElement.className.replace(/scale-\w+/g, "").trim() +
      (scale ? ` scale-${scale}` : "");
    localStorage.setItem("moss-font-scale", scale || "");
  }

  // Toggle on trigger click
  trigger.addEventListener("click", (e) => {
    e.stopPropagation();
    if (isOpen) close();
    else open();
  });

  // Pill button clicks
  pill.querySelectorAll("button").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      selectScale((btn as HTMLElement).dataset.scale || "");
    });
  });

  // Escape closes and returns focus to the trigger — whose position never
  // changed, so focus return is trivial.
  pill.addEventListener("keydown", (e) => {
    if ((e as KeyboardEvent).key === "Escape" && isOpen) {
      close();
      trigger.focus();
    }
  });

  // Dismiss on click outside
  document.addEventListener("click", (e) => {
    if (!anchor.contains(e.target as Node)) close();
  });

  // Dismiss on scroll
  window.addEventListener("scroll", () => { if (isOpen) close(); }, { passive: true });

  // Establish the pill's initial offset before any scale is restored.
  updatePillOffset();

  // Restore saved font scale
  const savedScale = localStorage.getItem("moss-font-scale");
  if (savedScale) {
    selectScale(savedScale);
  }
}

// Init font panel
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initFontPanel);
} else {
  initFontPanel();
}

// Re-scan after an in-place morph (frontend/bridge/iframe-bridge.ts). A
// same-URL rebuild can introduce the font-panel markup for the first time
// (e.g. the user just added `date:` frontmatter to a page that had none) —
// idiomorph creates that subtree as brand-new nodes with no listeners
// bound, since the initial init above ran before they existed and never
// runs again on its own. `initFontPanel` is idempotent per-element (guarded
// by `pill.dataset.initialized`), so re-invoking it here is safe: it only
// binds whatever wasn't already bound and no-ops otherwise.
document.addEventListener("moss-morph-patched", initFontPanel);

// ==========================================================================
// Immersive Mode - FLIP-Based Click-Triggered Fullscreen Toggle
//
// Replaces the former scroll-driven animation system with a simple
// YouTube-style fullscreen button. Click to expand iframe to fullscreen,
// click (or ESC) to exit. Uses FLIP technique for smooth transitions.
// ==========================================================================

import { initImmersiveMode } from "./immersive-mode";
initImmersiveMode();

import { initCardVideos } from "./card-video";
initCardVideos();

import { initAmbientVideos } from "./ambient-video";
initAmbientVideos();

import { initSelectionActions } from "./selection-actions";
initSelectionActions();


// Breadcrumb hover hints, shown only while a segment's label is actually cut
// off. It rides this bundle rather than shipping its own `<script>` because
// theme.js is the nav-chrome runtime and is already on every page — a second
// request for ~600 bytes would cost more than the code does. The module
// self-wires on import (DOMContentLoaded + `moss-morph-patched`).
import "./nav/breadcrumb-hint";

// The masthead breadcrumb folds like the island (breadcrumb-fold.ts is the
// shared algorithm): middle segments fold whole into a `…` levels menu
// instead of ellipsising to unreadable stubs. Self-wires on import.
import "./nav/masthead-fold";

// On a plain-site-name masthead that wraps, the toggle cluster stays on row 1
// beside the name and the links take row 2 alone, spread edge to edge — a
// measured decision flexbox cannot make (`data-nav-split`, site.css).
// Self-wires on import.
import "./nav/nav-split";

// Hint placement — clamps every `[data-tooltip]` pill into the viewport on
// hover/focus entry, so no hint can crop at a screen edge whatever its host's
// position or its text's length. Delegated listeners only; holds no node
// references, so it needs no morph re-init.
import "./nav/hint-place";

// The floating nav island (ADR-049) — reveal on scroll-up, measured folding,
// the two panels, reading progress. Rides this bundle for the same reason the
// hint above does: theme.js is the nav-chrome runtime and is already on every
// page. Self-wires on import (DOMContentLoaded + `moss-morph-patched`), and
// no-ops on a page whose markup carries no island, which is every page without
// a breadcrumb trail.
import "./nav/nav-island";

// ==========================================================================
// Vertical typesetting: remap wheel/trackpad to horizontal scroll
// In writing-mode: vertical-rl, content overflows horizontally but browsers
// don't automatically map wheel events to horizontal scrolling.
// ==========================================================================

if (document.body.dataset.typesetting === "vertical") {
  document.addEventListener("wheel", (e: WheelEvent) => {
    // Only remap when vertical delta dominates (skip natural horizontal swipes)
    if (e.ctrlKey || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
    const before = document.body.scrollLeft;
    const unit = e.deltaMode === WheelEvent.DOM_DELTA_LINE ? 16
      : e.deltaMode === WheelEvent.DOM_DELTA_PAGE ? document.body.clientWidth : 1;
    // The vertical theme makes body the horizontal scroll container. Wheel
    // down moves it left, forward in the vertical-rl reading direction.
    document.body.scrollBy({ left: -e.deltaY * unit });
    if (document.body.scrollLeft !== before) e.preventDefault();
  }, { passive: false });
}

// ==========================================================================
// Series keyboard navigation: arrow keys for prev/next in series pages
// ==========================================================================

(function () {
  const nav = document.querySelector(".moss-series-nav");
  if (!nav) return;

  document.addEventListener("keydown", (e: KeyboardEvent) => {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" ||
        (e.target as HTMLElement)?.isContentEditable) return;

    if (e.key === "ArrowLeft") {
      const prev = nav.querySelector<HTMLAnchorElement>(".moss-series-nav-prev");
      if (prev?.href) { e.preventDefault(); window.location.href = prev.href; }
    } else if (e.key === "ArrowRight") {
      const next = nav.querySelector<HTMLAnchorElement>(".moss-series-nav-next");
      if (next?.href) { e.preventDefault(); window.location.href = next.href; }
    }
  });
})();
