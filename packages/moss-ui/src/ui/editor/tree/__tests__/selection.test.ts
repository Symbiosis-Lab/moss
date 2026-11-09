import { describe, test, expect } from 'vitest';
import { FileSelection } from '../selection';
import type { EntryId } from '../../entry-registry';

const id = (n: number) => n as unknown as EntryId;

describe('FileSelection', () => {
  test('starts empty', () => {
    const s = new FileSelection();
    expect(s.size).toBe(0);
    expect(s.anchor).toBeNull();
    expect(s.focus).toBeNull();
  });

  test('setSingle replaces selection, sets anchor and focus', () => {
    const s = new FileSelection();
    s.setSingle(id(1));
    expect(s.has(id(1))).toBe(true);
    expect(s.size).toBe(1);
    expect(s.anchor).toBe(id(1));
    expect(s.focus).toBe(id(1));
    s.setSingle(id(2));
    expect(s.has(id(1))).toBe(false);
    expect(s.has(id(2))).toBe(true);
    expect(s.anchor).toBe(id(2));
  });

  test('toggle adds and removes; anchor stays at last additive action', () => {
    const s = new FileSelection();
    s.toggle(id(1));
    expect(s.has(id(1))).toBe(true);
    expect(s.anchor).toBe(id(1));
    s.toggle(id(2));
    expect(s.size).toBe(2);
    expect(s.anchor).toBe(id(2));   // additive — anchor moves
    s.toggle(id(1));                // deselect
    expect(s.has(id(1))).toBe(false);
    expect(s.anchor).toBe(id(2));   // deselect — anchor stays (Finder/VS Code convention)
    expect(s.focus).toBe(id(1));    // focus moves to the deselected row
  });

  test('reconcile falls back anchor and focus to a surviving member', () => {
    const s = new FileSelection();
    s.toggle(id(1));
    s.toggle(id(2));
    s.toggle(id(3));                // anchor = focus = 3
    s.reconcile(new Set([id(1), id(2)]));
    expect(s.size).toBe(2);
    // anchor and focus fell back to a surviving member (Set iteration order)
    expect(s.anchor).not.toBeNull();
    expect([id(1), id(2)]).toContain(s.anchor);
    expect(s.focus).not.toBeNull();
    expect([id(1), id(2)]).toContain(s.focus);
  });

  test('computeRange returns just [to] when anchor is not in visible order', () => {
    // Anchor was inside a now-collapsed folder; shift+click should silently
    // shrink to the clicked item, not throw.
    const s = new FileSelection();
    s.setSingle(id(99));                              // anchor=99, but 99 not in visible
    s.setRange(id(5), [id(1), id(2), id(3), id(4), id(5)]);
    expect(s.size).toBe(1);
    expect(s.has(id(5))).toBe(true);
    expect(s.focus).toBe(id(5));
  });

  test('setRange selects inclusive range in given visible order', () => {
    const s = new FileSelection();
    s.setSingle(id(1));
    s.setRange(id(4), [id(1), id(2), id(3), id(4), id(5)]);
    expect(s.size).toBe(4);
    [1, 2, 3, 4].forEach(n => expect(s.has(id(n))).toBe(true));
    expect(s.has(id(5))).toBe(false);
    expect(s.anchor).toBe(id(1));   // anchor unchanged
    expect(s.focus).toBe(id(4));    // focus moves to clicked
  });

  test('setRange works in reverse order', () => {
    const s = new FileSelection();
    s.setSingle(id(4));
    s.setRange(id(1), [id(1), id(2), id(3), id(4), id(5)]);
    expect(s.size).toBe(4);
    [1, 2, 3, 4].forEach(n => expect(s.has(id(n))).toBe(true));
  });

  test('addRange unions with existing selection', () => {
    const s = new FileSelection();
    s.setSingle(id(1));
    s.toggle(id(7));                              // {1, 7}, anchor=7
    s.addRange(id(9), [id(7), id(8), id(9)]);     // adds 7..9 to {1,7}
    expect(s.has(id(1))).toBe(true);
    expect(s.has(id(7))).toBe(true);
    expect(s.has(id(8))).toBe(true);
    expect(s.has(id(9))).toBe(true);
    expect(s.size).toBe(4);
  });

  test('clear empties everything', () => {
    const s = new FileSelection();
    s.setSingle(id(1));
    s.toggle(id(2));
    s.clear();
    expect(s.size).toBe(0);
    expect(s.anchor).toBeNull();
    expect(s.focus).toBeNull();
  });

  test('setFocus does not change selection', () => {
    const s = new FileSelection();
    s.setSingle(id(1));
    s.setFocus(id(5));
    expect(s.has(id(1))).toBe(true);
    expect(s.has(id(5))).toBe(false);
    expect(s.focus).toBe(id(5));
    expect(s.anchor).toBe(id(1));
  });

  test('reconcile drops ids not in the valid set', () => {
    const s = new FileSelection();
    s.setSingle(id(1));
    s.toggle(id(2));
    s.toggle(id(3));
    s.reconcile(new Set([id(1), id(3)]));
    expect(s.has(id(1))).toBe(true);
    expect(s.has(id(2))).toBe(false);
    expect(s.has(id(3))).toBe(true);
    expect(s.anchor).toBe(id(3));   // last anchor was 3, still valid after reconcile
    expect(s.focus).toBe(id(3));
  });

  test('reconcile nulls anchor and focus when they vanish', () => {
    const s = new FileSelection();
    s.setSingle(id(99));
    s.reconcile(new Set([id(1)]));
    expect(s.size).toBe(0);
    expect(s.anchor).toBeNull();
    expect(s.focus).toBeNull();
  });

  test('snapshot returns a defensive copy', () => {
    const s = new FileSelection();
    s.setSingle(id(1));
    const snap = s.snapshot();
    s.toggle(id(2));
    expect(snap.selected.size).toBe(1);   // snapshot is frozen view
    expect(s.size).toBe(2);
  });
});
