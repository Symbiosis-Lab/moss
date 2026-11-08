import { describe, it, expect, afterEach } from 'vitest';
import { interpolate, t, localeTag, formatDate, setLocale } from '../index';

afterEach(() => setLocale('en'));

describe('interpolate', () => {
  it('substitutes a single named placeholder', () => {
    expect(interpolate('Hello {name}', { name: 'Guo' })).toBe('Hello Guo');
  });

  it('substitutes multiple placeholders and coerces numbers', () => {
    expect(interpolate('{n} of {total}', { n: 2, total: 5 })).toBe('2 of 5');
  });

  it('leaves unknown placeholders untouched', () => {
    expect(interpolate('Hi {who}', { name: 'x' })).toBe('Hi {who}');
  });

  it('is a no-op when there are no placeholders', () => {
    expect(interpolate('plain', { a: 1 })).toBe('plain');
  });
});

describe('t with params', () => {
  it('interpolates against a real key (publish.publishing_as uses {email})', () => {
    // English table value is "Publishing as {email}".
    expect(t('publish.publishing_as', { email: 'a@b.com' })).toContain('a@b.com');
  });

  it('returns the bare template when no params given', () => {
    expect(t('publish.publish_btn')).not.toContain('{');
  });
});

describe('localeTag', () => {
  it('maps runtime locales to title-case BCP-47', () => {
    expect(localeTag('en')).toBe('en');
    expect(localeTag('zh-hans')).toBe('zh-Hans');
    expect(localeTag('zh-hant')).toBe('zh-Hant');
  });
});

describe('formatDate', () => {
  it('renders 年月日 for Simplified Chinese', () => {
    setLocale('zh-hans');
    const out = formatDate(new Date(Date.UTC(2026, 5, 18)), {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
      timeZone: 'UTC',
    });
    expect(out).toContain('年');
    expect(out).toContain('月');
  });
});
