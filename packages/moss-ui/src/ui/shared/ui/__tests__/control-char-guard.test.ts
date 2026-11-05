/**
 * Unit tests for the control-character guard (shared/ui/control-char-guard.ts).
 *
 * Regression target: on macOS + Tauri multiwebview, arrow keys insert their
 * legacy control code (Right = 0x1D GROUP SEPARATOR) into plain inputs
 * The guard must drop those inserts while leaving normal text,
 * whitespace, and IME composition untouched.
 */

import { describe, it, expect, afterEach } from 'vitest';
import {
  containsControlChars,
  stripControlChars,
  shouldBlockInsertion,
  installControlCharGuard,
} from '../control-char-guard';

// Control chars built via fromCharCode so no literal control byte is in source.
const GS = String.fromCharCode(0x1d); // Right arrow's inserted code — the bug
const LEFT = String.fromCharCode(0x1c);
const UP = String.fromCharCode(0x1e);
const DOWN = String.fromCharCode(0x1f);
const NUL = String.fromCharCode(0x00);
const DEL = String.fromCharCode(0x7f);
const C1 = String.fromCharCode(0x85); // NEL, a C1 control
const TAB = String.fromCharCode(0x09);
const LF = String.fromCharCode(0x0a);
const CR = String.fromCharCode(0x0d);

describe('containsControlChars', () => {
  it('is false for ordinary text', () => {
    expect(containsControlChars('hello world')).toBe(false);
  });

  it('is false for CJK text', () => {
    expect(containsControlChars('邀请第一批写作者。')).toBe(false);
  });

  it('detects the arrow-key control codes 0x1C-0x1F', () => {
    for (const c of [LEFT, GS, UP, DOWN]) {
      expect(containsControlChars('websites.' + c)).toBe(true);
    }
  });

  it('detects NUL, DEL, and 0x80-0x9F controls', () => {
    expect(containsControlChars(NUL)).toBe(true);
    expect(containsControlChars(DEL)).toBe(true);
    expect(containsControlChars(C1)).toBe(true);
  });

  it('keeps TAB, LF, and CR (legitimate whitespace)', () => {
    expect(containsControlChars('a' + TAB + 'b')).toBe(false);
    expect(containsControlChars('a' + LF + 'b')).toBe(false);
    expect(containsControlChars('a' + CR + 'b')).toBe(false);
  });
});

describe('stripControlChars', () => {
  it('removes the trailing group separators from a corrupted value', () => {
    const corrupted = 'A desktop app that turns folders into websites.' + GS.repeat(8);
    expect(stripControlChars(corrupted)).toBe('A desktop app that turns folders into websites.');
  });

  it('preserves TAB/LF/CR while removing other controls', () => {
    const input = 'a' + TAB + LF + CR + NUL + DEL + 'b';
    expect(stripControlChars(input)).toBe('a' + TAB + LF + CR + 'b');
  });

  it('is a no-op for clean text', () => {
    expect(stripControlChars('nothing to strip')).toBe('nothing to strip');
  });
});

describe('shouldBlockInsertion', () => {
  it('blocks a control char typed as insertText', () => {
    expect(shouldBlockInsertion('insertText', GS, false)).toBe(true);
  });

  it('does NOT block paste/replacement at beforeinput (the input fallback strips those, keeping clean text)', () => {
    expect(shouldBlockInsertion('insertFromPaste', 'a' + GS + 'b', false)).toBe(false);
    expect(shouldBlockInsertion('insertReplacementText', 'a' + GS + 'b', false)).toBe(false);
  });

  it('allows ordinary and CJK text', () => {
    expect(shouldBlockInsertion('insertText', 'a', false)).toBe(false);
    expect(shouldBlockInsertion('insertText', '你好', false)).toBe(false);
  });

  it('never blocks while composing (IME)', () => {
    expect(shouldBlockInsertion('insertText', GS, true)).toBe(false);
  });

  it('ignores non-text inputTypes (IME commit, deletions)', () => {
    expect(shouldBlockInsertion('insertCompositionText', GS, false)).toBe(false);
    expect(shouldBlockInsertion('deleteContentBackward', '', false)).toBe(false);
  });
});

describe('installControlCharGuard (DOM)', () => {
  let uninstall: (() => void) | null = null;

  afterEach(() => {
    uninstall?.();
    uninstall = null;
    document.body.innerHTML = '';
  });

  function makeField(): HTMLTextAreaElement {
    const el = document.createElement('textarea');
    document.body.appendChild(el);
    return el;
  }

  it('cancels a beforeinput that inserts a control char', () => {
    uninstall = installControlCharGuard(document);
    const el = makeField();
    const evt = new InputEvent('beforeinput', {
      inputType: 'insertText',
      data: GS,
      cancelable: true,
      bubbles: true,
    });
    // dispatchEvent returns false when a listener called preventDefault().
    const notCancelled = el.dispatchEvent(evt);
    expect(notCancelled).toBe(false);
    expect(evt.defaultPrevented).toBe(true);
  });

  it('does not cancel ordinary typed text', () => {
    uninstall = installControlCharGuard(document);
    const el = makeField();
    const evt = new InputEvent('beforeinput', {
      inputType: 'insertText',
      data: 'x',
      cancelable: true,
      bubbles: true,
    });
    expect(el.dispatchEvent(evt)).toBe(true);
    expect(evt.defaultPrevented).toBe(false);
  });

  it('input-event fallback strips control chars and keeps the caret', () => {
    uninstall = installControlCharGuard(document);
    const el = makeField();
    el.value = 'ab' + GS + 'cd';
    el.setSelectionRange(5, 5); // caret after "cd" (index 5 in the dirty value)
    el.dispatchEvent(new InputEvent('input', { inputType: 'insertText', bubbles: true }));
    expect(el.value).toBe('abcd');
    expect(el.selectionStart).toBe(4); // one control char removed before the caret
  });

  it('preserves a spanning selection when stripping (selectionStart != selectionEnd)', () => {
    uninstall = installControlCharGuard(document);
    const el = makeField();
    el.value = 'ab' + GS + 'cd'; // control char at index 2
    el.setSelectionRange(1, 4); // selection spans the control char
    el.dispatchEvent(new InputEvent('input', { inputType: 'insertText', bubbles: true }));
    expect(el.value).toBe('abcd');
    // one control char removed before both ends: (1,4) -> (1,3)
    expect(el.selectionStart).toBe(1);
    expect(el.selectionEnd).toBe(3);
  });

  it('does NOT cancel a control-char paste at beforeinput (fallback handles it, keeping clean text)', () => {
    uninstall = installControlCharGuard(document);
    const el = makeField();
    const evt = new InputEvent('beforeinput', {
      inputType: 'insertFromPaste',
      data: 'hello' + GS + 'world',
      cancelable: true,
      bubbles: true,
    });
    // Not prevented — the whole paste is not discarded...
    expect(el.dispatchEvent(evt)).toBe(true);
    expect(evt.defaultPrevented).toBe(false);
    // ...and the input fallback then strips only the control char.
    el.value = 'hello' + GS + 'world';
    el.setSelectionRange(11, 11);
    el.dispatchEvent(new InputEvent('input', { inputType: 'insertFromPaste', bubbles: true }));
    expect(el.value).toBe('helloworld');
  });

  it('strips control chars on compositionend (post-IME commit)', () => {
    uninstall = installControlCharGuard(document);
    const el = makeField();
    el.value = 'a' + GS + 'b';
    el.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    expect(el.value).toBe('ab');
  });

  it('after uninstall, control chars are no longer blocked', () => {
    const off = installControlCharGuard(document);
    off();
    const el = makeField();
    const evt = new InputEvent('beforeinput', {
      inputType: 'insertText',
      data: GS,
      cancelable: true,
      bubbles: true,
    });
    expect(el.dispatchEvent(evt)).toBe(true);
    expect(evt.defaultPrevented).toBe(false);
  });
});
