import { describe, it, expect, afterEach } from 'vitest';
import { kbdLabel, isMacPlatform, matchesKeySpec, UNDO_KEY, REDO_KEY } from '../kbd-label';

/** jsdom reports a Linux platform, so mac has to be simulated. */
function withPlatform(value: string, run: () => void): void {
  const original = Object.getOwnPropertyDescriptor(navigator, 'platform');
  Object.defineProperty(navigator, 'platform', { value, configurable: true });
  try {
    run();
  } finally {
    if (original) Object.defineProperty(navigator, 'platform', original);
  }
}

afterEach(() => {
  delete (navigator as Navigator & { userAgentData?: unknown }).userAgentData;
});

describe('kbdLabel', () => {
  it('prints Command glyphs on macOS', () => {
    withPlatform('MacIntel', () => {
      expect(kbdLabel('Mod-Z')).toBe('⌘Z');
      expect(kbdLabel('Mod-Shift-Z')).toBe('⇧⌘Z');
      expect(kbdLabel('Mod-Alt-Shift-K')).toBe('⌥⇧⌘K'); // ⌃⌥⇧⌘, Apple's fixed order
    });
  });

  it('prints the words those platforms print, everywhere else', () => {
    // The reason the helper exists: undo/redo's keymap is registered inside
    // CodeMirror, so Ctrl+Z genuinely works on Linux and Windows — where there
    // is no Edit menu and the keymap is therefore the ONLY path to it. A
    // hard-coded ⌘ there misnames a working key.
    withPlatform('Linux x86_64', () => {
      expect(kbdLabel('Mod-Z')).toBe('Ctrl+Z');
      expect(kbdLabel('Mod-Shift-Z')).toBe('Ctrl+Shift+Z');
    });
    withPlatform('Win32', () => {
      expect(kbdLabel('Mod-Z')).toBe('Ctrl+Z');
    });
  });

  it('prefers userAgentData, which is where Chromium still answers honestly', () => {
    // navigator.platform is deprecated and frozen in some engines; the tooltip
    // would silently go wrong on the platform it is read on.
    withPlatform('Linux x86_64', () => {
      Object.defineProperty(navigator, 'userAgentData', {
        value: { platform: 'macOS' },
        configurable: true,
      });
      expect(isMacPlatform()).toBe(true);
      expect(kbdLabel('Mod-Z')).toBe('⌘Z');
    });
  });

  it('orders mac modifiers the way macOS does, not the way they were written', () => {
    // ⌃⌥⇧⌘ is the platform's fixed order; a menu that prints them in source
    // order reads as a different shortcut.
    withPlatform('MacIntel', () => {
      expect(kbdLabel('Shift-Ctrl-Alt-Mod-A')).toBe('⌃⌥⇧⌘A');
    });
  });
});

describe('matchesKeySpec', () => {
  const key = (init: KeyboardEventInit) => new KeyboardEvent('keydown', init);

  it('resolves Mod to Command on macOS and Control everywhere else', () => {
    withPlatform('MacIntel', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z', metaKey: true }))).toBe(true);
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z', ctrlKey: true }))).toBe(false);
    });
    withPlatform('Linux x86_64', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z', ctrlKey: true }))).toBe(true);
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z', metaKey: true }))).toBe(false);
    });
  });

  it('does not let undo swallow redo', () => {
    // Mod-z and Mod-Shift-z differ ONLY in a modifier the spec has to compare
    // even when it does not ask for it. A matcher that ignored an unlisted
    // Shift would fire undo on ⇧⌘Z and redo would never be reachable.
    withPlatform('Linux x86_64', () => {
      const redoChord = key({ key: 'Z', ctrlKey: true, shiftKey: true });
      expect(matchesKeySpec(UNDO_KEY, redoChord)).toBe(false);
      expect(matchesKeySpec(REDO_KEY, redoChord)).toBe(true);
      expect(matchesKeySpec(REDO_KEY, key({ key: 'z', ctrlKey: true }))).toBe(false);
    });
  });

  it('refuses a chord that carries the other platform modifier too', () => {
    // ⌃⌘Z is somebody else's shortcut. Comparing only the wanted modifier would
    // claim it. BOTH halves, on BOTH platforms: with only the mac case tested,
    // weakening the meta comparison to "wanted and absent" — which lets ⌘Z on
    // Linux match Ctrl-z — went unnoticed.
    withPlatform('MacIntel', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z', metaKey: true, ctrlKey: true }))).toBe(false);
    });
    withPlatform('Linux x86_64', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z', metaKey: true, ctrlKey: true }))).toBe(false);
    });
  });

  it('refuses a chord carrying an Alt the spec never asked for', () => {
    // ⌃⌥Z is a distinct chord (a word-wise binding in several editors). Without
    // the comparison this matcher claims it as undo.
    withPlatform('Linux x86_64', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z', ctrlKey: true, altKey: true }))).toBe(false);
    });
  });

  it('falls back to the US-layout key when the layout prints something else', () => {
    // On a Russian layout the Z key reports `e.key === 'я'`, so a name
    // comparison alone finds no shortcut. CM6's own keymap has this fallback
    // (`base[event.keyCode]` whenever a modifier is held), which is why ⌃Z used
    // to work inside the editor body and NOWHERE else in the pane — exactly the
    // per-surface divergence this matcher exists to remove.
    withPlatform('Linux x86_64', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'я', ctrlKey: true, keyCode: 90 }))).toBe(true);
      expect(
        matchesKeySpec(REDO_KEY, key({ key: 'Я', ctrlKey: true, shiftKey: true, keyCode: 90 })),
      ).toBe(true);
      // The fallback is modifier-gated, as CM6 has it: a bare 'я' is a letter
      // the user is typing, not a shortcut whose name happens to sit under it.
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'я', keyCode: 90 }))).toBe(false);
      // And it does not widen the match: a different physical key is still no.
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'ф', ctrlKey: true, keyCode: 65 }))).toBe(false);
    });
  });

  it('never re-reads a key that already names a Latin character', () => {
    // The fallback maps `keyCode` through a US table, so on a German layout the
    // key printing `y` sits where US has `z` and reports keyCode 90. Running it
    // anyway makes ⌘Y answer "this is ⌘Z" — and since callers ask about undo
    // before redo, the undo spec claims the keypress and ⌘Y UNDOES on every
    // QWERTZ machine, defeating the platform gate that sends ⌘Y to redo.
    withPlatform('MacIntel', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'y', metaKey: true, keyCode: 90 }))).toBe(false);
    });
    withPlatform('Linux x86_64', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'y', ctrlKey: true, keyCode: 90 }))).toBe(false);
      // The key that DOES print z still matches by name, fallback or no.
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z', ctrlKey: true, keyCode: 89 }))).toBe(true);
    });
  });

  it('does not re-read the placeholder names an IME reports', () => {
    // `Dead`, `Process` and `Unidentified` are not one character and are not a
    // letter the user pressed; reading them as whatever sits under the physical
    // key turns a compose sequence into a shortcut.
    withPlatform('Linux x86_64', () => {
      for (const name of ['Dead', 'Process', 'Unidentified']) {
        expect(matchesKeySpec(UNDO_KEY, key({ key: name, ctrlKey: true, keyCode: 90 }))).toBe(false);
      }
    });
  });

  it('does not re-read a SHIFTED Latin character either', () => {
    // Shift makes `e.key` uppercase, so the "is this already a Latin letter"
    // test has to be case-insensitive. Without that, ⇧⌘Y on a QWERTZ layout —
    // whose Y sits on the physical Z key, keyCode 90 — would fall through to
    // the keyCode and claim the redo shortcut from a capital Y.
    withPlatform('MacIntel', () => {
      expect(matchesKeySpec(REDO_KEY, key({ key: 'Y', metaKey: true, shiftKey: true, keyCode: 90 })))
        .toBe(false);
    });
  });

  it('stands aside mid-IME-composition', () => {
    // The keystroke belongs to the IME's own undo-conversion. Claiming it both
    // walks the document log and preventDefaults the conversion, and on Linux
    // and Windows that keypress previously did nothing at all.
    withPlatform('Linux x86_64', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z', ctrlKey: true, isComposing: true }))).toBe(
        false,
      );
    });
  });

  it('ignores case, because Shift reports the capital', () => {
    withPlatform('Linux x86_64', () => {
      expect(matchesKeySpec(REDO_KEY, key({ key: 'Z', ctrlKey: true, shiftKey: true }))).toBe(true);
    });
  });

  it('does not fire on a bare key', () => {
    withPlatform('Linux x86_64', () => {
      expect(matchesKeySpec(UNDO_KEY, key({ key: 'z' }))).toBe(false);
    });
  });
});
