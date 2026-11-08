import { describe, it, expect } from 'vitest';
import { LANG_NAMES, langName } from '../lang-names';

describe('langName', () => {
  it('returns 简体中文 for zh-hans', () => {
    expect(langName('zh-hans')).toBe('简体中文');
  });

  it('returns English for en', () => {
    expect(langName('en')).toBe('English');
  });

  it('returns 繁體中文 for zh-hant', () => {
    expect(langName('zh-hant')).toBe('繁體中文');
  });

  it('returns Deutsch for de', () => {
    expect(langName('de')).toBe('Deutsch');
  });

  it('falls back to the input for unknown codes', () => {
    expect(langName('xx-unknown')).toBe('xx-unknown');
  });
});

describe('LANG_NAMES', () => {
  it('contains the en key', () => {
    expect('en' in LANG_NAMES).toBe(true);
    expect(LANG_NAMES['en']).toBe('English');
  });
});
