import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { EditorView } from '@codemirror/view';
import { EditorState, Text } from '@codemirror/state';
import { wrapSelection, detectToggle } from '../selection-toolbar';

describe('wrapSelection', () => {
  test('wraps text with bold markers', () => {
    expect(wrapSelection('hello', 'bold')).toBe('**hello**');
  });

  test('wraps text with italic markers', () => {
    expect(wrapSelection('hello', 'italic')).toBe('*hello*');
  });

  test('wraps text with strikethrough markers', () => {
    expect(wrapSelection('hello', 'strike')).toBe('~~hello~~');
  });

  test('wraps text with code markers', () => {
    expect(wrapSelection('hello', 'code')).toBe('`hello`');
  });

  test('wraps text with link syntax', () => {
    expect(wrapSelection('hello', 'link')).toBe('[hello]()');
  });

  test('handles empty string', () => {
    expect(wrapSelection('', 'bold')).toBe('****');
  });

  test('handles text with existing markers', () => {
    expect(wrapSelection('**already bold**', 'bold')).toBe('****already bold****');
  });
});

describe('detectToggle', () => {
  function doc(s: string) { return Text.of(s.split('\n')); }

  test('detects bold toggle-off when inner text selected', () => {
    // doc: **hello** — select positions 2–7 ("hello")
    const result = detectToggle(doc('**hello**'), 2, 7, 'bold');
    expect(result.isToggle).toBe(true);
    expect(result.removeFrom).toBe(0);   // position of opening **
    expect(result.removeTo).toBe(9);     // position after closing **
  });

  // Also the document-boundary case: from=0 leaves no room for a leading `**`.
  test('no toggle when text not surrounded by bold', () => {
    const result = detectToggle(doc('hello'), 0, 5, 'bold');
    expect(result.isToggle).toBe(false);
  });

  test('detects italic toggle-off', () => {
    // doc: *hi* — select positions 1–3 ("hi")
    const result = detectToggle(doc('*hi*'), 1, 3, 'italic');
    expect(result.isToggle).toBe(true);
    expect(result.removeFrom).toBe(0);
    expect(result.removeTo).toBe(4);
  });

  test('detects strikethrough toggle-off', () => {
    // doc: ~~foo~~ — select positions 2–5 ("foo")
    const result = detectToggle(doc('~~foo~~'), 2, 5, 'strike');
    expect(result.isToggle).toBe(true);
    expect(result.removeFrom).toBe(0);
    expect(result.removeTo).toBe(7);
  });

  test('detects code toggle-off', () => {
    // doc: `bar` — select positions 1–4 ("bar")
    const result = detectToggle(doc('`bar`'), 1, 4, 'code');
    expect(result.isToggle).toBe(true);
    expect(result.removeFrom).toBe(0);
    expect(result.removeTo).toBe(5);
  });

  test('detects link toggle-off (empty URL)', () => {
    // doc: [click]() — select positions 1–6 ("click")
    const result = detectToggle(doc('[click]()'), 1, 6, 'link');
    expect(result.isToggle).toBe(true);
    expect(result.removeFrom).toBe(0);
    expect(result.removeTo).toBe(9);
  });

  test('detects link toggle-off (non-empty URL)', () => {
    // doc: [click](https://x.com) — select positions 1–6 ("click")
    const result = detectToggle(doc('[click](https://x.com)'), 1, 6, 'link');
    expect(result.isToggle).toBe(true);
    expect(result.removeFrom).toBe(0);
    expect(result.removeTo).toBe(22);
  });

  test('partial mark selection does not toggle (falls through to wrap)', () => {
    // doc: **abc — select 0–5, chars before pos 0 are "" not "**"
    const result = detectToggle(doc('**abc'), 0, 5, 'bold');
    expect(result.isToggle).toBe(false);
  });

  test('detects link toggle-off for URL with parentheses (Wikipedia-style)', () => {
    // doc: [text](https://en.wikipedia.org/wiki/Foo_(bar)) — select "text" at 1–5
    const result = detectToggle(doc('[text](https://en.wikipedia.org/wiki/Foo_(bar))'), 1, 5, 'link');
    expect(result.isToggle).toBe(true);
    expect(result.removeFrom).toBe(0);
    expect(result.removeTo).toBe(47); // full length of the string
  });

  test('detects link toggle-off for URL with nested parentheses', () => {
    // doc: [text](url(a(b))) — select "text" at 1–5
    const result = detectToggle(doc('[text](url(a(b)))'), 1, 5, 'link');
    expect(result.isToggle).toBe(true);
    expect(result.removeFrom).toBe(0);
    expect(result.removeTo).toBe(17);
  });

  test('does not toggle italic when selection is inside **bold** markers', () => {
    // doc: **bold** — select "bold" at 2–6, action italic
    // chars outside: * at pos 1 and 7 — but outer chars are also *, so no toggle
    const result = detectToggle(doc('**bold**'), 2, 6, 'italic');
    expect(result.isToggle).toBe(false);
  });
});
