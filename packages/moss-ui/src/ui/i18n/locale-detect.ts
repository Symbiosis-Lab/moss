import { setLocale, type AppLocale } from './index';
import { UI_STRINGS } from './ui-strings';

declare global {
  interface Window {
    __MOSS_LOCALE__?: string;
  }
}

/**
 * Read window.__MOSS_LOCALE__ (injected by Rust via initialization_script)
 * and initialize the app-shell locale. Call once before any UI renders.
 * Falls back to 'en' if the variable is absent or unrecognized.
 */
export function initLocale(): void {
  const raw = window.__MOSS_LOCALE__ ?? 'en';
  const valid = Object.keys(UI_STRINGS) as AppLocale[];
  const locale: AppLocale = valid.includes(raw as AppLocale)
    ? (raw as AppLocale)
    : 'en';
  setLocale(locale);
}
