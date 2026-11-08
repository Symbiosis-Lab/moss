// Generic declarative localizer for static HTML that ships with English
// fallbacks baked into the markup. Driven by `data-i18n*` attributes, so the
// HTML is self-documenting and the pass needs no per-element hardcoding.
//
// Call once per webview, AFTER initLocale() (so the locale table is resolved)
// and BEFORE first interaction.
//
// Convention (set any combination on an element):
//   data-i18n="toolbar.key"          → element.textContent
//   data-i18n-aria="toolbar.key"     → aria-label attribute
//   data-i18n-tooltip="toolbar.key"  → data-tooltip attribute (portal tooltip)
//
// The HTML keeps its English text/attributes as the at-rest fallback; this swap
// only runs in a real webview where initLocale() has populated the table. Keys
// are looked up via t(), which already falls back to the en table and then to
// the raw key string if a key is missing — so the pass degrades gracefully.

import { t, getLocale } from './index';
import type { UiStringMap } from './ui-strings';

type I18nKey = keyof UiStringMap;

/**
 * Apply `data-i18n*` attributes within `root` (default: whole document).
 * Idempotent — safe to call again after injecting more static markup.
 */
export function applyStaticStrings(root: ParentNode = document): void {
  // Keep the document language attribute in sync so the CJK font/justify CSS
  // (html[lang^="zh"], :lang(zh-*)) applies.
  if (root === document) {
    document.documentElement.lang = getLocale();
  }

  root.querySelectorAll<HTMLElement>('[data-i18n]').forEach((el) => {
    const key = el.dataset.i18n;
    if (key) el.textContent = t(key as I18nKey);
  });

  root.querySelectorAll<HTMLElement>('[data-i18n-aria]').forEach((el) => {
    const key = el.dataset.i18nAria;
    if (key) el.setAttribute('aria-label', t(key as I18nKey));
  });

  root.querySelectorAll<HTMLElement>('[data-i18n-tooltip]').forEach((el) => {
    const key = el.dataset.i18nTooltip;
    if (key) el.setAttribute('data-tooltip', t(key as I18nKey));
  });
}
