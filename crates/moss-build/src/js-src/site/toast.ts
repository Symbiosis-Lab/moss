/**
 * Shared transient toast for published-site feedback (e.g. "Copied to
 * clipboard"). Reuses the existing `.share-toast` CSS in site.css.
 * Extracted from share-card.ts so heading-anchor.ts and share-card.ts
 * share one implementation.
 */
import { langBucket } from "./subscribe/i18n";

const COPY = {
  en: "Copied to clipboard",
  "zh-hans": "已复制到剪贴板",
  "zh-hant": "已複製到剪貼簿",
} as const;

export function showToast(message?: string): void {
  const msg = message ?? COPY[langBucket(document.documentElement.lang)];

  const el = document.createElement("div");
  el.className = "share-toast";
  // role="status" gives this an implicit aria-live="polite" region, so a
  // screen reader announces the confirmation even though nothing kept
  // focus (WCAG 4.1.3) — the toast fades on its own timer and no control
  // ever points at it.
  el.setAttribute("role", "status");
  // Insert the region empty and fill it on the next frame. A live region that
  // arrives with its text already in place is a new region, not a change to an
  // existing one, and several screen readers announce nothing at all for that —
  // the announcement is triggered by the mutation, so the region has to be
  // observed first.
  document.body.appendChild(el);

  requestAnimationFrame(() => {
    el.textContent = msg;
    el.classList.add("visible");
  });
  setTimeout(() => {
    el.classList.remove("visible");
    setTimeout(() => el.remove(), 300);
  }, 1500);
}
