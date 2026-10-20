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

import { holdPageScroll } from "./viewport";

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
 * The one place the menu opens or closes, so every path (button, Escape,
 * outside click, leaving the mobile width) leaves the same state: `.mobile-open`,
 * `aria-expanded` (WCAG 4.1.2), `inert` on the closed links (2.4.3/2.4.7), the
 * page's scroll lock and, on open, focus in the menu.
 * The sheet starts where the header ends, which only layout knows.
 */
function setMobileMenuOpen(open: boolean, returnFocus = false): void {
  const navLinks = document.querySelector<HTMLElement>(".nav-links");
  const button = document.querySelector<HTMLElement>(".mobile-menu-button");
  if (!navLinks) return;

  navLinks.classList.toggle("mobile-open", open);
  button?.setAttribute("aria-expanded", String(open));
  syncMobileMenuInert(navLinks, open);

  if (open) {
    holdPageScroll("menu", true);
    placeMobileMenu(navLinks);
    navLinks.querySelector<HTMLElement>("a")?.focus({ preventScroll: true });
  } else {
    holdPageScroll("menu", false);
    if (returnFocus) button?.focus();
  }
}

/** Top edge of the sheet: the header's bottom edge, never above the viewport. */
function placeMobileMenu(navLinks: HTMLElement): void {
  const header = navLinks.closest("header");
  if (header) navLinks.style.top = `${Math.max(0, header.getBoundingClientRect().bottom)}px`;
}

function toggleMobileMenu(): void {
  const navLinks = document.querySelector<HTMLElement>(".nav-links");
  if (navLinks) setMobileMenuOpen(!navLinks.classList.contains("mobile-open"));
}

// Expose functions globally
window.toggleMobileMenu = toggleMobileMenu;

// Re-queries `.nav-links` rather than closing over it: a preview morph can
// replace the node, and a stale reference would leave the live menu tabbable.
function syncMobileMenuToViewport(): void {
  const navLinks = document.querySelector<HTMLElement>(".nav-links");
  // The lock is held only while a live `.nav-links` is open.
  const open = navLinks?.classList.contains("mobile-open") ?? false;
  if (!open) holdPageScroll("menu", false);
  if (!navLinks) return;
  // An open sheet (and its scroll lock) must not outlive the width that made it one.
  if (open && !isMobileMenuActive()) setMobileMenuOpen(false);
  else syncMobileMenuInert(navLinks, open);
}

syncMobileMenuToViewport();
window.matchMedia?.(MOBILE_MENU_QUERY).addEventListener?.("change", syncMobileMenuToViewport);

// idiomorph reconciles attributes, so a rebuild can drop `inert` (or the
// open class) from the live menu with nothing else noticing; re-assert after
// every patch.
document.addEventListener("moss-morph-patched", syncMobileMenuToViewport);

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
  const button = document.querySelector(".mobile-menu-button");
  const target = event.target as Element;
  if (navLinks?.classList.contains("mobile-open") && !navLinks.contains(target) && !button?.contains(target)) {
    setMobileMenuOpen(false);
  }
});

document.addEventListener("keydown", (event: KeyboardEvent) => {
  if (event.key !== "Escape" || event.defaultPrevented) return;
  if (document.querySelector(".nav-links.mobile-open")) setMobileMenuOpen(false, true);
});

window.addEventListener("resize", () => {
  const navLinks = document.querySelector<HTMLElement>(".nav-links.mobile-open");
  if (navLinks) placeMobileMenu(navLinks);
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
 * A single glyph ("字" or "Aa" depending on lang) sits next to the date.
 * Clicking it opens a size picker that grows out of that glyph. The
 * geometry this relies on — the fixed slot, the hold-while-open, the
 * release-on-close — is site.css's to own: see its "Morphing font control"
 * comment above `.font-anchor`.
 *
 * DOM contract (generated by render.rs):
 *
 *   .font-anchor              relative wrapper
 *     .font-trigger            absolutely positioned, static inline-start
 *     .font-pill#fontPill      always in layout (visibility:hidden when closed)
 *       button[data-scale] × 4
 */

const SIZE_CLASSES: Record<string, string> = {
  small: "size-small",
  "": "size-std",
  large: "size-large",
  xlarge: "size-xlarge",
};

export function initFontPanel(): void {
  const trigger = document.querySelector(".font-trigger") as HTMLElement | null;
  const pill = document.getElementById("fontPill");
  const anchor = document.querySelector<HTMLElement>(".font-anchor");
  if (!trigger || !pill || !anchor) return;

  // Guard against double-init
  if (pill.dataset.initialized) return;
  pill.dataset.initialized = "1";

  let currentScale = "";
  let isOpen = false;

  // Accumulated hold offset, physical px, reset to zero on every open and
  // on every close. Only ever written by `selectScale` while `isOpen`.
  let holdX = 0;
  let holdY = 0;

  function open(): void {
    if (isOpen) return;
    isOpen = true;
    holdX = 0;
    holdY = 0;
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

    // Release the hold: clearing both inline properties in the same step
    // hands the translate change to site.css's own `.font-anchor` transition
    // (a transition is decided by the AFTER-change style), so this animates
    // back to rest — the opposite of the instant write in selectScale below.
    holdX = 0;
    holdY = 0;
    anchor.style.transition = "";
    anchor.style.translate = "";
  }

  function selectScale(scale: string): void {
    pill.querySelectorAll("button").forEach((b) => b.classList.remove("active"));
    pill.querySelector(`[data-scale="${scale}"]`)?.classList.add("active");

    Object.values(SIZE_CLASSES).forEach((c) => trigger.classList.remove(c));
    trigger.classList.add(SIZE_CLASSES[scale]);
    currentScale = scale;

    // Hold still while open: `html.scale-*` reflows the whole page (title,
    // column width, the date row), which moves `.font-pill` along with
    // everything else even though the pill's own geometry relative to the
    // anchor never changes (site.css pins `.font-anchor`'s font-size, so the
    // pill's box doesn't resize here — only the page around it does).
    // Reading the pill's rect before and after the class swap therefore
    // isolates exactly that page-reflow displacement; accumulating it into
    // `.font-anchor`'s own inline `translate` cancels it, so the button the
    // reader just clicked stays under the pointer. `getBoundingClientRect`
    // is physical, and so is `translate`'s `<x> <y>` form, so this needs no
    // vertical-writing-mode branch — it isn't fighting the pill's own
    // scaleX/scaleY transform either, since `translate` is a separate CSS
    // property. Not applied when closed: the initial restore of a saved
    // scale on load must not shift a pill nobody has opened yet.
    const before = isOpen ? pill.getBoundingClientRect() : null;

    document.documentElement.className =
      document.documentElement.className.replace(/scale-\w+/g, "").trim() +
      (scale ? ` scale-${scale}` : "");
    localStorage.setItem("moss-font-scale", scale || "");

    if (before) {
      const after = pill.getBoundingClientRect();
      holdX += before.left - after.left;
      holdY += before.top - after.top;
      // `transition: none` in the same step as the translate write is what
      // keeps this instant — see close()'s release for the opposite case.
      anchor.style.transition = "none";
      anchor.style.translate = `${holdX}px ${holdY}px`;
    }
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

  // Dismiss on resize — a held translate is a snapshot of one viewport's
  // reflow; a resize (rotation, devtools, a font finishing its own async
  // load) invalidates it, and nothing else here would notice.
  window.addEventListener("resize", () => { if (isOpen) close(); }, { passive: true });

  // Restore saved font scale — pill is closed, so this writes no translate.
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

// Re-scan after an in-place morph (the preview iframe bridge). A
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

// The floating nav island — reveal on scroll-up, measured folding,
// the two panels, reading progress. Rides this bundle for the same reason the
// hint above does: theme.js is the nav-chrome runtime and is already on every
// page. Self-wires on import (DOMContentLoaded + `moss-morph-patched`), and
// no-ops on a page whose markup carries no island, which is every page without
// a breadcrumb trail.
import "./nav/nav-island";

// The blurred placeholder an image is painted with until it loads. Its own
// module so the scan and the load listener can be tested without this
// bundle's side effects; wired here because theme.js is on every page.
import { initImagePlaceholders } from "./image-placeholder";
initImagePlaceholders();

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
    // A key an earlier handler claimed, or one an open viewer (the gallery
    // lightbox, or a modal dialog) uses to step through its own items, must
    // not also leave the page.
    if (e.defaultPrevented) return;
    if (document.querySelector("#lightbox:not([hidden]), dialog[open]")) return;

    if (e.key === "ArrowLeft") {
      const prev = nav.querySelector<HTMLAnchorElement>(".moss-series-nav-prev");
      if (prev?.href) { e.preventDefault(); window.location.href = prev.href; }
    } else if (e.key === "ArrowRight") {
      const next = nav.querySelector<HTMLAnchorElement>(".moss-series-nav-next");
      if (next?.href) { e.preventDefault(); window.location.href = next.href; }
    }
  });
})();
