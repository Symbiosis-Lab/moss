/** Friendly (native) names for the audience-scope codes, so language
 *  selectors read "English / 简体中文" rather than raw "en / zh-hans".
 *  Mirrors `KNOWN_LANG_SUFFIXES` in crates/moss-core/src/home.rs.
 *  Unknown codes fall back to the code itself via `langName`. */
// Native/friendly names for every language code moss recognizes as a folder.
// An unknown code falls back to itself rather than showing a raw, unfriendly token.
export const LANG_NAMES: Record<string, string> = {
  // The root/default audience when its language is undetected. A multilingual
  // site's root is normally relabeled with its detected language (e.g. 简体中文).
  '': 'Default',
  en: 'English',
  zh: '简体中文',
  ja: '日本語',
  ko: '한국어',
  de: 'Deutsch',
  fr: 'Français',
  es: 'Español',
  it: 'Italiano',
  pt: 'Português',
  ru: 'Русский',
  ar: 'العربية',
  hi: 'हिन्दी',
  tr: 'Türkçe',
  pl: 'Polski',
  nl: 'Nederlands',
  sv: 'Svenska',
  da: 'Dansk',
  fi: 'Suomi',
  no: 'Norsk',
  cs: 'Čeština',
  hu: 'Magyar',
  ro: 'Română',
  el: 'Ελληνικά',
  vi: 'Tiếng Việt',
  th: 'ไทย',
  id: 'Bahasa Indonesia',
  he: 'עברית',
  uk: 'Українська',
  bg: 'Български',
  hr: 'Hrvatski',
  sr: 'Српски',
  sk: 'Slovenčina',
  sl: 'Slovenščina',
  et: 'Eesti',
  lv: 'Latviešu',
  lt: 'Lietuvių',
  cy: 'Cymraeg',
  mi: 'Māori',
  bo: 'བོད་སྐད་',
  gd: 'Gàidhlig',
  eu: 'Euskara',
  ca: 'Català',
  gl: 'Galego',
  sw: 'Kiswahili',
  'zh-hans': '简体中文',
  'zh-hant': '繁體中文',
  'zh-cn': '简体中文',
  'zh-tw': '繁體中文',
  'pt-br': 'Português (Brasil)',
  'en-us': 'English (US)',
  'en-gb': 'English (UK)',
};

/** Returns the friendly display name for a language scope code.
 *  Falls back to the input code itself for unrecognized values. */
export function langName(scope: string): string {
  return LANG_NAMES[scope] ?? scope;
}
