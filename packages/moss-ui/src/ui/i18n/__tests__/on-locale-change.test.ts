import { describe, it, expect, afterEach } from 'vitest';
import { setLocale, onLocaleChange, getLocale } from '../index';

// Reset the module-global locale so tests are order-independent (mirrors the
// existing i18n test convention). setLocale no-ops when already 'en'.
afterEach(() => setLocale('en'));

describe('onLocaleChange', () => {
  it('notifies subscribers on change, with the new locale already live', () => {
    const seen: string[] = [];
    const off = onLocaleChange(() => seen.push(getLocale()));
    setLocale('de');
    setLocale('zh-hans');
    setLocale('zh-hant');
    expect(seen).toEqual(['de', 'zh-hans', 'zh-hant']);
    off();
    setLocale('en');
    expect(seen).toEqual(['de', 'zh-hans', 'zh-hant']); // unsubscribed → no more calls
  });

  it('does not notify when the locale is unchanged', () => {
    let n = 0;
    const off = onLocaleChange(() => (n += 1));
    setLocale('en'); // already 'en' after the afterEach reset
    expect(n).toBe(0);
    off();
  });

  it('is safe when a subscriber mutates the subscriber set during notify', () => {
    const order: string[] = [];
    // A subscriber that unsubscribes itself and adds a new one mid-notification.
    // Iterating a copy must not throw or skip/duplicate the surviving subscribers.
    const offB = onLocaleChange(() => order.push('B'));
    const offSelf = onLocaleChange(() => {
      order.push('A');
      offSelf();
      onLocaleChange(() => order.push('C'));
    });
    expect(() => setLocale('zh-hans')).not.toThrow();
    // Both original subscribers ran exactly once this round.
    expect(order.filter((x) => x === 'A')).toHaveLength(1);
    expect(order.filter((x) => x === 'B')).toHaveLength(1);
    // The newly-added 'C' only fires on the NEXT change, not this one.
    expect(order).not.toContain('C');
    setLocale('zh-hant');
    expect(order).toContain('C');
    offB();
  });
});
