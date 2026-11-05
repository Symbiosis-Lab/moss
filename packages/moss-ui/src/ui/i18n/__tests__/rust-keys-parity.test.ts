/**
 * Cross-language i18n key-set parity (natively-emitted key dumps, never node-regex over Rust source).
 *
 * The contract under test: `BUILTIN_FIELDS.label_key` in
 * crates/moss-core/src/schema_fields.rs declares `chip.<name>.label` keys that
 * the frontend resolves through UI_STRINGS (chipLabel() warns-and-falls-back on
 * a miss — runtime-only detection without this test). The Rust side of the
 * contract is emitted natively by
 * `cargo run --bin generate-artifacts --features dev-tools -- i18n-keys`
 * into rust-keys.generated.json, which is committed; the desktop app's
 * release check regenerates it and fails when it differs.
 *
 * The two big string registries (the Rust side's, for site output, and
 * ui-strings.ts, for the UI) are disjoint; this test
 * deliberately covers only the chip-label contract that actually crosses the
 * language boundary.
 */

import { describe, it, expect } from 'vitest';
import { UI_STRINGS, type UiStringMap } from '../ui-strings';
import rustKeys from '../rust-keys.generated.json';

const chipLabelKeys: string[] = rustKeys.chipLabelKeys;
const fmHintKeys: string[] = rustKeys.fmHintKeys;
const channelNameKeys: string[] = rustKeys.channelNameKeys;
const locales = Object.keys(UI_STRINGS) as Array<keyof typeof UI_STRINGS>;

describe('Rust ↔ TS chip-label key parity', () => {
  it('the generated dump is present and non-trivial', () => {
    expect(chipLabelKeys.length).toBeGreaterThan(10);
    for (const k of chipLabelKeys) expect(k).toMatch(/^chip\.[a-zA-Z_]+\.label$/);
  });

  it('every Rust-declared chip label key resolves in EVERY locale table', () => {
    const missing: string[] = [];
    for (const locale of locales) {
      const table = UI_STRINGS[locale] as Record<string, string>;
      for (const k of chipLabelKeys) {
        if (typeof table[k] !== 'string' || table[k].length === 0) {
          missing.push(`${locale}: ${k}`);
        }
      }
    }
    expect(missing,
      `Rust BUILTIN_FIELDS.label_key entries unresolved in ui-strings.ts — add the keys, or if the field was removed, regen the dump (generate-artifacts i18n-keys)`
    ).toEqual([]);
  });

  it('every chip.*.label key in the TS tables is Rust-declared (no orphans)', () => {
    const declared = new Set(chipLabelKeys);
    const orphans: string[] = [];
    const en = UI_STRINGS[locales[0]] as Record<string, string>;
    for (const k of Object.keys(en)) {
      if (/^chip\.[a-zA-Z_]+\.label$/.test(k) && !declared.has(k)) orphans.push(k);
    }
    expect(orphans,
      `chip.*.label keys in ui-strings.ts with no BUILTIN_FIELDS declaration — delete them, or regen a stale dump (generate-artifacts i18n-keys)`
    ).toEqual([]);
  });
});

// Frontmatter hints: every
// schema-exposed BUILTIN_FIELDS entry has an `fm_hint.<name>` string in every
// locale — a new field added without hints fails here, not at runtime.
describe('Rust ↔ TS frontmatter-hint key parity', () => {
  it('the generated dump is present and non-trivial', () => {
    expect(fmHintKeys.length).toBeGreaterThan(10);
    for (const k of fmHintKeys) expect(k).toMatch(/^fm_hint\.[a-zA-Z_]+$/);
  });

  it('every schema-exposed field has a hint string in EVERY locale table', () => {
    const missing: string[] = [];
    for (const locale of locales) {
      const table = UI_STRINGS[locale] as Record<string, string>;
      for (const k of fmHintKeys) {
        if (typeof table[k] !== 'string' || table[k].length === 0) {
          missing.push(`${locale}: ${k}`);
        }
      }
    }
    expect(missing,
      `BUILTIN_FIELDS entries with no fm_hint string in ui-strings.ts — add the hints (en + de + zh-hans + zh-hant), or if the field was removed, regen the dump (generate-artifacts i18n-keys)`
    ).toEqual([]);
  });

  it('every fm_hint.* key in the TS tables belongs to a schema field (no orphans)', () => {
    const declared = new Set(fmHintKeys);
    const orphans: string[] = [];
    const en = UI_STRINGS[locales[0]] as Record<string, string>;
    for (const k of Object.keys(en)) {
      if (/^fm_hint\./.test(k) && !declared.has(k)) orphans.push(k);
    }
    expect(orphans,
      `fm_hint.* keys in ui-strings.ts with no BUILTIN_FIELDS declaration — delete them, or regen a stale dump (generate-artifacts i18n-keys)`
    ).toEqual([]);
  });
});

/**
 * A channel moss ships takes its name from the locale table and nowhere else.
 *
 * `AvailablePlugin` carries no Rust-authored name, so an omission here shows
 * up only as a Title-Cased id and a console warning — this test is what
 * turns that into a red suite instead.
 */
describe('every channel moss ships is nameable from the locale alone', () => {
  it('the generated dump lists the shipped channels', () => {
    expect(channelNameKeys.length).toBeGreaterThan(0);
    for (const k of channelNameKeys) expect(k).toMatch(/^channels\.name\.[a-z0-9-]+$/);
  });

  it('resolves in EVERY locale', () => {
    const missing: string[] = [];
    for (const locale of locales) {
      const table = UI_STRINGS[locale] as Record<string, string>;
      for (const k of channelNameKeys) {
        if (typeof table[k] !== 'string' || table[k].length === 0) missing.push(`${locale}: ${k}`);
      }
    }
    expect(missing).toEqual([]);
  });
});
