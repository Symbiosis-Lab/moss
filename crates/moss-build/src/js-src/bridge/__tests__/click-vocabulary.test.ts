// Click-count disambiguation for the preview-click vocabulary.
//
// There is deliberately NO delay timer: a 250–300ms hold would make every
// single-click highlight land perceptibly late, and the vocabulary makes it
// unnecessary — the single action (reveal, no focus) is a strict prefix of the
// double (jump), so the first click of a double may fire immediately and be
// superseded. What must NOT happen is the reveal firing twice, which is what
// the detail>1 suppression is for.
import { describe, test, expect } from 'vitest';
import { classifyClickGesture } from '../click-vocabulary';

describe('classifyClickGesture', () => {
  test('a lone click is the reveal, with zero latency', () => {
    expect(classifyClickGesture('click', 1)).toBe('click');
  });

  test('the second click of a double is suppressed — dblclick owns the gesture', () => {
    // Browser sequence for a double click: click(1), click(2), dblclick.
    expect(classifyClickGesture('click', 2)).toBe(null);
    // A triple click (paragraph selection) escalates the same way.
    expect(classifyClickGesture('click', 3)).toBe(null);
  });

  test('dblclick is the jump regardless of detail', () => {
    expect(classifyClickGesture('dblclick', 2)).toBe('dblclick');
    // Some engines report detail differently on synthetic events; the type wins.
    expect(classifyClickGesture('dblclick', 0)).toBe('dblclick');
  });

  test('a full double-click sequence yields exactly one reveal then one jump', () => {
    const posted = (['click', 'click', 'dblclick'] as const)
      .map((type, i) => classifyClickGesture(type, type === 'click' ? i + 1 : 2))
      .filter((x) => x !== null);
    expect(posted).toEqual(['click', 'dblclick']);
  });
});
