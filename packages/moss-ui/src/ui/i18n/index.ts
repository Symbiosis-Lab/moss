import { UI_STRINGS, PLATFORM_VARIANTS, type AppLocale, type PlatformVariantKey, type UiStringMap } from './ui-strings';
import { uiPlatform } from '../utils/platform';

let locale: AppLocale = 'en';

/**
 * Subscribers notified after the locale changes (live language switch, no
 * restart). Persistent chrome registers a relabel closure here; each app-chrome
 * webview also registers `applyStaticStrings` so declarative `data-i18n` markup
 * re-localizes on every change.
 */
const localeSubscribers = new Set<() => void>();

/**
 * Subscribe to live locale changes. Returns an unsubscribe function — callers
 * whose component mounts/unmounts (file tree, chip bar, …) MUST call it on
 * teardown so subscriptions don't accumulate and relabel dead DOM.
 */
export function onLocaleChange(cb: () => void): () => void {
  localeSubscribers.add(cb);
  return () => {
    localeSubscribers.delete(cb);
  };
}

export function setLocale(l: AppLocale): void {
  if (l === locale) return; // no-op: don't churn subscribers on a same-locale set
  locale = l; // set BEFORE notifying so t()/localeTag() inside subscribers see the new value
  // Drop old-locale Intl formatters. The cache key already includes the tag, so
  // a new tag is a natural miss; clearing removes all doubt if a subscriber
  // formats a date/number synchronously right after the switch.
  dtfCache.clear();
  nfCache.clear();
  // Iterate a COPY: a subscriber may add/remove subscribers (or re-enter
  // setLocale) during notification; iterating the live Set would be unsafe.
  for (const cb of [...localeSubscribers]) {
    try {
      cb();
    } catch (e) {
      console.error('[i18n] locale subscriber failed', e);
    }
  }
}

export function getLocale(): AppLocale {
  return locale;
}

/** Values substitutable into a `{name}` placeholder. */
export type TParams = Record<string, string | number>;

/**
 * Substitute `{name}` placeholders in a template from `params`.
 * Unknown placeholders are left untouched. The one interpolation primitive —
 * mirror of the Rust `interpolate()` in `i18n/strings.rs`. No plural engine:
 * the ~handful of English count strings branch at the call site.
 */
export function interpolate(template: string, params: TParams): string {
  return template.replace(/\{(\w+)\}/g, (whole, name) =>
    Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : whole,
  );
}

/**
 * Type-safe literal key lookup. Key typos are TypeScript compile errors.
 * Pass `params` to fill `{name}` placeholders, e.g. `t('publish.as', { email })`.
 */
export function t(key: keyof UiStringMap, params?: TParams): string {
  const p = uiPlatform();
  const variant = p === 'other' ? undefined : PLATFORM_VARIANTS[locale][`${key}@${p}` as PlatformVariantKey];
  const template = variant ?? UI_STRINGS[locale][key] ?? UI_STRINGS.en[key] ?? key;
  return params ? interpolate(template, params) : template;
}

/**
 * A refusal, as the two screens that render one need it — the catalog tile and
 * the publish rail's button.
 *
 * Structural rather than the generated `Refusal`, so this module stays off the
 * Tauri seam. Rust decides WHICH refusal it is and supplies the parts; the
 * whole sentence is built here, so the tile and the button cannot drift into
 * two wordings for one verdict — and so a translated frame is the only frame.
 * A withdrawal is the exception that proves it: `reason` there is the
 * registry's own published sentence, which moss quotes rather than writes.
 */
export type RefusalLike =
  | { kind: 'revoked'; reason: string }
  | { kind: 'incompatible'; needs: string; host: string }
  | { kind: 'unapproved' }
  | { kind: 'unavailable'; platforms: string[] }
  | { kind: 'unreadable_floor'; floor: string };

/** Platform keys as the registry spells them, in the reader's words; an unknown key is shown as written. */
function platformWords(platforms: string[]): string {
  const words = new Set(
    platforms.map((key) => {
      const k = `platform.${key}` as keyof UiStringMap;
      return k in UI_STRINGS.en ? t(k) : key;
    }),
  );
  if (words.size === 0) return t('platform.other');
  return new Intl.ListFormat(localeTag(), { type: 'conjunction' }).format([...words]);
}

export function refusalTooltip(refusal: RefusalLike, name: string): string {
  switch (refusal.kind) {
    case 'revoked':
      return t('channels.revoked_tooltip', { name, reason: refusal.reason });
    case 'incompatible':
      return t('channels.incompatible_tooltip', { name, needs: refusal.needs, host: refusal.host });
    case 'unapproved':
      return t('channels.unapproved_tooltip', { name });
    case 'unavailable':
      return t('channels.unavailable_tooltip', { name, where: platformWords(refusal.platforms) });
    case 'unreadable_floor':
      return t('channels.unreadable_floor_tooltip', { name, floor: refusal.floor });
  }
}

/**
 * Look up a chip label by field name from the schema.
 * Use this instead of t('chip.fieldName.label') to avoid unsafe concatenation.
 * Warns in console if the key is missing — caught immediately in dev.
 */
export function chipLabel(fieldName: string): string {
  const key = `chip.${fieldName}.label` as keyof UiStringMap;
  if (!(key in UI_STRINGS.en)) {
    console.warn(`[i18n] Missing chip label key: ${key}`);
    // A plugin contributes frontmatter fields, and moss has no key for a key
    // it did not know would exist — so this fallback draws a plugin-authored
    // string beside moss's own chip labels. Same treatment as a sideloaded
    // plugin's name, for the same reason.
    return displayable(fieldName);
  }
  return t(key);
}

const INVISIBLE = /[\p{Cc}\u061c\u200e\u200f\u2028\u2029\u202a-\u202e\u2066-\u2069]/gu;
const MAX_NAME = 64;

/**
 * Strip the characters that let a short string repaint the sentence around it,
 * and cap the length.
 *
 * A channel's `id` is a plugin's `manifest.name`, and for a SIDELOADED plugin
 * nothing validates it: the registry pins ids on the install path, a folder
 * dropped into `.moss/plugins/` is not on that path. The result lands in
 * `aria-label` and `data-tooltip` in the publish rail. The Rust side runs
 * every published string through `publisher_text` — `plugin_human_name` and
 * the catalog row included — so this is that rule at the one boundary Rust
 * does not own: a name moss derives here, from an id.
 */
function displayable(name: string): string {
  const visible = [...name.replace(INVISIBLE, '')];
  return visible.length > MAX_NAME ? `${visible.slice(0, MAX_NAME).join('')}\u2026` : visible.join('');
}

/**
 * Resolve a channel's user-facing display name from its stable id
 * (`email`, `matters`, `github`, …) via the i18n system. Every channel moss
 * SHIPS must have a `channels.name.<id>` key in ui-strings.ts, and that key is
 * what a row without a `published` name is called, so no hardcoded
 * Rust/English string reaches the UI for one of those.
 *
 * `published` WINS over the key, and Rust decides when a row has one
 * (`registry::published_name`, plus the catalog's identity-follows-the-bytes
 * rule). It is `None` for a plugin moss ships — there a missing key is a bug,
 * and the warning below has to fire rather than the manifest's English
 * quietly rendering — and `Some` in the two cases where the key would name
 * the wrong plugin: a plugin moss does not ship, which can have no key, and a
 * folder's own bytes sitting under an id moss or the registry also knows.
 * That ordering is why this reads `published` first: the caller does not get
 * to re-derive the rule, and there is exactly one place it lives.
 *
 * The registry's published English already reaches the UI the same way for
 * `channels.revoked_tooltip`'s reason.
 *
 * With no key and no published name, the id is Title-Cased and the warning
 * still fires.
 */
export function channelName(id: string, published?: string | null): string {
  if (published) return displayable(published);
  const key = `channels.name.${id}` as keyof UiStringMap;
  if (!(key in UI_STRINGS.en)) {
    console.warn(`[i18n] Missing channel name key: ${key}`);
    // Last-resort label for an unkeyed channel: Title-Case the id. Add a
    // `channels.name.<id>` key to localize it.
    return displayable(
      id
        .replace(/-syndicator$/, '')
        .split('-')
        .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
        .join(' ')
    );
  }
  return t(key);
}

/**
 * Look up a frontmatter field's hint (the property bar's chip tooltip) by
 * field name. `fm_hint.<field>` keys exist for every schema-exposed
 * BUILTIN_FIELDS entry (bound by rust-keys-parity.test.ts); unknown
 * user-authored fields have none and get an empty string — no tooltip.
 */
export function fieldHint(fieldName: string): string {
  const key = `fm_hint.${fieldName}` as keyof UiStringMap;
  if (!(key in UI_STRINGS.en)) return '';
  return t(key);
}

/**
 * Look up the "+" menu add-row's detail line (`chip.<key>.detail`) by field
 * name — the shorter sibling of `fieldHint` above, and the same absence rule:
 * `chip.<key>.detail` keys exist only for builtin fields whose name doesn't
 * already say what they do (title/date/publisher/nav are skipped as
 * self-explanatory, ui-strings.ts); a plugin-contributed or otherwise unkeyed
 * field gets an empty string, so its add row renders no second line rather
 * than a fabricated one.
 */
export function chipDetail(fieldName: string): string {
  const key = `chip.${fieldName}.detail` as keyof UiStringMap;
  if (!(key in UI_STRINGS.en)) return '';
  return t(key);
}

/**
 * Canonical BCP-47 tag (title-case script subtag) for the active (or given)
 * locale — for `Intl.*` and generated `lang`/`hreflang` attributes. The runtime
 * key stays lowercase (`zh-hans`); only the emitted tag is title-cased.
 */
export function localeTag(l: AppLocale = locale): string {
  switch (l) {
    case 'de':
      return 'de';
    case 'zh-hans':
      return 'zh-Hans';
    case 'zh-hant':
      return 'zh-Hant';
    default:
      return 'en';
  }
}

const dtfCache = new Map<string, Intl.DateTimeFormat>();
const nfCache = new Map<string, Intl.NumberFormat>();

/** Locale-aware date formatting. NEVER hardcode 'en-US'. */
export function formatDate(d: Date, opts?: Intl.DateTimeFormatOptions): string {
  const tag = localeTag();
  const key = tag + '|' + JSON.stringify(opts ?? {});
  let f = dtfCache.get(key);
  if (!f) {
    f = new Intl.DateTimeFormat(tag, opts);
    dtfCache.set(key, f);
  }
  return f.format(d);
}

/** Locale-aware number formatting. NEVER hardcode 'en-US'. */
export function formatNumber(n: number, opts?: Intl.NumberFormatOptions): string {
  const tag = localeTag();
  const key = tag + '|' + JSON.stringify(opts ?? {});
  let f = nfCache.get(key);
  if (!f) {
    f = new Intl.NumberFormat(tag, opts);
    nfCache.set(key, f);
  }
  return f.format(n);
}

export type { AppLocale, UiStringMap };
