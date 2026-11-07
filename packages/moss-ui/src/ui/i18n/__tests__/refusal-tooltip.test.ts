import { afterEach, describe, expect, it } from 'vitest';
import { refusalTooltip, setLocale, type RefusalLike } from '../index';
import type { AppLocale } from '../ui-strings';

const LOCALES: AppLocale[] = ['en', 'de', 'zh-hans', 'zh-hant'];

describe('refusalTooltip for an unavailable plugin', () => {
  afterEach(() => setLocale('en'));

  it.each(LOCALES)('names the plugin and translates every platform in %s', (l) => {
    setLocale(l);
    const text = refusalTooltip(
      { kind: 'unavailable', platforms: ['darwin-arm64', 'linux-x64', 'darwin-arm64'] },
      'PDF OCR',
    );
    expect(text).toContain('PDF OCR');
    expect(text).not.toMatch(/darwin|linux-x64|\{/);
  });

  it('joins platforms in the locale and shows an unknown key as written', () => {
    expect(
      refusalTooltip({ kind: 'unavailable', platforms: ['darwin-arm64', 'windows-x64'] }, 'X'),
    ).toBe('Not available on this computer: X runs on Apple silicon Macs and Windows computers only.');
    expect(refusalTooltip({ kind: 'unavailable', platforms: ['plan9-mips'] }, 'X')).toContain('plan9-mips');
    expect(refusalTooltip({ kind: 'unavailable', platforms: [] }, 'X')).toContain('other computers');
  });

  it('still words the older refusals', () => {
    const cases: RefusalLike[] = [
      { kind: 'revoked', reason: 'bad' },
      { kind: 'incompatible', needs: '1.2', host: '1.0' },
      { kind: 'unapproved' },
    ];
    for (const c of cases) expect(refusalTooltip(c, 'X')).toContain('X');
  });

  it.each(LOCALES)('names the plugin and the floor it cannot read in %s', (l) => {
    setLocale(l);
    const text = refusalTooltip({ kind: 'unreadable_floor', floor: 'whenever' }, 'PDF OCR');
    expect(text).toContain('PDF OCR');
    expect(text).toContain('whenever');
    expect(text).not.toMatch(/\{/);
  });
});
