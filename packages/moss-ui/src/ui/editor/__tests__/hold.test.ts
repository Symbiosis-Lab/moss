import { afterEach, expect, test, vi } from 'vitest';
import { heldFor, isHeldFor, onHoldChange, setHold, showHold } from '../hold';

afterEach(() => {
  setHold('publish', false);
  setHold('commit', false);
});

test("a rename's release does not lift a publish's hold", () => {
  setHold('publish', true);
  setHold('commit', true);
  setHold('commit', false);
  expect(heldFor()).toBe('publish');
  expect(isHeldFor('commit')).toBe(false);
  setHold('publish', false);
  expect(heldFor()).toBeNull();
});

test('listeners hear a change once, and not a repeat', () => {
  const heard = vi.fn();
  const off = onHoldChange(heard);
  setHold('commit', true);
  setHold('commit', true);
  expect(heard).toHaveBeenCalledTimes(1);
  off();
});

test('a publish refuses a key in a field inside a pill and a right-click on it; the tree filter still types', () => {
  document.body.innerHTML =
    '<div id="moss-editor-root"><div id="chip-bar"><input id="pill"></div></div><div id="nav-grid"><input id="filter"></div>';
  const heard = vi.fn();
  for (const id of ['pill', 'filter']) {
    document.getElementById(id)!.addEventListener('keydown', () => heard(id));
  }
  document.getElementById('pill')!.addEventListener('contextmenu', () => heard('menu'));
  showHold('publish');
  for (const id of ['pill', 'filter']) {
    document.getElementById(id)!.dispatchEvent(new KeyboardEvent('keydown', { key: 'x', bubbles: true, cancelable: true }));
  }
  document.getElementById('pill')!.dispatchEvent(new Event('contextmenu', { bubbles: true, cancelable: true }));
  expect(heard.mock.calls).toEqual([['filter']]);
  showHold(null);
  document.body.innerHTML = '';
});
