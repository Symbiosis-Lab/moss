/**
 * moss — Hover Link Preview
 *
 * Shows a tooltip popup when the reader hovers (or Tab-focuses) an internal
 * link.  Data is lazy-fetched from /_moss/previews.json on first interaction.
 *
 * WCAG 2.2 SC 1.4.13: keyboard-triggerable (focusin), dismissible (Escape),
 * hoverable (grace period), screen-reader accessible (role=tooltip, aria).
 */
import { clampLeft, maxSurfaceWidth, verticalSlot } from "./viewport";

(function () {
  "use strict";

  if ((window as any).__moss_no_preview) return;
  if ("ontouchstart" in window && navigator.maxTouchPoints > 0) return;

  type PreviewEntry = { title: string; description?: string; preview?: string };
  type PreviewData = Record<string, PreviewEntry>;

  let data: PreviewData | null = null;
  let popup: HTMLDivElement | null = null;
  let currentLink: HTMLAnchorElement | null = null;
  let showTimer: ReturnType<typeof setTimeout> | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  let idCounter = 0;

  function esc(s: string): string {
    const d = document.createElement("div");
    d.textContent = s;
    return d.innerHTML;
  }

  function isExternal(href: string): boolean {
    try { return new URL(href, location.origin).origin !== location.origin; }
    catch { return true; }
  }

  function isSkippable(link: HTMLAnchorElement): boolean {
    if (!link.classList.contains("wikilink")) return true;
    const href = link.getAttribute("href");
    if (!href || href.charAt(0) === "#") return true;
    if (isExternal(link.href)) return true;
    try {
      if (new URL(link.href).pathname === location.pathname) return true;
    } catch { /* ignore */ }
    return false;
  }

  function ensureData(cb: () => void): void {
    if (data) return cb();
    fetch("/_moss/previews.json")
      .then(r => r.json())
      .then((d: PreviewData) => { data = d; cb(); })
      .catch(() => { /* silent — previews are optional */ });
  }

  function createPopup(): void {
    if (popup) return;
    popup = document.createElement("div");
    popup.className = "moss-preview-popup";
    popup.setAttribute("role", "tooltip");
    popup.setAttribute("aria-live", "polite");
    popup.addEventListener("mouseenter", () => { if (hideTimer) clearTimeout(hideTimer); });
    popup.addEventListener("mouseleave", () => { scheduleHide(); });
    popup.addEventListener("click", () => {
      if (currentLink) window.location.href = currentLink.href;
    });
    document.body.appendChild(popup);
  }

  function showPopup(link: HTMLAnchorElement): void {
    ensureData(() => {
      // Decode the pathname: `previews.json` keys are raw UTF-8 paths
      // (e.g. `/zh-hans/文档/from-matters/`, from the page's `url_path`), but
      // `URL.pathname` percent-encodes non-ASCII segments (文档 →
      // %E6%96%87%E6%A1%A3). Without decoding, the lookup misses for every CJK
      // (or space/Unicode) path and the popup silently never shows. Mirrors the
      // decode already done in thumb-swap.ts / iframe-bridge.ts / path-utils.ts.
      let path = decodeURIComponent(new URL(link.href).pathname);
      if (path.charAt(path.length - 1) !== "/" && path.indexOf(".") === -1) {
        path = path + "/";
      }
      const entry = data![path];
      if (!entry) return;

      createPopup();
      if (hideTimer) clearTimeout(hideTimer);
      currentLink = link;

      let html = '<strong class="moss-preview-title">' + esc(entry.title) + "</strong>";
      if (entry.description) {
        html += '<p class="moss-preview-desc">' + esc(entry.description) + "</p>";
      }
      if (entry.preview) {
        html += '<p class="moss-preview-text">' + esc(entry.preview) + "</p>";
      }
      popup!.innerHTML = html;

      const id = "moss-preview-" + (++idCounter);
      popup!.id = id;
      link.setAttribute("aria-describedby", id);

      // Cap the card against the visible band before measuring it, so a narrow
      // phone gets a card that wraps rather than one that overflows: the CSS
      // `max-width: 360px` alone is wider than the screen below 376px.
      popup!.style.maxWidth = Math.min(360, maxSurfaceWidth()) + "px";
      popup!.classList.add("visible");

      // The card is `opacity`-hidden rather than `display: none`, so it is
      // already laid out and both dimensions read true right here. No
      // requestAnimationFrame: the old code positioned naively, painted, then
      // corrected on the next frame, which is one frame of visibly wrong
      // placement — and its correction measured `window.innerWidth`, which
      // includes the scrollbar gutter, so on a desktop engine that reserves one
      // the "fixed" card still sat partly underneath it.
      const rect = link.getBoundingClientRect();
      const pr = popup!.getBoundingClientRect();

      // `position: absolute`, so the vertical slot (viewport coordinates) has
      // to be carried back into document coordinates by the scroll offset.
      // Below the link by preference — above it only when the link is near the
      // bottom of the window.
      popup!.style.left = clampLeft(rect.left, pr.width) + "px";
      popup!.style.top =
        verticalSlot(rect, pr.height, "below").top + window.scrollY + "px";
    });
  }

  function hidePopup(): void {
    if (!popup) return;
    popup.classList.remove("visible");
    if (currentLink) {
      currentLink.removeAttribute("aria-describedby");
      currentLink = null;
    }
  }

  function scheduleHide(): void {
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(hidePopup, 150);
  }

  document.addEventListener("mouseover", (e: MouseEvent) => {
    const link = (e.target as Element).closest?.("a[href]") as HTMLAnchorElement | null;
    if (!link || isSkippable(link)) return;
    if (hideTimer) clearTimeout(hideTimer);
    if (showTimer) clearTimeout(showTimer);
    showTimer = setTimeout(() => showPopup(link), 200);
  }, true);

  document.addEventListener("mouseout", (e: MouseEvent) => {
    const link = (e.target as Element).closest?.("a[href]");
    if (!link) return;
    if (showTimer) clearTimeout(showTimer);
    scheduleHide();
  }, true);

  document.addEventListener("focusin", (e: FocusEvent) => {
    const link = (e.target as Element).closest?.("a[href]") as HTMLAnchorElement | null;
    if (!link || isSkippable(link)) return;
    if (hideTimer) clearTimeout(hideTimer);
    showPopup(link);
  });

  document.addEventListener("focusout", (e: FocusEvent) => {
    const link = (e.target as Element).closest?.("a[href]");
    if (!link) return;
    scheduleHide();
  });

  document.addEventListener("keydown", (e: KeyboardEvent) => {
    if (e.key === "Escape" && popup && popup.classList.contains("visible")) {
      if (showTimer) clearTimeout(showTimer);
      if (hideTimer) clearTimeout(hideTimer);
      hidePopup();
    }
  });
})();

// Page-colour extraction used to live here. It now lives in the bridge
// (js-src/bridge/iframe-bridge-theme.ts), because this file ships as
// preview.js only when `[site].link_preview` is true — so with hover previews
// switched off, the shell's chrome tint had no reporter at all and wedged on
// whatever backdrop it last measured. The bridge is injected unconditionally.

// Auto-hide scrollbar: show on scroll, fade after 1.5s idle.
(function () {
  let scrollTimeout: ReturnType<typeof setTimeout>;
  window.addEventListener("scroll", () => {
    document.documentElement.classList.add("is-scrolling");
    clearTimeout(scrollTimeout);
    scrollTimeout = setTimeout(() => {
      document.documentElement.classList.remove("is-scrolling");
    }, 1500);
  }, { passive: true });
})();
