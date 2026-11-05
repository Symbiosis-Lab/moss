import { describe, it, expect, afterEach, vi } from 'vitest';
import { channelName, chipLabel, setLocale } from '../index';
import { UI_STRINGS, type AppLocale } from '../ui-strings';

// Reset locale between tests so a leftover locale can't leak across cases.
afterEach(() => setLocale('en'));

/**
 * Guards that the displayed name of a channel moss SHIPS
 * must come from the i18n system (`channels.name.<id>` in ui-strings.ts) — NOT
 * from the raw Rust `channel.name` registry string. channelName is the only
 * path that produces a channel's UI label.
 *
 * The exception, and it is not a loophole: a plugin moss does not ship cannot
 * have a key, because moss did not know it existed when its strings were
 * written. Those rows pass the registry's `display_name` as the second
 * argument and get it. Everything with a key is unaffected, and an id with
 * neither still warns.
 */
describe('channelName', () => {
  it('localizes the email channel per locale', () => {
    setLocale('en');
    expect(channelName('email')).toBe('Email Newsletter');
    setLocale('de');
    expect(channelName('email')).toBe('E-Mail-Newsletter');
    setLocale('zh-hans');
    expect(channelName('email')).toBe('邮件订阅');
    setLocale('zh-hant');
    expect(channelName('email')).toBe('電子報');
  });

  it('returns "Matters" (brand, untranslated) in all locales', () => {
    for (const locale of Object.keys(UI_STRINGS) as AppLocale[]) {
      setLocale(locale);
      expect(channelName('matters')).toBe('Matters');
    }
  });

  it('returns "GitHub" (brand, untranslated) in all locales', () => {
    for (const locale of Object.keys(UI_STRINGS) as AppLocale[]) {
      setLocale(locale);
      expect(channelName('github')).toBe('GitHub');
    }
  });

  it('uses the registry display name for a plugin moss does not ship', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      // A third-party id: no key exists and none ever can.
      expect(channelName('stranger', 'Stranger Publishing')).toBe('Stranger Publishing');
      // Not a bug, so not a warning — otherwise every catalog render of a
      // registry row would log, and the warning would stop meaning anything.
      expect(warn).not.toHaveBeenCalled();
    } finally {
      warn.mockRestore();
    }
  });

  it('lets a name passed in win, because Rust sends one only where the key would misname the row', () => {
    setLocale('zh-hans');
    // `published_name` is None for every id moss ships, so a shipped channel
    // is normally called by its key and stays localized.
    expect(channelName('email')).toBe('邮件订阅');
    // The one case where Rust does send a name for such an id: a folder's own
    // bytes sitting under that id, which the catalog re-identifies from the
    // manifest. Calling that row 邮件订阅 would name the wrong plugin.
    expect(channelName('email', 'Something Else Entirely')).toBe('Something Else Entirely');
  });

  it('warns and Title-Cases the id for an unknown channel (never a raw English name)', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      const out = channelName('unknown-xyz');

      // Warns loudly so a missing key is caught in dev.
      expect(warn).toHaveBeenCalledWith(
        expect.stringContaining('channels.name.unknown-xyz')
      );
      // Last-resort label is the Title-Cased id, not a hardcoded channel name.
      expect(out).toBe('Unknown Xyz');
      // And crucially not the id verbatim / not any raw registry string.
      expect(out).not.toBe('unknown-xyz');
    } finally {
      warn.mockRestore();
    }
  });

  it('resolves every known channel through the i18n map, not a raw channel.name', () => {
    // The returned value must be the localized i18n string, so a
    // raw Rust registry `name` can never reach the UI. We prove the value came
    // from the i18n map by (a) it being defined and (b) email actually varying
    // by locale — impossible if it were a single hardcoded English constant.
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      for (const id of ['email', 'matters', 'github']) {
        setLocale('en');
        const label = channelName(id);
        expect(label.length).toBeGreaterThan(0);
        // No missing-key warning fired for a known channel → it hit the map.
        expect(warn).not.toHaveBeenCalled();
      }

      // The email label is locale-dependent → it is resolved via the i18n map,
      // not a raw English constant from the Rust channel registry.
      setLocale('en');
      const en = channelName('email');
      setLocale('zh-hans');
      const zh = channelName('email');
      expect(en).not.toBe(zh);
    } finally {
      warn.mockRestore();
    }
  });

  it('gives a sideloaded plugin no way to write the sentence around its name', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      // A folder in `.moss/plugins/` names itself; nothing validates that name,
      // and it lands in `aria-label` and `data-tooltip` in the publish rail.
      const hostile = channelName(`evil\u202e${'x'.repeat(200)}`);
      expect(hostile).not.toContain('\u202e');
      expect([...hostile].length).toBeLessThan(70);

      // And a published name is bounded the same way on the way through.
      const published = channelName('unkeyed', `Fine\u200fname${'y'.repeat(200)}`);
      expect(published).not.toContain('\u200f');
      expect([...published].length).toBeLessThan(70);
    } finally {
      warn.mockRestore();
    }
  });
});

/**
 * A plugin contributes frontmatter fields, so the key `chipLabel` falls back
 * to drawing is a string the plugin author wrote. It lands in `textContent`
 * beside moss's own chip labels — the one place in this file where a name moss
 * did not write is displayed, which is the same case `displayable` exists for.
 */
describe('chipLabel', () => {
  it('draws a builtin field from the locale, not from its key', () => {
    setLocale('zh-hans');
    expect(chipLabel('title')).not.toBe('title');
  });

  it('strips the bidi controls out of a plugin-contributed field key', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    expect(chipLabel('draft‮yfward')).toBe('draftyfward');
  });

  it('cuts a plugin-contributed field key that has no end', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    const drawn = chipLabel('x'.repeat(400));
    expect([...drawn]).toHaveLength(65);
    expect(drawn.endsWith('…')).toBe(true);
  });
});
