/**
 * The checks every localized string table the app ships must pass. Each table
 * is guarded beside it: `ui-strings` in i18n/, the slash and insert-bar table
 * in editor/. A table left out of these checks is how a block-name label once
 * stayed English in a Chinese UI, so a new table gets its own call.
 */
import { describe, it, expect } from 'vitest';
import type { AppLocale } from '../ui-strings';

// Companion to the Rust `no_chinese_value_is_byte_identical_to_english`
// test on the advisory catalog: catches a key added with its
// Chinese value byte-identical to English (copy-paste-untranslated). Compile-time
// `Record<AppLocale, UiStringMap>` parity already guarantees keys EXIST in every map;
// this guards that the Chinese ones were actually translated.
//
// ALLOWLIST holds entries whose Chinese value is LEGITIMATELY identical to English
// (brand names, pure technical tokens, symbols). Format: `${locale}:${key}`.
const ALLOWLIST = new Set<string>(
  // Example data (sample addresses / URLs / slugs) and pure-interpolation strings
  // carry no translatable text. Block names are NOT in that set: the slash menu
  // and the insert bar's catalog show them as prose labels, so a Chinese reader
  // saw a row of English words among translated ones. They are translated now —
  // the shortcode the label inserts (`:::gallery`) stays English either way.
  [
    'subscribers.emails_placeholder', // sample email addresses
    'deployment.email.placeholder', // sample email
    'domain.phone_placeholder', // sample phone number
    'schema.site_id_placeholder', // sample slug ("my-blog")
    'audience.breakdown_segment', // pure interpolation: "{name} · {count}"
    'pubmgr.lamp.tooltip', // pure interpolation: "{name} — {state} · {hint}"
    'upload.size_pair', // pure interpolation: "{done}/{total} {unit}" — kB/MB/GB stay Latin in Chinese technical UI
    'upload.size_one', // pure interpolation: "{n} {unit}" — same reason as the pair
    'channels.installing_caption', // pure interpolation: "{percent}%" — the % sign is identical in Chinese numeric UI
    'channels.verb_step_join', // pure punctuation: "{verb} — {step}"; the em-dash join is what the zh step tooltips already use. It is a KEY rather than a code-level `+` so a locale that wants 、or no spacing can have it
    'channels.name.matters', // brand name — identical in every locale (email IS localized)
    'channels.name.github', // brand name — identical in every locale
    'channels.name.onionpress', // brand name — identical in every locale
    'deployment.host.moss', // brand name — the hosting destination is mosspub.com in every locale
  ].flatMap((key) => [`zh-hans:${key}`, `zh-hant:${key}`]),
);

// German values legitimately identical to English. Format: `de:${key}`.
const GERMAN_IDENTICAL = [
  // brand and product names
  'launcher.wordmark',
  'channels.name.matters',
  'channels.name.github',
  'channels.name.onionpress',
  'deployment.host.moss',
  // loanwords German uses unchanged (Duden-listed, glossary "Loanwords")
  'chip.status.label',
  'subscribers.col_status',
  'chip.tickets.label',
  'chip.online.label',
  'chip.logo.label',
  'chip.layout.label',
  'chip.jury.label',
  'versions.back_editor',
  'analytics.col_medium',
  'progress.details',
  'onboard.card_plugin_title',
  'publish_receipt.row_newsletter',
  'deployment.label.host',
  'deployment.label.domain',
  'deployment.label.repository',
  'domain.label_name',
  'pubmgr.website.label',
  'toolbar.website',
  'toolbar.code',
  'toolbar.link',
  // acronyms and abbreviations that are the same word in German
  'chip.url.label',
  'onboard.card_url_title',
  'dialog.ok',
  // sample data
  'subscribers.emails_placeholder',
  // pure interpolation or punctuation: no translatable text
  'trust.ability_line',
  'trust.changed_version',
  'channels.verb_step_join',
  'onboard.starter.status_of',
  'audience.breakdown_segment',
  'audience.breakdown_separator',
  'audience.breakdown_wrap',
  'pubmgr.lamp.tooltip',
  'tree.derive_count',
  'upload.size_pair',
  'upload.size_one',
  'upload.rate',
  // "Version" is the German word too
  'plugin.version.installed',
  // slash-menu block name, WordPress de_DE keeps "Buttons"
  'slash.buttons.label',
].map((key) => `de:${key}`);
for (const k of GERMAN_IDENTICAL) ALLOWLIST.add(k);

const TRANSLATED: AppLocale[] = ['de', 'zh-hans', 'zh-hant'];
// The Chinese-only rules below (青苔, 插件): German keeps "moss" and has no 外掛.
const CHINESE = TRANSLATED.filter((l) => l.startsWith('zh'));

/** Keys whose Chinese value legitimately keeps the Latin name, `${locale}:${key}`. */
const BRAND_ALLOWLIST = new Set<string>(
  [
    // The sender name on the real email is the Latin "moss" — a Chinese reader
    // scanning their spam folder is looking for that string, not for 青苔.
    'publish.awaiting_advice',
    // `moss build` is the command a terminal runs, typed the same in every language.
    'appsettings.terminal_desc',
  ].flatMap((key) => [`zh-hans:${key}`, `zh-hant:${key}`]),
);

/** Does this string use the bare product name in prose? `mosspub.com`,
 * `.moss/config.toml`, `moss.log`, `moss-*` class names and the reserved
 * `moss:` action-id prefix are identifiers, not the brand — a neighbouring
 * `.`, `-`, `/`, `_`, `:` or alphanumeric marks one. */
function saysMossInProse(v: string): boolean {
  return /(^|[^\w./-])moss($|[^\w./:-])/i.test(v);
}

export type StringTables = Record<string, Record<AppLocale, Record<string, string>>>;

/** Registers the translation checks for `tables`, by table name. */
export function guardStringTables(TABLES: StringTables): void {
  describe('no untranslated leakage', () => {
    for (const [table, strings] of Object.entries(TABLES)) {
      for (const locale of TRANSLATED) {
        it(`${table}: ${locale} has no value byte-identical to English`, () => {
          const en_map = strings.en;
          const map = strings[locale];
          const leaks: string[] = [];
          for (const key of Object.keys(en_map)) {
            const en = en_map[key];
            const tr = map[key];
            if (typeof en !== 'string' || typeof tr !== 'string') continue;
            if (en === tr && !ALLOWLIST.has(`${locale}:${key}`)) {
              leaks.push(`${key} = ${JSON.stringify(en)}`);
            }
          }
          expect(
            leaks,
            `Untranslated ${locale} values in ${table} (translate them, or add \`${locale}:<key>\` to ALLOWLIST if the English form is intentional):\n${leaks.join('\n')}`,
          ).toEqual([]);
        });
      }
    }
  });

  // The product is "moss" in English and 青苔 in Chinese — the calque the tray, the
  // app menu and the site colophon (青苔发布) have always used. A Chinese sentence
  // with a bare Latin "moss" in it is drift, not a brand decision. The Rust twin is
  // `chinese_values_call_the_product_qingtai` in `infra/app_advisory.rs`.
  describe('the product is 青苔 in Chinese', () => {
    for (const [table, strings] of Object.entries(TABLES)) {
      for (const locale of CHINESE) {
        it(`${table}: ${locale} never says "moss" in prose`, () => {
          const map = strings[locale];
          const leaks = Object.keys(map).filter(
            (key) =>
              typeof map[key] === 'string' &&
              saysMossInProse(map[key]) &&
              !BRAND_ALLOWLIST.has(`${locale}:${key}`),
          );
          expect(
            leaks.map((k) => `${k} = ${JSON.stringify(map[k])}`),
            `Chinese values must call the product 青苔 (or add \`${locale}:<key>\` to BRAND_ALLOWLIST with a reason):`,
          ).toEqual([]);
        });
      }
    }
  });

  // A plugin is 插件 in both Chinese locales. zh-Hant must not use the Taiwanese
  // 外掛, which would read as a second feature to the same reader. The choice is deliberate — one
  // term across both locales — so a well-meaning translator "correcting" it back
  // should fail here rather than in review. The Rust twin covers the advisory
  // catalog in `infra/app_advisory.rs`.
  describe('a plugin is 插件 in Chinese', () => {
    for (const [table, strings] of Object.entries(TABLES)) {
      for (const locale of CHINESE) {
        it(`${table}: ${locale} never says 外掛`, () => {
          const map = strings[locale];
          const leaks = Object.keys(map).filter(
            (key) => typeof map[key] === 'string' && map[key].includes('外掛'),
          );
          expect(
            leaks.map((k) => `${k} = ${JSON.stringify(map[k])}`),
            'Chinese values must call a plugin 插件, in zh-hant as well as zh-hans:',
          ).toEqual([]);
        });
      }
    }
  });
}
