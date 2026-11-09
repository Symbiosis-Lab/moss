import { describe, it, expect, afterEach } from 'vitest';
import { t, setLocale } from '../index';
import { UI_STRINGS, type AppLocale } from '../ui-strings';

function onPlatform(value: string, run: () => void): void {
  const original = Object.getOwnPropertyDescriptor(navigator, 'platform');
  Object.defineProperty(navigator, 'platform', { value, configurable: true });
  try {
    run();
  } finally {
    if (original) Object.defineProperty(navigator, 'platform', original);
  }
}

afterEach(() => setLocale('en'));

describe('platform wording', () => {
  it('names the file manager each platform has', () => {
    onPlatform('MacIntel', () => expect(t('tree.reveal')).toBe('Reveal in Finder'));
    onPlatform('Win32', () => expect(t('tree.reveal')).toBe('Show in folder'));
    onPlatform('Linux x86_64', () => expect(t('tree.reveal')).toBe('Show in folder'));
  });

  it('says Recycle Bin on Windows and Trash elsewhere', () => {
    onPlatform('Win32', () => expect(t('tree.trash_one', { name: 'a.md' })).toBe('Move "a.md" to the Recycle Bin?'));
    onPlatform('MacIntel', () => expect(t('tree.trash_one', { name: 'a.md' })).toBe('Move "a.md" to Trash?'));
    onPlatform('Linux x86_64', () => expect(t('tree.trash_one', { name: 'a.md' })).toBe('Move "a.md" to Trash?'));
    setLocale('zh-hans');
    onPlatform('Win32', () => expect(t('tree.delete_trash')).toBe('移到回收站'));
  });

  it('never names the Finder off macOS, in any locale', () => {
    for (const loc of Object.keys(UI_STRINGS) as AppLocale[]) {
      setLocale(loc);
      onPlatform('Win32', () => {
        for (const key of Object.keys(UI_STRINGS.en) as (keyof typeof UI_STRINGS.en)[]) {
          expect(t(key), `${loc} ${key}`).not.toMatch(/Finder|访达/);
        }
      });
    }
  });

  // German's Papierkorb is the word on both platforms, so de is not checked.
  it('never says Trash on Windows', () => {
    for (const loc of ['en', 'zh-hans', 'zh-hant'] as AppLocale[]) {
      setLocale(loc);
      onPlatform('Win32', () => {
        for (const key of Object.keys(UI_STRINGS.en) as (keyof typeof UI_STRINGS.en)[]) {
          expect(t(key), `${loc} ${key}`).not.toMatch(/trash|废纸篓|垃圾桶/i);
        }
      });
    }
  });
});
