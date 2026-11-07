// Live language switch wiring for an app-chrome webview (no restart).
//
// Call installLocaleReactivity() ONCE per webview, right after
// initLocale() + applyStaticStrings() in its init. It:
//   1. re-applies declarative `data-i18n` markup on every locale change, and
//   2. listens for the backend `LocaleChanged` broadcast and calls setLocale(),
//      which fires every onLocaleChange subscriber (applyStaticStrings + each
//      persistent component's relabel closure).
//
// The subscription is webview-level (registered once, lives for the webview's
// lifetime), so callers normally don't need to unsubscribe. Per-COMPONENT
// relabel closures (file tree, chip bar, …) DO unsubscribe on unmount.

import { onMossEvent } from '../utils/moss-events';
import { setLocale, onLocaleChange, type AppLocale } from './index';
import { applyStaticStrings } from './apply-static-strings';
import { UI_STRINGS } from './ui-strings';

const VALID_LOCALES = Object.keys(UI_STRINGS) as AppLocale[];

/**
 * Wire the current webview for live language switching. Returns the unlisten
 * from the `LocaleChanged` subscription (usually unused — webview-level).
 */
export function installLocaleReactivity(): Promise<() => void> {
  // Declarative markup re-localizes on every change.
  onLocaleChange(() => applyStaticStrings());
  // Backend broadcast → update locale. Validate the payload before casting
  // (mirrors initLocale's guard); the event carries the hyphenated AppLocale
  // tag ("zh-hans"), never the snake_case AppLanguage binding value.
  return onMossEvent('LocaleChanged', (msg) => {
    const l = msg.payload.locale;
    if ((VALID_LOCALES as readonly string[]).includes(l)) {
      setLocale(l as AppLocale);
    }
  });
}
